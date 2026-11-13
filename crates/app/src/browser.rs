//! Browser integration in the app: bridges the native hosts' socket to the Workspace, which
//! answers them. The socket is served from when browser integration is first turned on. Turned
//! off, the socket stays served and the Workspace answers every host that keefast is not running,
//! which makes the hosts end.
//!
//! Requests that need the user (Pairing, Access, captured logins) wait in the Workspace; the UI
//! sees them in the workspace snapshot and answers them with commands.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use keefast_native_host::ipc::{self, Replies, Server, ServerEvent};
use keefast_workspace::{ConnectionId, Outgoing};

use crate::core::AppCore;

#[derive(Default)]
pub struct BrowserBridge {
    replies: HashMap<ConnectionId, Replies>,
    server: Option<Server>,
    /// Why the app can't serve the native hosts, if it can't: another keefast serves them, or
    /// there is nowhere private to serve them from.
    serve_error: Option<io::Error>,
}

impl BrowserBridge {
    /// Sends replies to the hosts they are for, if those are still connected.
    pub fn send(&self, replies: impl IntoIterator<Item = Outgoing>) {
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

    pub fn serve_error(&self) -> Option<&io::Error> {
        self.serve_error.as_ref()
    }
}

/// Serves the native hosts, if not already, trying again if it failed to before.
pub fn serve(core: &Arc<AppCore>) {
    let mut bridge = core
        .browser
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if bridge.server.is_some() {
        return;
    }
    bridge.serve_error = None;
    let (events, received) = mpsc::channel();
    let server = ipc::socket_path()
        .and_then(|path| Server::start(path, move |event| events.send(event).is_ok()));
    match server {
        Ok(server) => bridge.server = Some(server),
        Err(error) => {
            eprintln!("keefast: can't serve the browser: {error}");
            bridge.serve_error = Some(error);
            return;
        }
    }
    drop(bridge);
    let core = Arc::clone(core);
    thread::spawn(move || {
        while let Ok(event) = received.recv() {
            on_event(&core, event);
        }
    });
}

fn on_event(core: &Arc<AppCore>, event: ServerEvent) {
    match event {
        ServerEvent::Opened(id, replies) => {
            let mut bridge = core
                .browser
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            bridge.replies.insert(ConnectionId(id), replies);
        }
        ServerEvent::Message(id, message) => {
            let connection = ConnectionId(id);
            let reply = core.update(|workspace| {
                workspace.handle_browser_request(connection, &String::from_utf8_lossy(&message))
            });
            let bridge = core
                .browser
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            bridge.send(reply.map(|message| Outgoing {
                connection,
                message,
            }));
        }
        ServerEvent::Closed(id) => {
            core.browser
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .replies
                .remove(&ConnectionId(id));
            core.update(|workspace| workspace.disconnect_browser(ConnectionId(id)));
        }
    }
}

/// Turns browser integration on or off, answering the hosts it settles. Turned on, it serves the
/// native hosts.
pub fn set_enabled(core: &Arc<AppCore>, enabled: bool) {
    let replies = core.update(|workspace| workspace.set_browser_integration(enabled));
    core.browser
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .send(replies);
    if enabled {
        serve(core);
    }
}

/// Sends replies to the hosts they are for.
pub fn send(core: &AppCore, replies: impl IntoIterator<Item = Outgoing>) {
    core.browser
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .send(replies);
}
