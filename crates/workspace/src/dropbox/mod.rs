//! The Dropbox Storage provider: connects a Dropbox account, browses it, and reads and writes
//! Vault files there through Dropbox's HTTP API. Its tokens live in the OS secret store, never
//! in the app config.

mod oauth;

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ureq::Body;
use ureq::http::Response;
use zeroize::Zeroizing;

use crate::secret_store::{SecretStore, SecretStoreError};
use crate::storage::{Revision, StorageError, StorageProvider, StoredFile, VaultLocation};

pub use oauth::{Authorization, ConnectCanceller};

/// The name the connected account's tokens are kept under in the secret store.
const SECRET_NAME: &str = "dropbox";
/// The largest Vault file keefast downloads.
const MAX_FILE_SIZE: u64 = 512 * 1024 * 1024;
/// An access token is renewed this long before Dropbox says it expires.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

/// Which Dropbox app keefast is, and where Dropbox is. Only tests point it elsewhere.
#[derive(Debug, Clone)]
pub struct DropboxConfig {
    /// The app key from the Dropbox App Console. A PKCE client has no secret.
    pub app_key: String,
    pub api_url: String,
    pub content_url: String,
    pub authorize_url: String,
    /// The loopback port Dropbox sends the browser back to; 0 picks a free one. The app's
    /// redirect URI, `http://localhost:<port>/dropbox`, must be registered with Dropbox.
    pub redirect_port: u16,
}

impl DropboxConfig {
    pub const DEFAULT_REDIRECT_PORT: u16 = 47_826;

    pub fn new(app_key: impl Into<String>) -> Self {
        Self {
            app_key: app_key.into(),
            api_url: "https://api.dropboxapi.com".into(),
            content_url: "https://content.dropboxapi.com".into(),
            authorize_url: "https://www.dropbox.com/oauth2/authorize".into(),
            redirect_port: Self::DEFAULT_REDIRECT_PORT,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DropboxError {
    #[error("Dropbox is not set up in this build of keefast")]
    NotConfigured,
    #[error("no Dropbox account is connected")]
    NotConnected,
    #[error("Dropbox no longer accepts keefast's access; connect the account again")]
    AccessRevoked,
    #[error("connecting Dropbox was cancelled")]
    Cancelled,
    #[error("Dropbox was not connected: {0}")]
    Refused(String),
    #[error("could not wait for Dropbox to send the browser back: {0}")]
    Callback(#[source] std::io::Error),
    #[error("could not reach Dropbox: {0}")]
    Network(String),
    #[error("the file is not in Dropbox")]
    NotFound,
    #[error("the Vault file changed in Dropbox since keefast read it")]
    Conflict,
    #[error("Dropbox answered with an error: {0}")]
    Api(String),
    #[error(transparent)]
    SecretStore(#[from] SecretStoreError),
}

/// The connected Dropbox account, as the user knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxAccount {
    pub name: String,
    pub email: String,
}

/// A file or folder in Dropbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxItem {
    pub name: String,
    /// The full path, as Dropbox displays it: `/Vaults/Personal.kdbx`.
    pub path: String,
    pub is_folder: bool,
    /// A file's size in bytes; `None` for a folder.
    pub size: Option<u64>,
    /// When a file last changed in Dropbox, as Dropbox writes it (`2026-10-07T23:15:22Z`, UTC);
    /// `None` for a folder.
    pub modified: Option<String>,
}

impl DropboxItem {
    /// Where the file is, as a Vault file location.
    pub fn location(&self) -> VaultLocation {
        VaultLocation::Dropbox(self.path.clone())
    }
}

/// A connected account's tokens, as kept in the secret store.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    refresh_token: Zeroizing<String>,
    name: String,
    email: String,
}

struct Session {
    refresh_token: SecretString,
    account: DropboxAccount,
}

struct AccessToken {
    token: SecretString,
    expires: Instant,
}

#[derive(Default)]
struct State {
    /// `None` until read from the secret store; then the connected account, if any.
    session: Option<Option<Session>>,
    access: Option<AccessToken>,
}

/// The Dropbox Storage provider. Every call blocks on the network, so run them off the UI
/// thread.
pub struct Dropbox {
    config: DropboxConfig,
    secrets: Arc<dyn SecretStore>,
    agent: ureq::Agent,
    state: Mutex<State>,
}

impl Dropbox {
    pub fn new(config: DropboxConfig, secrets: Arc<dyn SecretStore>) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_recv_response(Some(Duration::from_secs(60)))
                .build(),
        );
        Self {
            config,
            secrets,
            agent,
            state: Mutex::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The connected account, or `None` when none is. Reads the secret store the first time.
    pub fn account(&self) -> Result<Option<DropboxAccount>, DropboxError> {
        let mut state = self.lock();
        Ok(self
            .load_session(&mut state)?
            .map(|session| session.account.clone()))
    }

    fn load_session<'a>(&self, state: &'a mut State) -> Result<Option<&'a Session>, DropboxError> {
        if state.session.is_none() {
            let session = match self.secrets.get(SECRET_NAME)? {
                None => None,
                // Something keefast can't read is as good as no account: connecting replaces it.
                Some(stored) => serde_json::from_str::<StoredSession>(stored.expose_secret())
                    .ok()
                    .map(|stored| Session {
                        refresh_token: SecretString::from(stored.refresh_token.as_str()),
                        account: DropboxAccount {
                            name: stored.name,
                            email: stored.email,
                        },
                    }),
            };
            state.session = Some(session);
        }
        Ok(state.session.as_ref().and_then(Option::as_ref))
    }

