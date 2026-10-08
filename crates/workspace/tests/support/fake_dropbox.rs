//! A fake Dropbox: its OAuth endpoints (PKCE, no client secret) and the file endpoints keefast
//! uses, served over HTTP on a local port, with Dropbox's revisions and errors. Its authorize
//! page lets keefast in straight away, as if the user had clicked "Allow".

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Request, Response, Server};
use url::Url;

pub const APP_KEY: &str = "fake-app-key";
pub const ACCOUNT_NAME: &str = "Robin Example";
pub const ACCOUNT_EMAIL: &str = "robin@example.com";

pub struct FakeDropbox {
    server: Arc<Server>,
    url: String,
    state: Arc<Mutex<FakeState>>,
    thread: Option<JoinHandle<()>>,
}

struct FakeFile {
    path: String,
    bytes: Vec<u8>,
    rev: String,
}

struct Grant {
    challenge: String,
    redirect_uri: String,
}

#[derive(Default)]
struct FakeState {
    /// Files and folders by lowercased path, as Dropbox paths are case-insensitive.
    files: BTreeMap<String, FakeFile>,
    folders: BTreeMap<String, String>,
    codes: HashMap<String, Grant>,
    /// Live access tokens, with the refresh token each was issued for.
    access_tokens: HashMap<String, String>,
    refresh_tokens: Vec<String>,
    next: u64,
    deny: bool,
    page_size: usize,
    refreshes: usize,
    revocations: usize,
    saw_client_secret: bool,
    /// The `Dropbox-API-Arg` headers received, to check they are ASCII.
    api_args: Vec<String>,
    downloads: usize,
}

impl FakeState {
    fn next(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }
}

impl FakeDropbox {
    pub fn start() -> Self {
        let server = Arc::new(Server::http("127.0.0.1:0").expect("the fake Dropbox starts"));
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let state = Arc::new(Mutex::new(FakeState {
            page_size: 100,
            ..FakeState::default()
        }));
        let thread = {
            let server = Arc::clone(&server);
            let state = Arc::clone(&state);
            thread::spawn(move || {
                for request in server.incoming_requests() {
                    handle(&state, request);
                }
            })
        };
        Self {
            server,
            url,
            state,
            thread: Some(thread),
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap()
    }

    /// Puts a file in the Dropbox, and the folders above it, replacing any file there.
    pub fn add_file(&self, path: &str, bytes: &[u8]) {
        let mut state = self.lock();
        let mut folder = String::new();
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        for part in &parts[..parts.len() - 1] {
            folder = format!("{folder}/{part}");
            state.folders.insert(folder.to_lowercase(), folder.clone());
        }
        let rev = state.next("rev");
        state.files.insert(
            path.to_lowercase(),
            FakeFile {
                path: path.to_owned(),
                bytes: bytes.to_vec(),
                rev,
            },
        );
    }

    pub fn add_folder(&self, path: &str) {
        self.lock()
            .folders
            .insert(path.to_lowercase(), path.to_owned());
    }

    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.lock()
            .files
            .get(&path.to_lowercase())
            .map(|file| file.bytes.clone())
    }

    /// Lists folders this many items per page, so keefast has to ask for the rest.
    pub fn page_size(&self, size: usize) {
        self.lock().page_size = size;
    }

    /// The user clicks "Cancel" instead of "Allow" on the next authorize page.
    pub fn deny_authorization(&self) {
        self.lock().deny = true;
    }

    /// Every access token stops working, as when they expire.
    pub fn expire_access_tokens(&self) {
        self.lock().access_tokens.clear();
    }

    /// How many times an access token was renewed with a refresh token.
    pub fn refreshes(&self) -> usize {
        self.lock().refreshes
    }

    pub fn revocations(&self) -> usize {
        self.lock().revocations
    }

    /// Whether any request carried a client secret, which a PKCE client never has.
    pub fn saw_client_secret(&self) -> bool {
        self.lock().saw_client_secret
    }

    /// How many times a file was downloaded.
    pub fn downloads(&self) -> usize {
        self.lock().downloads
    }

    pub fn api_args(&self) -> Vec<String> {
        self.lock().api_args.clone()
    }

    /// Opens `url` the way the user's browser would, following Dropbox's redirect back to
    /// keefast, and returns the page keefast shows at the end. It runs on its own thread, as
    /// keefast must be waiting for the browser meanwhile.
    pub fn visit_in_browser(url: &str) -> JoinHandle<String> {
        let url = url.to_owned();
        thread::spawn(move || {
            let mut response = ureq::get(&url).call().expect("the browser loads the page");
            response.body_mut().read_to_string().unwrap()
        })
    }
}

