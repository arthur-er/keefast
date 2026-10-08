//! Browser integration in the app: bridges the native hosts' socket to the Workspace, which
//! answers them. The socket is served from when browser integration is first turned on.

use std::collections::HashMap;
use std::io;

use gpui::{Context, Entity, Task};
use keefast_native_host::ipc::{self, Replies, Server, ServerEvent};
use keefast_workspace::{
    ConnectionId, EditVaultError, Outgoing, PairingId, PendingPairing, Workspace,
};

use crate::saving;

pub struct BrowserIntegration {
    workspace: Entity<Workspace>,
    replies: HashMap<ConnectionId, Replies>,
    serving: Option<(Server, Task<()>)>,
    /// Why the app can't serve the native hosts, if it can't.
    serve_error: Option<io::Error>,
}

impl BrowserIntegration {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let mut integration = Self {
            workspace,
            replies: HashMap::new(),
            serving: None,
            serve_error: None,
        };
        if integration.workspace.read(cx).browser_integration() {
            integration.serve(cx);
        }
        integration
    }

    pub fn enabled(&self, cx: &gpui::App) -> bool {
        self.workspace.read(cx).browser_integration()
    }

    /// Turns browser integration on or off. Turned on, it serves the native hosts, trying again
    /// if it failed to before. Turned off, the socket stays served and the Workspace answers
    /// every host that keefast is not running, which makes the hosts end.
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let replies = self.workspace.update(cx, |workspace, cx| {
            cx.notify();
            workspace.set_browser_integration(enabled)
        });
        self.send(replies);
        if enabled && self.serving.is_none() {
            self.serve_error = None;
            self.serve(cx);
        }
        cx.notify();
    }

    pub fn serve_error(&self) -> Option<&io::Error> {
        self.serve_error.as_ref()
    }

    /// The Pairing request to put to the user now.
    pub fn pending_pairing(&self, cx: &gpui::App) -> Option<PendingPairing> {
        self.workspace.read(cx).pending_pairing()
    }

    /// Pairs the extension that asked, as `name`. A name the Vault can't take is refused,
    /// and the request stays pending.
    pub fn approve_pairing(
        &mut self,
        pairing: PairingId,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), EditVaultError> {
        let reply = self.workspace.update(cx, |workspace, cx| {
            cx.notify();
            workspace.approve_pairing(pairing, name)
        })?;
        self.send(reply);
        saving::autosave(&self.workspace, cx);
        cx.notify();
        Ok(())
    }

    pub fn deny_pairing(&mut self, pairing: PairingId, cx: &mut Context<Self>) {
        let reply = self
            .workspace
            .update(cx, |workspace, _| workspace.deny_pairing(pairing));
        self.send(reply);
        cx.notify();
    }

    fn serve(&mut self, cx: &mut Context<Self>) {
        let (events, receiver) = async_channel::unbounded();
        let server = Server::start(ipc::socket_path(), move |event| {
            events.send_blocking(event).is_ok()
        });
        let server = match server {
            Ok(server) => server,
            Err(error) => {
                self.serve_error = Some(error);
                return;
            }
        };
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                if this
                    .update(cx, |this, cx| this.on_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        self.serving = Some((server, task));
    }

    fn on_event(&mut self, event: ServerEvent, cx: &mut Context<Self>) {
        match event {
            ServerEvent::Opened(id, replies) => {
                self.replies.insert(ConnectionId(id), replies);
            }
            ServerEvent::Message(id, message) => {
                let connection = ConnectionId(id);
                let reply = self.workspace.update(cx, |workspace, _| {
                    workspace.handle_browser_request(connection, &String::from_utf8_lossy(&message))
                });
                self.send(reply.map(|message| Outgoing {
                    connection,
                    message,
                }));
            }
            ServerEvent::Closed(id) => {
                self.replies.remove(&ConnectionId(id));
                self.workspace.update(cx, |workspace, _| {
                    workspace.disconnect_browser(ConnectionId(id))
                });
            }
        }
        cx.notify();
    }

    fn send(&self, replies: impl IntoIterator<Item = Outgoing>) {
        for Outgoing {
            connection,
            message,
        } in replies
        {
            if let Some(host) = self.replies.get(&connection) {
                host.send(message);
            }
        }
    }
}