    /// Starts connecting a Dropbox account: open [`Authorization::url`] in the user's browser,
    /// then call [`Self::finish_connect`].
    pub fn begin_connect(&self) -> Result<Authorization, DropboxError> {
        if self.config.app_key.is_empty() {
            return Err(DropboxError::NotConfigured);
        }
        Authorization::begin(&self.config, false)
    }

    /// Starts connecting another Dropbox account than the one connected, as
    /// [`Self::begin_connect`] does, except that Dropbox's page asks the user to sign in rather
    /// than letting in the account the browser is signed in to. The account connected stays
    /// until [`Self::finish_connect`] replaces it.
    pub fn begin_connect_another(&self) -> Result<Authorization, DropboxError> {
        if self.config.app_key.is_empty() {
            return Err(DropboxError::NotConfigured);
        }
        Authorization::begin(&self.config, true)
    }

    /// Waits for the user to let keefast in, then keeps the account's tokens in the secret
    /// store. Replaces the account connected before, if any.
    pub fn finish_connect(
        &self,
        authorization: Authorization,
    ) -> Result<DropboxAccount, DropboxError> {
        let code = authorization.wait_for_code()?;
        let tokens = self.request_tokens(&[
            ("grant_type", "authorization_code"),
            ("code", code.expose_secret()),
            ("code_verifier", authorization.verifier.expose_secret()),
            ("client_id", &self.config.app_key),
            ("redirect_uri", &authorization.redirect_uri),
        ])?;
        let Some(refresh_token) = tokens.refresh_token else {
            return Err(DropboxError::Api("Dropbox sent no refresh token".into()));
        };
        let access = tokens.access_token;
        let response =
            error_for_status(self.rpc_request("users/get_current_account", "null", &access)?)?;
        let profile: Value = read_json(response)?;
        let account = DropboxAccount {
            name: profile["name"]["display_name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            email: profile["email"].as_str().unwrap_or_default().to_owned(),
        };
        let stored = Zeroizing::new(
            serde_json::to_string(&StoredSession {
                refresh_token: Zeroizing::new(refresh_token.expose_secret().to_owned()),
                name: account.name.clone(),
                email: account.email.clone(),
            })
            .expect("a stored session serializes"),
        );
        self.secrets
            .set(SECRET_NAME, &SecretString::from(stored.as_str()))?;
        *self.lock() = State {
            session: Some(Some(Session {
                refresh_token,
                account: account.clone(),
            })),
            access: Some(AccessToken {
                token: access,
                expires: Instant::now() + tokens.expires_in,
            }),
        };
        Ok(account)
    }

    /// Disconnects the account: tells Dropbox to revoke keefast's tokens, if it can be
    /// reached, and removes them from the secret store.
    pub fn disconnect(&self) -> Result<(), DropboxError> {
        if let Ok(access) = self.access_token() {
            // Revoking is a courtesy; the tokens are gone from this machine regardless.
            let _ = self.rpc_request("auth/token/revoke", "null", &access);
        }
        let mut state = self.lock();
        self.secrets.delete(SECRET_NAME)?;
        *state = State {
            session: Some(None),
            ..State::default()
        };
        Ok(())
    }

    /// The files and folders in a Dropbox folder, folders first, then by name. `""` is the
    /// top of the Dropbox.
    pub fn list_folder(&self, path: &str) -> Result<Vec<DropboxItem>, DropboxError> {
        let mut items = Vec::new();
        let mut page = self.rpc("files/list_folder", &json!({ "path": path }))?;
        loop {
            for entry in page["entries"].as_array().into_iter().flatten() {
                let is_folder = match entry[".tag"].as_str() {
                    Some("folder") => true,
                    Some("file") => false,
                    _ => continue,
                };
                let (Some(name), Some(path)) =
                    (entry["name"].as_str(), entry["path_display"].as_str())
                else {
                    continue;
                };
                items.push(DropboxItem {
                    name: name.to_owned(),
                    path: path.to_owned(),
                    is_folder,
                    size: entry["size"].as_u64(),
                    modified: entry["server_modified"].as_str().map(str::to_owned),
                });
            }
            if page["has_more"] != Value::Bool(true) {
                break;
            }
            let cursor = page["cursor"].clone();
            page = self.rpc("files/list_folder/continue", &json!({ "cursor": cursor }))?;
        }
        items.sort_by_cached_key(|item| (!item.is_folder, item.name.to_lowercase()));
        Ok(items)
    }

    /// Downloads a file, with its revision.
    fn download(&self, path: &str) -> Result<(Vec<u8>, String), DropboxError> {
        let url = format!("{}/2/files/download", self.config.content_url);
        let arg = api_arg(&json!({ "path": path }));
        let mut response = self.call(|token| {
            self.agent
                .post(&url)
                .header("Authorization", bearer(token))
                .header("Dropbox-API-Arg", &arg)
                .send_empty()
        })?;
        let metadata: Value = response
            .headers()
            .get("Dropbox-API-Result")
            .and_then(|value| serde_json::from_slice(value.as_bytes()).ok())
            .ok_or_else(|| DropboxError::Api("Dropbox sent no file metadata".into()))?;
        let rev = metadata["rev"].as_str().unwrap_or_default().to_owned();
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_FILE_SIZE)
            .read_to_vec()
            .map_err(network)?;
        Ok((bytes, rev))
    }