impl Drop for FakeDropbox {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn handle(state: &Mutex<FakeState>, mut request: Request) {
    let url = Url::parse(&format!("http://fake{}", request.url())).unwrap();
    let mut body = Vec::new();
    let _ = request.as_reader().read_to_end(&mut body);
    let header = |name: &str| {
        request
            .headers()
            .iter()
            .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str().to_owned())
    };
    let authorization = header("Authorization");
    let api_arg = header("Dropbox-API-Arg");
    let mut state = state.lock().unwrap();
    let response = match url.path() {
        "/oauth2/authorize" => authorize(&mut state, &url),
        "/oauth2/token" => token(&mut state, &body),
        path => {
            let token = authorization
                .as_deref()
                .and_then(|value| value.strip_prefix("Bearer "));
            if !token.is_some_and(|token| state.access_tokens.contains_key(token)) {
                error(
                    401,
                    json!({ "error_summary": "expired_access_token/", "error": { ".tag": "expired_access_token" } }),
                )
            } else {
                let token = token.unwrap().to_owned();
                if let Some(arg) = &api_arg {
                    state.api_args.push(arg.clone());
                }
                let arg = api_arg
                    .as_deref()
                    .map(|arg| serde_json::from_str(arg).expect("Dropbox-API-Arg is JSON"));
                let args = serde_json::from_slice(&body).unwrap_or(Value::Null);
                match path {
                    "/2/users/get_current_account" => ok_json(json!({
                        "account_id": "dbid:fake",
                        "name": { "display_name": ACCOUNT_NAME },
                        "email": ACCOUNT_EMAIL,
                    })),
                    "/2/auth/token/revoke" => {
                        state.revocations += 1;
                        let refresh = state.access_tokens.remove(&token).unwrap();
                        state.refresh_tokens.retain(|live| *live != refresh);
                        state
                            .access_tokens
                            .retain(|_, issued_for| *issued_for != refresh);
                        ok_json(Value::Null)
                    }
                    "/2/files/list_folder" => {
                        list_folder(&state, args["path"].as_str().unwrap(), 0)
                    }
                    "/2/files/list_folder/continue" => {
                        let cursor = args["cursor"].as_str().unwrap();
                        let (path, offset) = cursor.rsplit_once('|').unwrap();
                        list_folder(&state, path, offset.parse().unwrap())
                    }
                    "/2/files/download" => {
                        state.downloads += 1;
                        download(&state, &arg.unwrap())
                    }
                    "/2/files/get_metadata" => metadata(&state, &args),
                    "/2/files/upload" => upload(&mut state, &arg.unwrap(), body),
                    _ => Response::from_string("not found")
                        .with_status_code(404)
                        .boxed(),
                }
            }
        }
    };
    let _ = request.respond(response);
}

type Reply = tiny_http::ResponseBox;

fn ok_json(value: Value) -> Reply {
    Response::from_string(value.to_string())
        .with_header(header("Content-Type", "application/json"))
        .boxed()
}

fn error(status: u16, value: Value) -> Reply {
    Response::from_string(value.to_string())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json"))
        .boxed()
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn authorize(state: &mut FakeState, url: &Url) -> Reply {
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    state.saw_client_secret |= query.contains_key("client_secret");
    assert_eq!(query["client_id"], APP_KEY);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["token_access_type"], "offline");
    let redirect_uri = &query["redirect_uri"];
    let mut back = Url::parse(redirect_uri).unwrap();
    if std::mem::take(&mut state.deny) {
        back.query_pairs_mut()
            .append_pair("error", "access_denied")
            .append_pair(
                "error_description",
                "The user chose not to give your app access.",
            )
            .append_pair("state", &query["state"]);
    } else {
        let code = state.next("code");
        state.codes.insert(
            code.clone(),
            Grant {
                challenge: query["code_challenge"].clone(),
                redirect_uri: redirect_uri.clone(),
            },
        );
        back.query_pairs_mut()
            .append_pair("code", &code)
            .append_pair("state", &query["state"]);
    }
    Response::empty(302)
        .with_header(header("Location", back.as_str()))
        .boxed()
}

fn token(state: &mut FakeState, body: &[u8]) -> Reply {
    let form: HashMap<String, String> = url::form_urlencoded::parse(body).into_owned().collect();
    state.saw_client_secret |= form.contains_key("client_secret");
    let invalid_grant = || error(400, json!({ "error": "invalid_grant" }));
    if form.get("client_id").map(String::as_str) != Some(APP_KEY) {
        return error(400, json!({ "error": "invalid_client" }));
    }
    match form["grant_type"].as_str() {
        "authorization_code" => {
            let Some(grant) = state.codes.remove(&form["code"]) else {
                return invalid_grant();
            };
            let challenge = BASE64_URL.encode(Sha256::digest(form["code_verifier"].as_bytes()));
            if challenge != grant.challenge || form["redirect_uri"] != grant.redirect_uri {
                return invalid_grant();
            }
            let refresh = state.next("refresh-");
            let access = state.next("access-");
            state.refresh_tokens.push(refresh.clone());
            state.access_tokens.insert(access.clone(), refresh.clone());
            ok_json(json!({
                "access_token": access,
                "token_type": "bearer",
                "expires_in": 14400,
                "refresh_token": refresh,
                "account_id": "dbid:fake",
                "uid": "1",
            }))
        }
        "refresh_token" => {
            let refresh = &form["refresh_token"];
            if !state.refresh_tokens.contains(refresh) {
                return invalid_grant();
            }
            state.refreshes += 1;
            let access = state.next("access-");
            state.access_tokens.insert(access.clone(), refresh.clone());
            ok_json(json!({ "access_token": access, "token_type": "bearer", "expires_in": 14400 }))
        }
        _ => error(400, json!({ "error": "unsupported_grant_type" })),
    }
}

