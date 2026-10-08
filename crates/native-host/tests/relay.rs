//! The native host's stdio framing and relaying, with the browser and the app played by
//! socket pairs.

#![cfg(unix)]

use std::io::{self, Cursor};
use std::os::unix::net::UnixStream;
use std::thread::{self, JoinHandle};

use keefast_native_host::ipc::{MAX_MESSAGE, read_frame, write_frame};
use keefast_native_host::relay;
use serde_json::{Value, json};

/// The browser's end of a running host, which can't reach any app.
fn host_without_app() -> (UnixStream, JoinHandle<io::Result<()>>) {
    host(|| Err(io::ErrorKind::ConnectionRefused.into()))
}

fn host(
    connect: impl FnOnce() -> io::Result<(UnixStream, UnixStream)> + Send + 'static,
) -> (UnixStream, JoinHandle<io::Result<()>>) {
    let (browser, host_end) = UnixStream::pair().unwrap();
    let out = host_end.try_clone().unwrap();
    let handle = thread::spawn(move || relay::run(host_end, out, connect));
    (browser, handle)
}

fn send(stream: &mut UnixStream, message: &Value) {
    write_frame(stream, message.to_string().as_bytes()).unwrap();
}

fn receive(stream: &mut UnixStream) -> Value {
    let frame = read_frame(stream).unwrap().expect("a message");
    serde_json::from_slice(&frame).unwrap()
}

#[test]
fn frames_are_a_native_endian_length_then_the_message() {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, b"{\"a\":1}").unwrap();

    assert_eq!(&bytes[..4], &7u32.to_ne_bytes());
    assert_eq!(&bytes[4..], b"{\"a\":1}");
    let mut reader = Cursor::new(bytes);
    assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b"{\"a\":1}");
    assert_eq!(read_frame(&mut reader).unwrap(), None);
}

#[test]
fn oversized_frames_are_refused() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(MAX_MESSAGE as u32 + 1).to_ne_bytes());
    assert!(read_frame(&mut Cursor::new(bytes)).is_err());
    assert!(write_frame(&mut Vec::new(), &vec![b' '; MAX_MESSAGE + 1]).is_err());
}

#[test]
fn without_the_app_the_request_gets_the_not_running_error_and_the_host_ends() {
    let (mut browser, host) = host_without_app();

    send(
        &mut browser,
        &json!({"action": "change-public-keys", "publicKey": "a2V5", "nonce": "bm9uY2U=", "requestID": "7"}),
    );
    let reply = receive(&mut browser);

    assert_eq!(reply["action"], "change-public-keys");
    assert_eq!(reply["errorCode"], "5");
    assert_eq!(reply["requestID"], "7");
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("keefast is not running")
    );
    // The host ends, so the browser sees it disconnect and the extension starts a new one.
    host.join().unwrap().unwrap();
    assert_eq!(read_frame(&mut browser).unwrap(), None);
}

#[test]
fn messages_are_relayed_both_ways_unchanged() {
    let (app, app_end) = UnixStream::pair().unwrap();
    let (mut browser, host) = host(move || Ok((app_end.try_clone()?, app_end)));
    // An app that answers every message with its action reversed. It runs until the host
    // process would exit, which closes the connection.
    thread::spawn(move || {
        let mut app = app;
        while let Some(frame) = read_frame(&mut app).unwrap() {
            let request: Value = serde_json::from_slice(&frame).unwrap();
            let reversed: String = request["action"].as_str().unwrap().chars().rev().collect();
            write_frame(
                &mut app,
                json!({ "action": reversed }).to_string().as_bytes(),
            )
            .unwrap();
        }
    });

    send(&mut browser, &json!({"action": "associate"}));
    assert_eq!(receive(&mut browser)["action"], "etaicossa");
    send(&mut browser, &json!({"action": "test-associate"}));
    assert_eq!(receive(&mut browser)["action"], "etaicossa-tset");

    drop(browser);
    host.join().unwrap().unwrap();
}

#[test]
fn when_the_app_goes_away_requests_get_the_not_running_error() {
    let (app, app_end) = UnixStream::pair().unwrap();
    let (mut browser, host) = host(move || Ok((app_end.try_clone()?, app_end)));
    let app = thread::spawn(move || {
        let mut app = app;
        let frame = read_frame(&mut app).unwrap().unwrap();
        write_frame(&mut app, &frame).unwrap();
        // The app quits.
    });

    send(&mut browser, &json!({"action": "get-databasehash"}));
    assert_eq!(receive(&mut browser)["action"], "get-databasehash");
    app.join().unwrap();
    // The host notices the app has gone once it tries to reach it, and ends.
    let reply = loop {
        send(&mut browser, &json!({"action": "test-associate"}));
        let reply = receive(&mut browser);
        if reply["errorCode"] == "5" {
            break reply;
        }
    };

    assert_eq!(reply["action"], "test-associate");
    host.join().unwrap().unwrap();
}

mod server {
    use std::sync::mpsc;
    use std::time::Duration;

    use interprocess::local_socket::traits::Stream as _;
    use keefast_native_host::ipc::{self, Server, ServerEvent};

    use super::*;

    #[test]
    fn the_app_hears_hosts_and_answers_them_until_it_stops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keefast").join("browser.sock");
        let (events, heard) = mpsc::channel();
        let server = Server::start(path.clone(), move |event| events.send(event).is_ok()).unwrap();

        let (mut from_app, mut to_app) = ipc::connect(&path).unwrap().split();
        write_frame(&mut to_app, b"{\"action\":\"get-databasehash\"}").unwrap();
        let wait = Duration::from_secs(5);
        let ServerEvent::Opened(id, replies) = heard.recv_timeout(wait).unwrap() else {
            panic!("expected the host to be opened");
        };
        let Ok(ServerEvent::Message(from, message)) = heard.recv_timeout(wait) else {
            panic!("expected a message");
        };
        assert_eq!(from, id);
        assert_eq!(message, b"{\"action\":\"get-databasehash\"}");
        replies.send("{\"hash\":\"h\"}");
        assert_eq!(
            read_frame(&mut from_app).unwrap().unwrap(),
            b"{\"hash\":\"h\"}"
        );

        drop(server);
        assert!(!path.exists(), "the socket is removed");
        assert!(ipc::connect(&path).is_err());
        assert!(
            Server::start(path.clone(), |_| true).is_ok(),
            "it can listen there again"
        );
    }

    #[test]
    fn a_second_app_cannot_take_over_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser.sock");
        let _first = Server::start(path.clone(), |_| true).unwrap();

        let second = Server::start(path, |_| true);

        assert_eq!(second.err().unwrap().kind(), io::ErrorKind::AddrInUse);
    }
}
