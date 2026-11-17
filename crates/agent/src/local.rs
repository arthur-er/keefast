//! The in-app host's link (ADR 0012 "Where it runs", update "In-app host"): how
//! `keefast-agent mcp`, `run` and `store` reach the desktop app on the same machine.
//!
//! The app listens at an endpoint of its own beside the browser's: `agent.sock` in the user's
//! runtime directory, or the named pipe `keefast-agent-<user>` on Windows. It is made, and
//! checked, exactly as the browser's is (ADR 0004 update 2026-10-28): only the user can reach
//! it, and `keefast-agent` talks only to an app running as its own user. It is a separate
//! endpoint so that no browser extension can reach Agent requests through the native host,
//! and so that it is served whether or not browser integration is on.
//!
//! Each message is framed as the native host's are (a native-endian `u32` length, then the
//! bytes), and holds JSON: a [`Call`] one way, with the Agent pairing's token and an
//! [`AgentRequest`], and an [`Answer`] the other, with the [`AgentReply`] or [`AgentError`].
//! Calls carry an id, so one connection can wait on several prompts at once. A closed app, or
//! one that goes away mid-call, answers as a locked one does: "keefast is locked".

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use keefast_native_host::ipc::{self, Replies, Server, ServerEvent};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{AgentError, AgentReply, AgentRequest, ServesAgents};

/// The version of the messages below; the app refuses calls of another.
pub const PROTOCOL: u32 = 1;
/// The largest message either way: an `http_request`'s body and response fit, with room over.
pub const MAX_MESSAGE: usize = 8 << 20;
/// Names another endpoint than the user's own, for tests and for running a second keefast.
pub const ENDPOINT_ENV: &str = "KEEFAST_AGENT_SOCKET";

/// Where the desktop app listens for `keefast-agent`: [`ENDPOINT_ENV`] if it is set, else the
/// user's private endpoint.
pub fn endpoint() -> io::Result<PathBuf> {
    match std::env::var_os(ENDPOINT_ENV).filter(|path| !path.is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => ipc::endpoint_path(ipc::AGENT),
    }
}

/// What `keefast-agent` sends the app.
#[derive(Serialize, Deserialize)]
struct Call {
    version: u32,
    id: u64,
    token: Zeroizing<String>,
    request: AgentRequest,
}

/// The part of a [`Call`] every version keeps, read first so a call of another version gets
/// an answer it can match.
#[derive(Deserialize)]
struct CallHeader {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    id: u64,
}

/// What the app answers.
#[derive(Serialize, Deserialize)]
struct Answer {
    id: u64,
    answer: Result<AgentReply, AgentError>,
}

/// The desktop app, as `keefast-agent` reaches it.
#[derive(Debug, Clone)]
pub struct DesktopApp {
    endpoint: PathBuf,
}

impl DesktopApp {
    pub fn at(endpoint: PathBuf) -> Self {
        Self { endpoint }
    }

    /// The app at [`endpoint`].
    pub fn from_env() -> io::Result<Self> {
        endpoint().map(Self::at)
    }

    /// Asks the app to handle `request` for the Agent pairing whose token is `token`. It
    /// connects afresh for each call, so an app started or unlocked since the last one is
    /// found. It waits while the app prompts its user.
    pub fn call(&self, token: &str, request: AgentRequest) -> Result<AgentReply, AgentError> {
        // An app that isn't running, or runs as another user, is as good as locked: the Agent
        // can't do anything about either.
        let mut stream = ipc::connect(&self.endpoint).map_err(|_| AgentError::Locked)?;
        let call = Call {
            version: PROTOCOL,
            id: 1,
            token: Zeroizing::new(token.to_owned()),
            request,
        };
        let message = Zeroizing::new(serde_json::to_vec(&call).expect("a call is plain JSON"));
        ipc::write_frame_up_to(&mut stream, &message, MAX_MESSAGE)
            .map_err(|_| AgentError::Locked)?;
        let reply = match ipc::read_frame_up_to(&mut stream, MAX_MESSAGE) {
            Ok(Some(reply)) => Zeroizing::new(reply),
            // The app quit, or locked and dropped the connection, while it was asked.
            Ok(None) | Err(_) => return Err(AgentError::Locked),
        };
        serde_json::from_slice::<Answer>(&reply)
            .map_err(|error| AgentError::Mismatch(format!("its answer can't be read: {error}")))?
            .answer
    }
}

impl ServesAgents for DesktopApp {
    fn handle_agent(&self, token: &str, request: AgentRequest) -> Result<AgentReply, AgentError> {
        self.call(token, request)
    }
}

/// Serves `agents` to `keefast-agent` at `endpoint`, until the returned server is dropped.
/// Each call is handled on a thread of its own, since it may wait up to a minute on a prompt.
/// Fails while another keefast listens there, or where other users could reach it.
pub fn serve(endpoint: PathBuf, agents: Arc<dyn ServesAgents>) -> io::Result<Server> {
    let connections: Mutex<HashMap<u64, Arc<Replies>>> = Mutex::default();
    Server::start_up_to(endpoint, MAX_MESSAGE, move |event| {
        let mut open = connections.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            ServerEvent::Opened(connection, replies) => {
                open.insert(connection, Arc::new(replies));
            }
            ServerEvent::Message(connection, message) => {
                let message = Zeroizing::new(message);
                if let Some(replies) = open.get(&connection).cloned() {
                    let agents = Arc::clone(&agents);
                    thread::spawn(move || replies.send(answer(&*agents, &message).to_vec()));
                }
            }
            ServerEvent::Closed(connection) => {
                open.remove(&connection);
            }
        }
        true
    })
}

/// The answer to one message from `keefast-agent`.
fn answer(agents: &dyn ServesAgents, message: &[u8]) -> Zeroizing<Vec<u8>> {
    let (id, answer) = match serde_json::from_slice::<CallHeader>(message) {
        Err(error) => (
            0,
            Err(AgentError::Mismatch(format!(
                "its call can't be read: {error}"
            ))),
        ),
        Ok(header) if header.version != PROTOCOL => (
            header.id,
            Err(AgentError::Mismatch(format!(
                "it speaks version {}, the app {PROTOCOL}",
                header.version
            ))),
        ),
        Ok(header) => match serde_json::from_slice::<Call>(message) {
            Ok(call) => (call.id, agents.handle_agent(&call.token, call.request)),
            Err(error) => (
                header.id,
                Err(AgentError::Mismatch(format!(
                    "the app doesn't know its request: {error}"
                ))),
            ),
        },
    };
    let encoded = Zeroizing::new(
        serde_json::to_vec(&Answer { id, answer }).expect("an answer is plain JSON"),
    );
    if encoded.len() <= MAX_MESSAGE {
        return encoded;
    }
    Zeroizing::new(
        serde_json::to_vec(&Answer {
            id,
            answer: Err(AgentError::HttpFailed(
                "the answer is too large to pass to keefast-agent".to_owned(),
            )),
        })
        .expect("an answer is plain JSON"),
    )
}