    /// Uploads a file, treating a file already there as `mode` says. Returns the new revision.
    fn upload(&self, path: &str, bytes: &[u8], mode: Upload<'_>) -> Result<String, DropboxError> {
        let url = format!("{}/2/files/upload", self.config.content_url);
        let mode = match mode {
            Upload::Replace(rev) => json!({ ".tag": "update", "update": rev }),
            Upload::Overwrite => json!({ ".tag": "overwrite" }),
            // With `autorename` off, Dropbox refuses rather than add the file under another name.
            Upload::New => json!({ ".tag": "add" }),
        };
        let arg = api_arg(&json!({
            "path": path,
            "mode": mode,
            "autorename": false,
            "mute": true,
            "strict_conflict": true,
        }));
        let response = self.call(|token| {
            self.agent
                .post(&url)
                .header("Authorization", bearer(token))
                .header("Dropbox-API-Arg", &arg)
                .header("Content-Type", "application/octet-stream")
                .send(bytes)
        })?;
        let metadata: Value = read_json(response)?;
        Ok(metadata["rev"].as_str().unwrap_or_default().to_owned())
    }

    /// Calls an RPC endpoint with JSON arguments and returns its JSON answer.
    fn rpc(&self, endpoint: &str, args: &Value) -> Result<Value, DropboxError> {
        let body = args.to_string();
        let response = self.call(|token| self.send_rpc(endpoint, &body, token))?;
        read_json(response)
    }

