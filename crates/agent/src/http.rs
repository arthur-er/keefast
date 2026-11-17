//! `http_request`: an HTTP call keefast makes for an Agent with only MCP, with secrets put in by
//! `{{secret:<Secret reference>}}` placeholders (ADR 0012 "How a secret is used").
//!
//! This module has the request and response an Agent sees, the placeholders, and the
//! [`HttpTransport`] an [`AgentSession`](crate::AgentSession) is given to reach the network: a
//! fake one in tests, [`NetworkTransport`] everywhere else. The session decides where a
//! request may go; the transport resolves host names and sends one request at a time to the
//! addresses the session checked, never following a redirect itself, so every hop is checked
//! again and a host's DNS can't change its answer between the check and the connection.

use std::collections::BTreeMap;
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::AgentError;

/// The largest request body keefast sends, after placeholders are filled in.
pub const MAX_REQUEST_BODY: usize = 1 << 20;
/// The most of a response body an Agent gets back; the rest is cut off, and the response says so.
pub const MAX_RESPONSE_BODY: usize = 1 << 20;
/// The most headers a request may have.
pub const MAX_HEADERS: usize = 64;
/// The most redirects keefast follows for one request.
pub const MAX_REDIRECTS: usize = 5;
/// How long one hop may take, from connecting to the last byte of its response.
pub const HOP_TIMEOUT: Duration = Duration::from_secs(30);
/// How long connecting may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What an Agent asks keefast to send. Any of the URL's path and query, the header values, the
/// body and the basic auth may hold `{{secret:<Secret reference>}}` placeholders; the scheme and
/// host may not, so where a request goes is always written out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    /// Such as `GET` or `POST`.
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_auth: Option<BasicAuth>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

/// What came back, with every secret the request used redacted from its URL, headers and body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    /// The URL that answered, after redirects.
    pub url: String,
    /// Each header as `[name, value]`, in the order they came.
    pub headers: Vec<(String, String)>,
    /// The body as text, or in base64 when it isn't UTF-8 (see `body_encoding`).
    pub body: String,
    /// `utf-8` or `base64`.
    pub body_encoding: String,
    /// Whether the body was cut off at [`MAX_RESPONSE_BODY`] bytes.
    pub truncated: bool,
}

/// One request on its way: what a transport sends to one host. Its URL, headers and body may
/// hold secrets.
pub struct Hop<'a> {
    pub method: &'a str,
    pub url: &'a url::Url,
    /// Where to connect, already checked: the transport connects to none other.
    pub addresses: &'a [IpAddr],
    pub headers: &'a [(String, Zeroizing<String>)],
    pub body: Option<&'a [u8]>,
}

/// One hop's response, as the server sent it: its body is at most [`MAX_RESPONSE_BODY`] bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HopResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Whether there was more body than [`MAX_RESPONSE_BODY`].
    pub truncated: bool,
}

/// How an [`AgentSession`](crate::AgentSession) reaches the network for `http_request`.
pub trait HttpTransport: Send + Sync {
    /// The addresses `host`, a DNS name, has.
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String>;
    /// Sends `hop` to one of its addresses and returns the response, without following a
    /// redirect.
    fn send(&self, hop: &Hop<'_>) -> Result<HopResponse, String>;
}

/// The network, through the OS's resolver and `ureq`, with rustls and the Mozilla roots. It
/// ignores proxy settings: a proxy would resolve the host itself, after keefast checked it.
#[derive(Debug, Default, Clone, Copy)]
pub struct NetworkTransport;

impl HttpTransport for NetworkTransport {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
        let addresses = (host, port)
            .to_socket_addrs()
            .map_err(|error| format!("can't find {host}: {error}"))?;
        let addresses: Vec<IpAddr> = addresses.map(|address| address.ip()).collect();
        if addresses.is_empty() {
            return Err(format!("can't find {host}: it has no address"));
        }
        Ok(addresses)
    }

    fn send(&self, hop: &Hop<'_>) -> Result<HopResponse, String> {
        let port = hop
            .url
            .port_or_known_default()
            .ok_or("the URL has no port")?;
        let pinned = Pinned(
            hop.addresses
                .iter()
                .map(|address| SocketAddr::new(*address, port))
                .collect(),
        );
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(None)
            .save_redirect_history(false)
            .user_agent(concat!("keefast/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(HOP_TIMEOUT))
            .build();
        let agent = ureq::Agent::with_parts(
            config,
            ureq::unversioned::transport::DefaultConnector::default(),
            pinned,
        );
        let mut request = ureq::http::Request::builder()
            .method(hop.method)
            .uri(hop.url.as_str());
        for (name, value) in hop.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let result = match hop.body {
            Some(body) => agent.run(request.body(body).map_err(|error| error.to_string())?),
            None => agent.run(request.body(()).map_err(|error| error.to_string())?),
        };
        let mut response = result.map_err(|error| error.to_string())?;
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let mut body = Vec::new();
        response
            .body_mut()
            .with_config()
            .limit(u64::MAX)
            .reader()
            .take(MAX_RESPONSE_BODY as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|error| format!("the response couldn't be read: {error}"))?;
        let truncated = body.len() > MAX_RESPONSE_BODY;
        body.truncate(MAX_RESPONSE_BODY);
        Ok(HopResponse {
            status: response.status().as_u16(),
            headers,
            body,
            truncated,
        })
    }
}

/// A resolver that answers with the addresses keefast checked, whatever DNS says now.
#[derive(Debug)]
struct Pinned(Vec<SocketAddr>);

impl ureq::unversioned::resolver::Resolver for Pinned {
    fn resolve(
        &self,
        _uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let mut addresses = self.empty();
        // ureq keeps at most 16.
        for address in self.0.iter().take(16) {
            addresses.push(*address);
        }
        if addresses.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(addresses)
    }
}

const OPEN: &str = "{{secret:";
const CLOSE: &str = "}}";

/// The Secret references `text`'s placeholders name, in order.
pub(crate) fn placeholders(text: &str) -> Result<Vec<String>, AgentError> {
    let mut references = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let end = after.find(CLOSE).ok_or_else(|| {
            AgentError::BadHttpRequest(format!("a {OPEN} placeholder isn't closed with {CLOSE}"))
        })?;
        let reference = after[..end].trim();
        if reference.is_empty() {
            return Err(AgentError::BadHttpRequest(format!(
                "a {OPEN}{CLOSE} placeholder names no Secret reference"
            )));
        }
        references.push(reference.to_owned());
        rest = &after[end + CLOSE.len()..];
    }
    Ok(references)
}

/// `text` with each placeholder replaced by the value of the reference it names, which
/// `value` gives, percent-encoded if `encode`, as in a URL.
pub(crate) fn expand(
    text: &str,
    value: &dyn Fn(&str) -> Option<Zeroizing<String>>,
    encode: bool,
) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(text.len()));
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else {
            break;
        };
        if let Some(value) = value(after[..end].trim()) {
            if encode {
                out.push_str(&percent_encode(&value));
            } else {
                out.push_str(&value);
            }
        }
        rest = &after[end + CLOSE.len()..];
    }
    out.push_str(rest);
    out
}

