//! Relaying between the browser and the app.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Map, Value};

use crate::ipc::{read_frame, write_frame};

/// The KeePassXC-Browser error code for "timeout or cannot connect".
pub const NOT_RUNNING_CODE: &str = "5";
pub const NOT_RUNNING_MESSAGE: &str = "keefast is not running, or its browser integration is off";

/// A connection to the app: what it says, and where to send what the browser says.
struct App<W> {
    to_app: W,
    open: Arc<AtomicBool>,
}

/// Relays messages from the browser to the app, and the app's messages back, until the browser
/// closes `browser_in`. The app is connected to when the first message arrives. If it can't be
/// reached, or has gone, the request is answered with the protocol's "not running" error and
/// the relay ends: the browser sees the host disconnect, and the extension starts a new host
/// to reconnect, with a new key exchange, rather than keep asking an app that isn't there.
pub fn run<R, W>(
    mut browser_in: impl Read,
    browser_out: impl Write + Send + 'static,
    connect: impl FnOnce() -> io::Result<(R, W)>,
) -> io::Result<()>
where
    R: Read + Send + 'static,
    W: Write,
{
    let browser_out = Arc::new(Mutex::new(browser_out));
    let mut connect = Some(connect);
    let mut app: Option<App<W>> = None;
    while let Some(message) = read_frame(&mut browser_in)? {
        if let Some(connect) = connect.take() {
            app = connect().ok().map(|(from_app, to_app)| App {
                to_app,
                open: relay_replies(from_app, Arc::clone(&browser_out)),
            });
        }
        let sent = app.as_mut().is_some_and(|app| {
            app.open.load(Ordering::Acquire) && write_frame(&mut app.to_app, &message).is_ok()
        });
        if !sent {
            let reply = not_running_reply(&message);
            return write_frame(&mut *browser_out.lock().unwrap(), &reply);
        }
    }
    Ok(())
}

/// Copies the app's messages to the browser on a thread of its own. The flag turns false when
/// the app goes away.
fn relay_replies(
    mut from_app: impl Read + Send + 'static,
    browser_out: Arc<Mutex<impl Write + Send + 'static>>,
) -> Arc<AtomicBool> {
    let open = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&open);
    thread::spawn(move || {
        while let Ok(Some(message)) = read_frame(&mut from_app) {
            if write_frame(&mut *browser_out.lock().unwrap(), &message).is_err() {
                break;
            }
        }
        flag.store(false, Ordering::Release);
    });
    open
}

/// The protocol's error reply to `request`, for when the app can't be reached.
pub fn not_running_reply(request: &[u8]) -> Vec<u8> {
    let request: Map<String, Value> = serde_json::from_slice(request).unwrap_or_default();
    let mut reply = Map::new();
    reply.insert(
        "action".into(),
        request.get("action").cloned().unwrap_or_default(),
    );
    reply.insert("errorCode".into(), NOT_RUNNING_CODE.into());
    reply.insert("error".into(), NOT_RUNNING_MESSAGE.into());
    if let Some(id) = request.get("requestID") {
        reply.insert("requestID".into(), id.clone());
    }
    serde_json::to_vec(&reply).expect("JSON values serialise")
}