    fn rpc_request(
        &self,
        endpoint: &str,
        body: &str,
        token: &SecretString,
    ) -> Result<Response<Body>, DropboxError> {
        self.send_rpc(endpoint, body, token.expose_secret())
            .map_err(network)
    }

    fn send_rpc(
        &self,
        endpoint: &str,
        body: &str,
        token: &str,
    ) -> Result<Response<Body>, ureq::Error> {
        self.agent
            .post(format!("{}/2/{endpoint}", self.config.api_url))
            .header("Authorization", bearer(token))
            .header("Content-Type", "application/json")
            .send(body)
    }

    /// Makes an authorized request, renewing the access token and trying again once if
    /// Dropbox turns the token down.
    fn call(
        &self,
        request: impl Fn(&str) -> Result<Response<Body>, ureq::Error>,
    ) -> Result<Response<Body>, DropboxError> {
        let token = self.access_token()?;
        let response = request(token.expose_secret()).map_err(network)?;
        if response.status().as_u16() != 401 {
            return error_for_status(response);
        }
        self.lock().access = None;
        let token = self.access_token()?;
        let response = request(token.expose_secret()).map_err(network)?;
        if response.status().as_u16() == 401 {
            return Err(DropboxError::AccessRevoked);
        }
        error_for_status(response)
    }

    /// A current access token, renewed with the refresh token when needed.
    /// The renewal happens without holding the state lock, so other calls don't wait on it.
    fn access_token(&self) -> Result<SecretString, DropboxError> {
        let refresh_token = {
            let mut state = self.lock();
            if let Some(access) = &state.access
                && Instant::now() + EXPIRY_MARGIN < access.expires
            {
                return Ok(access.token.clone());
            }
            self.load_session(&mut state)?
                .ok_or(DropboxError::NotConnected)?
                .refresh_token
                .clone()
        };
        let tokens = self.request_tokens(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose_secret()),
            ("client_id", &self.config.app_key),
        ])?;
        let mut state = self.lock();
        // Disconnected or reconnected meanwhile: this token belongs to no current account.
        let current = state.session.as_ref().and_then(Option::as_ref);
        if current.map(|session| session.refresh_token.expose_secret())
            != Some(refresh_token.expose_secret())
        {
            return Err(DropboxError::NotConnected);
        }
        state.access = Some(AccessToken {
            token: tokens.access_token.clone(),
            expires: Instant::now() + tokens.expires_in,
        });
        Ok(tokens.access_token)
    }

    /// Asks Dropbox's token endpoint for tokens.
    fn request_tokens(&self, form: &[(&str, &str)]) -> Result<Tokens, DropboxError> {
        let mut response = self
            .agent
            .post(format!("{}/oauth2/token", self.config.api_url))
            .send_form(form.iter().copied())
            .map_err(network)?;
        let status = response.status().as_u16();
        let body = Zeroizing::new(response.body_mut().read_to_string().map_err(network)?);
        if status == 400 && body.contains("invalid_grant") {
            return Err(DropboxError::AccessRevoked);
        }
        if status != 200 {
            return Err(DropboxError::Api(format!(
                "HTTP {status} from the token endpoint"
            )));
        }
        let tokens: TokenResponse = serde_json::from_str(&body)
            .map_err(|error| DropboxError::Api(format!("unreadable tokens: {error}")))?;
        Ok(Tokens {
            access_token: SecretString::from(tokens.access_token.as_str()),
            refresh_token: tokens
                .refresh_token
                .map(|token| SecretString::from(token.as_str())),
            expires_in: Duration::from_secs(tokens.expires_in.unwrap_or(4 * 60 * 60)),
        })
    }

    fn location_path<'a>(&self, location: &'a VaultLocation) -> Result<&'a str, StorageError> {
        match location {
            VaultLocation::Dropbox(path) => Ok(path),
            _ => Err(StorageError::Unavailable(location.clone())),
        }
    }
}

/// How an upload treats a file already at its path.
enum Upload<'a> {
    /// Replaces the file only if it is at this revision.
    Replace(&'a str),
    /// Replaces whatever is there.
    Overwrite,
    /// Uploads only if no file is there.
    New,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Zeroizing<String>,
    refresh_token: Option<Zeroizing<String>>,
    expires_in: Option<u64>,
}