/// `value` with every byte but URL-unreserved ones (`A-Z a-z 0-9 - . _ ~`) percent-encoded, so
/// a secret put in a URL stays one path segment or query value.
pub(crate) fn percent_encode(value: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(value.len()));
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The `Authorization` header value for basic auth.
pub(crate) fn basic_auth(username: &str, password: &str) -> Zeroizing<String> {
    let credentials = Zeroizing::new(format!("{username}:{password}"));
    Zeroizing::new(format!("Basic {}", STANDARD.encode(credentials.as_bytes())))
}

/// Headers keefast sets itself, or that would change where or how a request goes: a `Host`
/// header could send it to another site behind the same address.
pub(crate) fn is_forbidden_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "upgrade"
            | "te"
            | "trailer"
            | "expect"
    ) || name.starts_with("proxy-")
}

/// Whether `name` is an HTTP token, as header names and methods are.
pub(crate) fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// The body as an Agent gets it: text if it is UTF-8, otherwise base64.
pub(crate) fn body_text(body: Vec<u8>) -> (String, &'static str) {
    match String::from_utf8(body) {
        Ok(text) => (text, "utf-8"),
        Err(error) => (STANDARD.encode(error.as_bytes()), "base64"),
    }
}

/// An [`HttpRequest`] checked before any secret is read: its method upper case, and the Secret
/// references its placeholders name, each once, in order.
pub(crate) struct Template {
    pub method: String,
    pub request: HttpRequest,
    pub references: Vec<String>,
}

impl Template {
    pub fn check(request: HttpRequest) -> Result<Self, AgentError> {
        let bad = |message: &str| Err(AgentError::BadHttpRequest(message.to_owned()));
        let method = request.method.trim().to_ascii_uppercase();
        if !is_token(&method) {
            return bad("the method isn't one: use GET, POST, PUT, PATCH, DELETE or another");
        }
        if method == "CONNECT" {
            return bad("keefast doesn't send CONNECT requests");
        }
        let Some((scheme, rest)) = request.url.split_once("://") else {
            return bad("the URL must be whole, starting with https://");
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if scheme.contains("{{") || authority.contains("{{") {
            return bad(
                "a secret can't be the URL's host: write the host out, and put secrets in the \
                 path, query, headers, body or basic auth",
            );
        }
        if request.headers.len() > MAX_HEADERS {
            return Err(AgentError::BadHttpRequest(format!(
                "a request may have at most {MAX_HEADERS} headers"
            )));
        }
        for name in request.headers.keys() {
            if !is_token(name) {
                return Err(AgentError::BadHttpRequest(format!(
                    "{name:?} isn't a header name"
                )));
            }
            if is_forbidden_header(name) {
                return Err(AgentError::BadHttpRequest(format!(
                    "keefast sets the {name} header itself"
                )));
            }
        }
        if request.basic_auth.is_some()
            && request
                .headers
                .keys()
                .any(|name| name.eq_ignore_ascii_case("authorization"))
        {
            return bad("give basic auth or an Authorization header, not both");
        }
        if request
            .body
            .as_ref()
            .is_some_and(|body| body.len() > MAX_REQUEST_BODY)
        {
            return Err(AgentError::BadHttpRequest(format!(
                "the body may be at most {MAX_REQUEST_BODY} bytes"
            )));
        }
        let mut references: Vec<String> = Vec::new();
        let texts = std::iter::once(request.url.as_str())
            .chain(request.headers.values().map(String::as_str))
            .chain(request.body.as_deref())
            .chain(
                request
                    .basic_auth
                    .iter()
                    .flat_map(|auth| [auth.username.as_str(), auth.password.as_str()]),
            );
        for text in texts {
            for reference in placeholders(text)? {
                if !references.contains(&reference) {
                    references.push(reference);
                }
            }
        }
        if references.is_empty() {
            return Err(AgentError::NoSecret);
        }
        Ok(Self {
            method,
            request,
            references,
        })
    }
}

/// Whether `status` is a redirect keefast follows.
pub(crate) fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}
