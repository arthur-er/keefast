//! Browser integration in the app: serves the native host's connections on the local socket,
//! and runs each connection's requests through a [`BrowserSession`] on the Workspace. Pairing
//! requests wait for the user to answer them, one at a time.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::mpsc;
use std::thread;

use gpui::{Context, Entity, Task};
use interprocess::local_socket::traits::{ListenerExt as _, Stream as _};
use keefast_native_host::ipc::{self, read_frame, write_frame};
use keefast_native_host::relay::not_running_reply;
use keefast_workspace::{
    BrowserReply, BrowserSession, EditVaultError, PairingRequest, VaultId, Workspace,
};

use crate::saving;

pub struct BrowserIntegration {
    workspace: Entity<Workspace>,
    connections: HashMap<u64, Connection>,
    pairings: VecDeque<PendingPairing>,
    next_pairing: u64,
    serving: Option<Task<()>>,
    /// Why the app can't serve the native host, if it can't.
    serve_error: Option<io::Error>,
}

/// One native host, that is one browser extension, connected to the app.
struct Connection {
    session: BrowserSession,
    replies: mpsc::Sender<Vec<u8>>,
}

impl Connection {
    fn send(&self, reply: String) {
        // A host that has gone is noticed when its connection closes.
        let _ = self.replies.send(reply.into_bytes());
    }
}

struct PendingPairing {
    id: u64,
    connection: u64,
    request: PairingRequest,
}

/// What the socket threads tell the app.
enum Event {
    Opened(u64, mpsc::Sender<Vec<u8>>),
    Message(u64, Vec<u8>),
    Closed(u64),
    Failed(io::Error),
}

impl BrowserIntegration {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        cx.observe(&workspace, |this, _, cx| this.follow_setting(cx))
            .detach();
        let mut integration = Self {
            workspace,
            connections: HashMap::new(),
            pairings: VecDeque::new(),
            next_pairing: 0,
            serving: None,
            serve_error: None,
        };
        integration.follow_setting(cx);
        integration
    }

    /// The Pairing request to put to the user now, and the Vault it is for.
    pub fn pending_pairing(&self) -> Option<(u64, VaultId)> {
        self.pairings
            .front()
            .map(|pending| (pending.id, pending.request.vault()))
    }

    pub fn serve_error(&self) -> Option<&io::Error> {
        self.serve_error.as_ref()
    }

    /// Pairs the extension that asked with the Vault, as `name`. A name the Vault can't take
    /// is refused, and the request stays pending.
    pub fn approve_pairing(
        &mut self,
        pairing: u64,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), EditVaultError> {
        let Some(pending) = self
            .pairings
            .front()
            .filter(|pending| pending.id == pairing)
        else {
            return Ok(());
        };
        if let Some(connection) = self.connections.get(&pending.connection) {
            let reply = self.workspace.update(cx, |workspace, cx| {
                cx.notify();
                connection
                    .session
                    .approve_pairing(workspace, &pending.request, name)
            })?;
            connection.send(reply);
            saving::autosave(&self.workspace, cx);
        }
        self.pairings.pop_front();
        cx.notify();
        Ok(())
    }

    pub fn deny_pairing(&mut self, pairing: u64, cx: &mut Context<Self>) {
        if self
            .pairings
            .front()
            .is_none_or(|pending| pending.id != pairing)
        {
            return;
        }
        let pending = self.pairings.pop_front().expect("checked above");
        if let Some(connection) = self.connections.get(&pending.connection) {
            connection.send(connection.session.deny_pairing(&pending.request));
        }
        cx.notify();
    }

    /// Serves the native host once browser integration is turned on. Turned off, every
    /// extension loses its session and its pending Pairing, and is told keefast isn't there.
    fn follow_setting(&mut self, cx: &mut Context<Self>) {
        if self.workspace.read(cx).browser_integration() {
            if self.serving.is_none() {
                self.serve(cx);
            }
            return;
        }
        let had_pairings = !self.pairings.is_empty();
        for pending in self.pairings.drain(..) {
            if let Some(connection) = self.connections.get(&pending.connection) {
                connection.send(connection.session.deny_pairing(&pending.request));
            }
        }
        for connection in self.connections.values_mut() {
            connection.session = BrowserSession::new();
        }
        if had_pairings {
            cx.notify();
        }
    }

    fn serve(&mut self, cx: &mut Context<Self>) {
        let (events, receiver) = async_channel::unbounded();
        thread::spawn(move || listen(events));
        self.serving = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                if this
                    .update(cx, |this, cx| this.on_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Opened(id, replies) => {
                self.connections.insert(
                    id,
                    Connection {
                        session: BrowserSession::new(),
                        replies,
                    },
                );
            }
            Event::Message(id, message) => self.handle(id, &message, cx),
            Event::Closed(id) => {
                self.connections.remove(&id);
                self.pairings.retain(|pending| pending.connection != id);
                cx.notify();
            }
            Event::Failed(error) => {
                self.serve_error = Some(error);
                cx.notify();
            }
        }
    }

    fn handle(&mut self, id: u64, message: &[u8], cx: &mut Context<Self>) {
        let Some(connection) = self.connections.get_mut(&id) else {
            return;
        };
        let workspace = self.workspace.read(cx);
        if !workspace.browser_integration() {
            let _ = connection.replies.send(not_running_reply(message));
            return;
        }
        match connection
            .session
            .handle(workspace, &String::from_utf8_lossy(message))
        {
            BrowserReply::Send(reply) => connection.send(reply),
            BrowserReply::AskPairing(request) => {
                self.pairings.push_back(PendingPairing {
                    id: self.next_pairing,
                    connection: id,
                    request,
                });
                self.next_pairing += 1;
                cx.notify();
            }
        }
    }
}

/// Accepts native hosts on the local socket, with a thread to read and one to write for each.
fn listen(events: async_channel::Sender<Event>) {
    let listener = match ipc::listen(&ipc::socket_path()) {
        Ok(listener) => listener,
        Err(error) => {
            let _ = events.send_blocking(Event::Failed(error));
            return;
        }
    };
    for (id, stream) in (0..).zip(listener.incoming().filter_map(Result::ok)) {
        let (mut from_host, mut to_host) = stream.split();
        let (replies, outgoing) = mpsc::channel::<Vec<u8>>();
        if events.send_blocking(Event::Opened(id, replies)).is_err() {
            return;
        }
        thread::spawn(move || {
            for reply in outgoing {
                if write_frame(&mut to_host, &reply).is_err() {
                    break;
                }
            }
        });
        let events = events.clone();
        thread::spawn(move || {
            while let Ok(Some(message)) = read_frame(&mut from_host) {
                if events.send_blocking(Event::Message(id, message)).is_err() {
                    return;
                }
            }
            let _ = events.send_blocking(Event::Closed(id));
        });
    }
}