struct Tokens {
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    expires_in: Duration,
}

impl StorageProvider for Dropbox {
    fn read(&self, location: &VaultLocation) -> Result<StoredFile, StorageError> {
        let path = self.location_path(location)?;
        let (bytes, rev) = self
            .download(path)
            .map_err(|error| storage_error(error, location))?;
        Ok(StoredFile {
            bytes,
            revision: Revision::new(rev),
        })
    }

    /// Asks Dropbox for the file's metadata only, so checking for changes doesn't download it.
    fn revision(&self, location: &VaultLocation) -> Result<Revision, StorageError> {
        let path = self.location_path(location)?;
        let metadata = self
            .rpc("files/get_metadata", &json!({ "path": path }))
            .map_err(|error| storage_error(error, location))?;
        match (metadata[".tag"].as_str(), metadata["rev"].as_str()) {
            (Some("file"), Some(rev)) => Ok(Revision::new(rev)),
            // A folder, or a file deleted: no Vault file there.
            _ => Err(StorageError::NotFound(location.clone())),
        }
    }

    /// Uploads with Dropbox's own revision check, so a change made on another device in
    /// between is never overwritten: Dropbox refuses, and the save Merges it in first.
    fn write(
        &self,
        location: &VaultLocation,
        bytes: &[u8],
        expected: Option<&Revision>,
    ) -> Result<Revision, StorageError> {
        let path = self.location_path(location)?;
        let mode = match expected {
            Some(expected) => Upload::Replace(expected.as_str()),
            None => Upload::Overwrite,
        };
        let rev = self
            .upload(path, bytes, mode)
            .map_err(|error| storage_error(error, location))?;
        Ok(Revision::new(rev))
    }

    /// Uploads in Dropbox's add mode, which refuses when a file is there.
    fn create(&self, location: &VaultLocation, bytes: &[u8]) -> Result<Revision, StorageError> {
        let path = self.location_path(location)?;
        let rev = self
            .upload(path, bytes, Upload::New)
            .map_err(|error| match error {
                DropboxError::Conflict => StorageError::Exists(location.clone()),
                error => storage_error(error, location),
            })?;
        Ok(Revision::new(rev))
    }
}

fn storage_error(error: DropboxError, location: &VaultLocation) -> StorageError {
    match error {
        DropboxError::NotFound => StorageError::NotFound(location.clone()),
        DropboxError::Conflict => StorageError::Conflict,
        DropboxError::Network(reason) => StorageError::Offline(reason),
        error => StorageError::Dropbox(error),
    }
}

/// A Dropbox answer, or the error it reports.
fn error_for_status(mut response: Response<Body>) -> Result<Response<Body>, DropboxError> {
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(response);
    }
    let body = response.body_mut().read_to_string().unwrap_or_default();
    let summary = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|error| error["error_summary"].as_str().map(str::to_owned));
    Err(match (status, summary) {
        (401, _) => DropboxError::AccessRevoked,
        (409, Some(summary)) if summary.contains("not_found") => DropboxError::NotFound,
        (409, Some(summary)) if summary.starts_with("path/conflict") => DropboxError::Conflict,
        (_, Some(summary)) => DropboxError::Api(summary),
        (_, None) => DropboxError::Api(format!("HTTP {status}")),
    })
}

fn network(error: ureq::Error) -> DropboxError {
    DropboxError::Network(error.to_string())
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn read_json(mut response: Response<Body>) -> Result<Value, DropboxError> {
    let body = response.body_mut().read_to_vec().map_err(network)?;
    serde_json::from_slice(&body)
        .map_err(|error| DropboxError::Api(format!("unreadable answer: {error}")))
}

/// JSON for the `Dropbox-API-Arg` header. HTTP headers carry ASCII only, so Dropbox wants
/// every other character, and DEL, escaped as `\uXXXX`.
fn api_arg(value: &Value) -> String {
    let mut arg = String::new();
    for c in value.to_string().chars() {
        if c.is_ascii() && c != '\u{7f}' {
            arg.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0; 2]) {
                let _ = write!(arg, "\\u{unit:04x}");
            }
        }
    }
    arg
}
