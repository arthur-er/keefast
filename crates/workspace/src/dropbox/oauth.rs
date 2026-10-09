//! Connecting a Dropbox account: OAuth 2 authorization code flow with PKCE, which needs no client
//! secret, finished through a redirect to a port keefast listens on.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use url::Url;

use super::{DropboxConfig, DropboxError};

/// The path Dropbox redirects the browser to, on the loopback port.
const CALLBACK_PATH: &str = "/dropbox";
/// How often the loopback port is checked while waiting for the browser.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A Dropbox authorization the user is giving in their browser. Show them [`Self::url`], then
/// finish with [`super::Dropbox::finish_connect`], which waits for Dropbox to send the browser
/// back.
pub struct Authorization {
    url: String,
    pub(super) redirect_uri: String,
    pub(super) verifier: SecretString,
    state: String,
    listeners: Vec<TcpListener>,
    cancelled: Arc<AtomicBool>,
}

/// Stops a [`super::Dropbox::finish_connect`] that is waiting for the browser.
#[derive(Clone)]
pub struct ConnectCanceller(Arc<AtomicBool>);

impl ConnectCanceller {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Authorization {
    /// Listens on the loopback port and builds the Dropbox page that asks the user to let
    /// keefast in.
    pub(super) fn begin(config: &DropboxConfig) -> Result<Self, DropboxError> {
        let v4 = bind_loopback(config.redirect_port)?;
        let port = v4.local_addr().map_err(DropboxError::Callback)?.port();
        let mut listeners = vec![v4];
        // `localhost` may resolve to either address, so both are served when the OS allows.
        if let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            listeners.push(v6);
        }
        for listener in &listeners {
            listener
                .set_nonblocking(true)
                .map_err(DropboxError::Callback)?;
        }

        let redirect_uri = format!("http://localhost:{port}{CALLBACK_PATH}");
        let verifier = SecretString::from(random_token(32));
        let challenge = BASE64_URL.encode(Sha256::digest(verifier.expose_secret().as_bytes()));
        let state = random_token(16);
        let mut url = Url::parse(&config.authorize_url).map_err(|_| DropboxError::NotConfigured)?;
        url.query_pairs_mut()
            .append_pair("client_id", &config.app_key)
            .append_pair("response_type", "code")
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            // A refresh token, so keefast stays connected after the access token expires.
            .append_pair("token_access_type", "offline")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("state", &state);
        Ok(Self {
            url: url.into(),
            redirect_uri,
            verifier,
            state,
            listeners,
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The Dropbox page to open in the user's browser.
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn canceller(&self) -> ConnectCanceller {
        ConnectCanceller(Arc::clone(&self.cancelled))
    }

    /// Waits until Dropbox sends the browser back, and returns the authorization code it
    /// brings. Requests that don't carry this authorization's state are turned away, so another
    /// page can't feed keefast a code.
    pub(super) fn wait_for_code(&self) -> Result<SecretString, DropboxError> {
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(DropboxError::Cancelled);
            }
            let mut waiting = true;
            for listener in &self.listeners {
                match listener.accept() {
                    Ok((stream, _)) => {
                        waiting = false;
                        if let Some(answer) = self.answer(stream) {
                            return answer;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(DropboxError::Callback(error)),
                }
            }
            if waiting {
                thread::sleep(POLL_INTERVAL);
            }
        }
    }

    /// Answers one request to the loopback port: the code, Dropbox's refusal, or `None` for a
    /// request that isn't Dropbox's redirect for this authorization.
    fn answer(&self, mut stream: TcpStream) -> Option<Result<SecretString, DropboxError>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let Some(target) = read_request_target(&mut stream) else {
            respond(&mut stream, "400 Bad Request", PAGE_NOT_FOUND);
            return None;
        };
        let url = Url::parse("http://localhost")
            .and_then(|base| base.join(&target))
            .ok()?;
        let query = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        if url.path() != CALLBACK_PATH || query("state").as_deref() != Some(&self.state) {
            respond(&mut stream, "404 Not Found", PAGE_NOT_FOUND);
            return None;
        }
        if let Some(code) = query("code") {
            respond(&mut stream, "200 OK", PAGE_CONNECTED);
            return Some(Ok(SecretString::from(code)));
        }
        respond(&mut stream, "200 OK", PAGE_REFUSED);
        let reason = query("error_description")
            .or_else(|| query("error"))
            .unwrap_or_default();
        Some(Err(DropboxError::Refused(reason)))
    }
}

/// Listens on the loopback port. An authorization just cancelled may hold the port until it
/// notices, a poll interval later, so a port in use is tried again for a moment.
fn bind_loopback(port: u16) -> Result<TcpListener, DropboxError> {
    let mut attempts = 0;
    loop {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Err(error) if error.kind() == io::ErrorKind::AddrInUse && attempts < 20 => {
                attempts += 1;
                thread::sleep(POLL_INTERVAL);
            }
            result => return result.map_err(DropboxError::Callback),
        }
    }
}

/// A random URL-safe string made from `bytes` random bytes.
fn random_token(bytes: usize) -> String {
    let mut random = vec![0; bytes];
    getrandom::fill(&mut random).expect("the OS random number generator is available");
    BASE64_URL.encode(random)
}

/// The request target of an HTTP request: `/dropbox?code=…` in `GET /dropbox?code=… HTTP/1.1`.
fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") && request.len() < 16 * 1024 {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let line = request.split(|&byte| byte == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?.trim_end();
    let mut parts = line.split(' ');
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) if target.starts_with('/') => Some(target.to_owned()),
        _ => None,
    }
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>keefast</title></head>\
         <body style=\"font-family: sans-serif; margin: 4em auto; max-width: 32em\">\
         <h1>keefast</h1><p>{body}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

const PAGE_CONNECTED: &str = "Dropbox is connected. You can close this tab and go back to keefast.";
const PAGE_REFUSED: &str =
    "Dropbox was not connected. You can close this tab and go back to keefast.";
const PAGE_NOT_FOUND: &str = "Nothing here.";