fn list_folder(state: &FakeState, path: &str, offset: usize) -> Reply {
    let folder = path.to_lowercase();
    if !folder.is_empty() && !state.folders.contains_key(&folder) {
        return error(
            409,
            json!({ "error_summary": "path/not_found/.", "error": {} }),
        );
    }
    let is_child = |key: &str| {
        key.strip_prefix(&folder)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|name| !name.is_empty() && !name.contains('/'))
    };
    let name_of = |path: &str| path.rsplit('/').next().unwrap().to_owned();
    let mut entries: Vec<Value> = state
        .folders
        .iter()
        .filter(|(key, _)| is_child(key))
        .map(|(key, path)| {
            json!({ ".tag": "folder", "name": name_of(path), "path_display": path, "path_lower": key, "id": "id:folder" })
        })
        .chain(state.files.iter().filter(|(key, _)| is_child(key)).map(|(key, file)| {
            json!({
                ".tag": "file", "name": name_of(&file.path), "path_display": file.path,
                "path_lower": key, "rev": file.rev, "size": file.bytes.len(), "id": "id:file",
            })
        }))
        .collect();
    let rest = entries.split_off(offset.min(entries.len()));
    let page: Vec<Value> = rest.iter().take(state.page_size).cloned().collect();
    let has_more = rest.len() > page.len();
    ok_json(json!({
        "entries": page,
        "cursor": format!("{path}|{}", offset + state.page_size),
        "has_more": has_more,
    }))
}

fn download(state: &FakeState, arg: &Value) -> Reply {
    let path = arg["path"].as_str().unwrap().to_lowercase();
    let Some(file) = state.files.get(&path) else {
        return error(
            409,
            json!({ "error_summary": "path/not_found/..", "error": {} }),
        );
    };
    let metadata =
        json!({ "name": file.path.rsplit('/').next(), "path_display": file.path, "rev": file.rev });
    Response::from_data(file.bytes.clone())
        .with_header(header("Content-Type", "application/octet-stream"))
        .with_header(header("Dropbox-API-Result", &ascii_json(&metadata)))
        .boxed()
}

fn metadata(state: &FakeState, args: &Value) -> Reply {
    let path = args["path"].as_str().unwrap().to_lowercase();
    if let Some(file) = state.files.get(&path) {
        return ok_json(json!({
            ".tag": "file", "name": file.path.rsplit('/').next(), "path_display": file.path,
            "path_lower": path, "rev": file.rev, "size": file.bytes.len(), "id": "id:file",
        }));
    }
    if let Some(folder) = state.folders.get(&path) {
        return ok_json(json!({
            ".tag": "folder", "name": folder.rsplit('/').next(), "path_display": folder,
            "path_lower": path, "id": "id:folder",
        }));
    }
    error(
        409,
        json!({ "error_summary": "path/not_found/..", "error": {} }),
    )
}

fn upload(state: &mut FakeState, arg: &Value, bytes: Vec<u8>) -> Reply {
    let path = arg["path"].as_str().unwrap().to_owned();
    let key = path.to_lowercase();
    let conflict = || {
        error(
            409,
            json!({ "error_summary": "path/conflict/file/...", "error": {} }),
        )
    };
    match (arg["mode"][".tag"].as_str(), state.files.get(&key)) {
        (Some("add"), Some(_)) => return conflict(),
        (Some("update"), Some(file)) if arg["mode"]["update"] != json!(file.rev) => {
            return conflict();
        }
        (Some("update"), None) => return conflict(),
        (Some("add" | "update"), _) => {}
        _ => return error(400, json!({ "error_summary": "unsupported mode" })),
    }
    let rev = state.next("rev");
    state.files.insert(
        key,
        FakeFile {
            path: path.clone(),
            bytes,
            rev: rev.clone(),
        },
    );
    ok_json(json!({ "name": path.rsplit('/').next(), "path_display": path, "rev": rev }))
}

/// JSON with every non-ASCII character escaped, as Dropbox sends it in headers.
fn ascii_json(value: &Value) -> String {
    value
        .to_string()
        .chars()
        .flat_map(|c| match c.is_ascii() {
            true => vec![c.to_string()],
            false => c
                .encode_utf16(&mut [0; 2])
                .iter()
                .map(|unit| format!("\\u{unit:04x}"))
                .collect(),
        })
        .collect()
}
