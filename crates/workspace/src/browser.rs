//! The browser request handler: the KeePassXC-Browser protocol (ADR 0003) spoken with the
//! browser extensions, on top of the Workspace.
//!
//! Each connection from a native host gets a [`BrowserSession`], which holds that
//! connection's encryption keys. Requests are JSON text; replies are JSON text to send back.
//! Pairing needs the user's approval, so `associate` requests wait in a queue, one put to the
//! user at a time, until approved with a name or denied. Likewise a `get-logins` request for
//! Entries the user hasn't decided on for that site waits for an Access decision, and a
//! `set-login` request waits for the user to say where the login goes.

mod generate_password;
mod get_totp;

use std::collections::{HashMap, VecDeque};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use crypto_box::aead::{Aead, OsRng};
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use keefast_core::{AccessDecision, CapturedLogin, EntryId, GroupId, Site, SiteEntry, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};
use zeroize::Zeroize;

use crate::{EditVaultError, VaultId, Workspace};

/// The KeePassXC version keefast answers as. The extension enables protocol features by
/// KeePassXC version, and refuses versions it considers too old.
pub const KEEPASSXC_VERSION: &str = "2.7.10";

const NONCE_SIZE: usize = 24;
const KEY_SIZE: usize = 32;

/// The protocol's error codes that keefast replies with, as KeePassXC numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    /// No Vault is open: keefast is locked.
    DatabaseNotOpened = 1,
    DatabaseHashNotReceived = 2,
    ClientPublicKeyNotReceived = 3,
    CannotDecryptMessage = 4,
    /// keefast can't be reached: answered while browser integration is off.
    NotRunning = 5,
    ActionCancelledOrDenied = 6,
    AssociationFailed = 8,
    KeyChangeFailed = 9,
    IncorrectAction = 12,
    EmptyMessageReceived = 13,
    NoUrlProvided = 14,
    NoLoginsFound = 15,
    NoValidUuidProvided = 18,
}

impl ProtocolError {
    fn message(self) -> &'static str {
        match self {
            ProtocolError::DatabaseNotOpened => "keefast is locked: unlock a Vault in keefast",
            ProtocolError::DatabaseHashNotReceived => "Database hash not available",
            ProtocolError::ClientPublicKeyNotReceived => "Client public key not received",
            ProtocolError::CannotDecryptMessage => "Cannot decrypt message",
            ProtocolError::NotRunning => {
                "keefast is not running, or its browser integration is off"
            }
            ProtocolError::ActionCancelledOrDenied => "Action cancelled or denied",
            ProtocolError::AssociationFailed => "Association failed",
            ProtocolError::KeyChangeFailed => "Key change was not successful",
            ProtocolError::IncorrectAction => "Incorrect action",
            ProtocolError::EmptyMessageReceived => "Empty message received",
            ProtocolError::NoUrlProvided => "No URL provided",
            ProtocolError::NoLoginsFound => "No logins found",
            ProtocolError::NoValidUuidProvided => "No valid UUID provided",
        }
    }
}

/// What to do with a request.
#[derive(Debug)]
pub enum BrowserReply {
    /// Send this reply to the extension.
    Send(String),
    /// Ask the user whether to pair the extension with a Vault, then answer the request.
    AskPairing(PairingRequest),
    /// Ask the user whether the site may have Entries, then answer the request.
    AskAccess(AccessRequest),
    /// Ask the user where a captured login goes, then answer the request.
    AskLogin(LoginRequest),
}

/// An extension asking to be paired with a Vault, waiting for the user's answer.
#[derive(Debug, Clone)]
pub struct PairingRequest {
    vault: VaultId,
    /// The identification public key the extension will prove itself with from now on.
    id_key: String,
    request: Request,
}

impl PairingRequest {
    /// The Vault the extension would be paired with.
    pub fn vault(&self) -> VaultId {
        self.vault
    }
}

/// A site asking for Entries the user hasn't decided on for it yet, waiting for the user's
/// answer.
#[derive(Debug, Clone)]
pub struct AccessRequest {
    query: SiteQuery,
    /// The Entries to decide on.
    undecided: Vec<(VaultId, EntryId)>,
    request: Request,
}

impl AccessRequest {
    /// The host of the site asking.
    pub fn site(&self) -> &str {
        &self.query.site_host
    }

    /// The Entries to decide on.
    pub fn entries(&self) -> &[(VaultId, EntryId)] {
        &self.undecided
    }
}

/// The user's answer to a site asking for Entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessAnswer {
    pub decision: AccessDecision,
    /// Whether to remember the decision on the Entries, for this site.
    pub remember: bool,
}

/// A login the browser captured, waiting for the user to say where it goes.
#[derive(Debug, Clone)]
pub struct LoginRequest {
    login: CapturedLogin,
    /// The Pairings the extension proved, as name and key: the login may go to the open
    /// Vaults paired with one of them.
    pairings: Vec<(String, String)>,
    /// The Entry the extension picked to update, if it did.
    picked: Option<EntryId>,
    request: Request,
}

/// Where the user saves a captured login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginTarget {
    /// A new Entry in `group`, or in the Vault's Browser group, made if the Vault has none.
    NewEntry {
        vault: VaultId,
        group: Option<GroupId>,
    },
    /// The Entry, given the login's username and password; its old version goes to history.
    Update { vault: VaultId, entry: EntryId },
}

/// What a `get-logins` request asks for: the Entries for a site.
#[derive(Debug, Clone)]
struct SiteQuery {
    site: Site,
    /// The site's host, which Access decisions are made for.
    site_host: String,
    /// The host the site's form submits to, if that differs: it needs the decision too.
    form_host: Option<String>,
    /// The Pairings the extension holds keys for, as name and key.
    keys: Vec<(String, String)>,
    /// The `id` the extension sent, echoed back.
    id: String,
}

impl SiteQuery {
    /// The hosts an Access decision is for.
    fn hosts(&self) -> Vec<String> {
        std::iter::once(self.site_host.clone())
            .chain(self.form_host.clone())
            .collect()
    }
}

/// An Entry offered to a site, with its Access decision for the site.
struct Candidate<'a> {
    vault: VaultId,
    found: SiteEntry<'a>,
    decision: Option<AccessDecision>,
}

impl Candidate<'_> {
    fn id(&self) -> (VaultId, EntryId) {
        (self.vault, self.found.entry.id())
    }

    /// The Entry as the extension receives it, with its TOTP code at `now` if it has TOTP.
    fn to_protocol(&self, now: Timestamp) -> Value {
        let SiteEntry { entry, group } = self.found;
        let mut fields = json!({
            "login": entry.username(),
            "name": entry.title(),
            "password": entry.password().expose_secret(),
            "uuid": entry.id().to_hex(),
            "group": group.name(),
            "stringFields": [],
        });
        if let Some(Ok(totp)) = entry.totp() {
            fields["totp"] = totp.code(now).code().into();
        }
        fields
    }
}

/// What every reply to a request needs from it.
#[derive(Debug, Clone)]
struct Request {
    action: String,
    nonce: [u8; NONCE_SIZE],
    request_id: Option<Value>,
}

/// One extension's conversation with keefast, from its key exchange on.
#[derive(Default)]
pub struct BrowserSession {
    keys: Option<SessionKeys>,
    /// The Pairing names the extension last said it holds keys for.
    connected_ids: Vec<String>,
    /// The Pairings the extension proved it holds, as name and key.
    proven: Vec<(String, String)>,
    /// The Entries the extension was last given for a site, each with that site. A site
    /// turning an Entry down takes it back.
    allowed: HashMap<(VaultId, EntryId), SiteQuery>,
}

struct SessionKeys {
    crypto: SalsaBox,
    /// The extension's public key for this session, Base64-encoded as it sent it.
    client_key: String,
}

impl BrowserSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles one request from the extension.
    pub fn handle(&mut self, workspace: &Workspace, request: &str) -> BrowserReply {
        let Ok(Value::Object(request)) = serde_json::from_str::<Value>(request) else {
            return BrowserReply::Send(error_reply("", None, ProtocolError::EmptyMessageReceived));
        };
        let action = text(&request, "action").to_owned();
        let request_id = request.get("requestID").cloned();
        let fail = |error| BrowserReply::Send(error_reply(&action, request_id.clone(), error));
        if action.is_empty() {
            return fail(ProtocolError::IncorrectAction);
        }
        if action != "change-public-keys" && workspace.vaults.is_empty() {
            return fail(match self.keys {
                None => ProtocolError::ClientPublicKeyNotReceived,
                Some(_) => ProtocolError::DatabaseNotOpened,
            });
        }
        let reply = match action.as_str() {
            "change-public-keys" => self.change_public_keys(&request),
            "get-databasehash" => self.get_database_hash(workspace, &request),
            "associate" => self.associate(workspace, &request),
            "test-associate" => self.test_associate(workspace, &request),
            "get-logins" => self.get_logins(workspace, &request),
            "set-login" => self.set_login(workspace, &request),
            "get-totp" => self.get_totp(workspace, &request),
            "generate-password" => self.generate_password(workspace, &request),
            _ => Err(ProtocolError::IncorrectAction),
        };
        reply.unwrap_or_else(fail)
    }

    /// Pairs the extension with the request's Vault under `name`, and returns the reply to
    /// send. A name the Vault can't take is refused, and the request can be answered again.
    pub fn approve_pairing(
        &self,
        workspace: &mut Workspace,
        pairing: &PairingRequest,
        name: &str,
    ) -> Result<String, EditVaultError> {
        let request = &pairing.request;
        match workspace.pair_browser(pairing.vault, name, &pairing.id_key) {
            Ok(()) => {}
            Err(EditVaultError::VaultNotOpen) => {
                return Ok(self.error(request, ProtocolError::DatabaseNotOpened));
            }
            Err(error) => return Err(error),
        }
        let hash = workspace
            .vault(pairing.vault)
            .expect("just paired")
            .vault()
            .browser_hash();
        Ok(self.encrypted_reply(request, json!({ "hash": hash, "id": name.trim() })))
    }

    /// The reply to send when the user turns a Pairing down.
    pub fn deny_pairing(&self, pairing: &PairingRequest) -> String {
        self.error(&pairing.request, ProtocolError::ActionCancelledOrDenied)
    }

    /// Answers a site asking for Entries with the user's decision on them, remembered on the
    /// Entries if the user wants, and returns the reply to send.
    pub(crate) fn answer_access(
        &mut self,
        workspace: &mut Workspace,
        access: &AccessRequest,
        answer: AccessAnswer,
    ) -> String {
        if answer.remember {
            let hosts = access.query.hosts();
            for &(vault, entry) in &access.undecided {
                // An Entry gone meanwhile has nothing to remember a decision on.
                let _ = workspace.edit(vault, |vault, now| {
                    vault.remember_access(entry, &hosts, answer.decision, now)
                });
            }
        }
        let decided = |vault, entry| {
            (answer.decision == AccessDecision::Allow && access.undecided.contains(&(vault, entry)))
                .then_some(AccessDecision::Allow)
        };
        self.entries_reply(workspace, &access.request, &access.query, decided)
    }

    /// Starts the session's encryption: the extension sends its public key, keefast answers
    /// with a new key pair's public key.
    fn change_public_keys(
        &mut self,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let client_key = text(request, "publicKey");
        let nonce = text(request, "nonce");
        if client_key.is_empty() || nonce.is_empty() {
            return Err(ProtocolError::ClientPublicKeyNotReceived);
        }
        let public_key = decode::<KEY_SIZE>(client_key).ok_or(ProtocolError::KeyChangeFailed)?;
        let nonce = decode::<NONCE_SIZE>(nonce).ok_or(ProtocolError::KeyChangeFailed)?;
        let secret = SecretKey::generate(&mut OsRng);
        let host_key = BASE64.encode(secret.public_key().as_bytes());
        self.keys = Some(SessionKeys {
            crypto: SalsaBox::new(&PublicKey::from(public_key), &secret),
            client_key: client_key.to_owned(),
        });
        // New keys start a new conversation: nothing given before carries over.
        self.allowed.clear();
        let mut reply = success(increment(nonce));
        reply.insert("action".into(), "change-public-keys".into());
        reply.insert("publicKey".into(), host_key.into());
        with_request_id(&mut reply, request.get("requestID").cloned());
        Ok(BrowserReply::Send(Value::Object(reply).to_string()))
    }

    fn get_database_hash(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        if text(&message, "action") != "get-databasehash" {
            return Err(ProtocolError::DatabaseHashNotReceived);
        }
        if let Some(Value::Array(ids)) = message.get("connectedKeys") {
            self.connected_ids = ids
                .iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect();
        }
        let vault = workspace
            .browser_vault(&self.connected_ids)
            .ok_or(ProtocolError::DatabaseNotOpened)?;
        Ok(BrowserReply::Send(self.encrypted_reply(
            &request,
            json!({ "hash": browser_hash(workspace, vault) }),
        )))
    }

    fn associate(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        if text(&message, "action") != "associate" {
            return Err(ProtocolError::IncorrectAction);
        }
        // The extension proves it holds this session's key, and sends the key it will
        // identify itself with from now on.
        let session_key = &self.keys.as_ref().expect("decrypted").client_key;
        let id_key = text(&message, "idKey");
        if text(&message, "key") != session_key || decode::<KEY_SIZE>(id_key).is_none() {
            return Err(ProtocolError::AssociationFailed);
        }
        let vault = workspace
            .browser_vault(&self.connected_ids)
            .ok_or(ProtocolError::DatabaseNotOpened)?;
        Ok(BrowserReply::AskPairing(PairingRequest {
            vault,
            id_key: id_key.to_owned(),
            request,
        }))
    }

    fn test_associate(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        if text(&message, "action") != "test-associate" {
            return Err(ProtocolError::IncorrectAction);
        }
        let (id, key) = (text(&message, "id"), text(&message, "key"));
        // KeePassXC answers a request without both as if no database were open.
        if id.is_empty() || key.is_empty() {
            return Err(ProtocolError::DatabaseNotOpened);
        }
        let vault = workspace
            .paired_vault(id, key, &self.connected_ids)
            .ok_or(ProtocolError::AssociationFailed)?;
        let pairing = (id.to_owned(), key.to_owned());
        if !self.proven.contains(&pairing) {
            self.proven.push(pairing);
        }
        Ok(BrowserReply::Send(self.encrypted_reply(
            &request,
            json!({ "hash": browser_hash(workspace, vault), "id": id }),
        )))
    }

    /// Finds the Entries for a site in the Vaults the extension is paired with, and answers
    /// with those the user has allowed the site, or asks the user about the others first.
    /// Without any Entry for the site that isn't denied it, there are no logins found.
    fn get_logins(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        if text(&message, "action") != "get-logins" {
            return Err(ProtocolError::IncorrectAction);
        }
        let url = text(&message, "url");
        if url.is_empty() {
            return Err(ProtocolError::NoUrlProvided);
        }
        let site = Site::parse(url).ok_or(ProtocolError::NoLoginsFound)?;
        let site_host = site.host();
        let form_host = Site::parse(text(&message, "submitUrl"))
            .map(|form| form.host())
            .filter(|form| *form != site_host);
        let keys = match message.get("keys") {
            Some(Value::Array(keys)) => keys
                .iter()
                .filter_map(Value::as_object)
                .map(|key| (text(key, "id").to_owned(), text(key, "key").to_owned()))
                .collect(),
            _ => Vec::new(),
        };
        let query = SiteQuery {
            site,
            site_host,
            form_host,
            keys,
            id: text(&message, "id").to_owned(),
        };
        let candidates = candidates(workspace, &query);
        if candidates.is_empty() {
            return Err(ProtocolError::NoLoginsFound);
        }
        let undecided: Vec<(VaultId, EntryId)> = candidates
            .iter()
            .filter(|candidate| candidate.decision.is_none())
            .map(Candidate::id)
            .collect();
        if undecided.is_empty() {
            return Ok(BrowserReply::Send(self.entries_reply(
                workspace,
                &request,
                &query,
                |_, _| None,
            )));
        }
        Ok(BrowserReply::AskAccess(AccessRequest {
            query,
            undecided,
            request,
        }))
    }

    /// Takes a login the user submitted, to ask the user where it goes. The extension must have
    /// proven a Pairing with an open Vault, as KeePassXC requires a successful
    /// `test-associate` first. The Group it asks for is ignored: the user decides.
    fn set_login(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, mut message) = self.decrypt(request)?;
        let password = SecretString::from(text(&message, "password").to_owned());
        if let Some(Value::String(sent)) = message.get_mut("password") {
            sent.zeroize();
        }
        if text(&message, "action") != "set-login" {
            return Err(ProtocolError::IncorrectAction);
        }
        let url = text(&message, "url");
        if url.is_empty() {
            return Err(ProtocolError::NoUrlProvided);
        }
        let login = LoginRequest {
            login: CapturedLogin {
                url: url.to_owned(),
                username: text(&message, "login").to_owned(),
                password,
            },
            pairings: self.proven.clone(),
            picked: EntryId::from_hex(text(&message, "uuid")),
            request,
        };
        if workspace.login_vaults(&login).is_empty() {
            return Err(ProtocolError::AssociationFailed);
        }
        Ok(BrowserReply::AskLogin(login))
    }

    /// The reply to a saved login, which KeePassXC sends without Entries.
    fn login_saved_reply(
        &self,
        workspace: &Workspace,
        login: &LoginRequest,
        vault: VaultId,
    ) -> String {
        self.encrypted_reply(
            &login.request,
            json!({
                "count": null,
                "entries": null,
                "error": "success",
                "hash": browser_hash(workspace, vault),
            }),
        )
    }

    /// The reply with the Entries the site is allowed, with `decided` standing in for those
    /// without a remembered decision. There may be none, once the user has turned the site
    /// down, which KeePassXC answers with an empty list too.
    fn entries_reply(
        &mut self,
        workspace: &Workspace,
        request: &Request,
        query: &SiteQuery,
        decided: impl Fn(VaultId, EntryId) -> Option<AccessDecision>,
    ) -> String {
        if workspace.vaults.is_empty() {
            return self.error(request, ProtocolError::DatabaseNotOpened);
        }
        let now = workspace.now();
        let (allowed, turned_down): (Vec<Candidate>, Vec<Candidate>) = candidates(workspace, query)
            .into_iter()
            .partition(|candidate| {
                let (vault, entry) = candidate.id();
                let decision = candidate.decision.or_else(|| decided(vault, entry));
                decision == Some(AccessDecision::Allow)
            });
        for candidate in &turned_down {
            self.allowed.remove(&candidate.id());
        }
        for candidate in &allowed {
            self.allowed.insert(candidate.id(), query.clone());
        }
        let entries: Vec<Value> = allowed
            .iter()
            .map(|candidate| candidate.to_protocol(now))
            .collect();
        let vault = workspace
            .browser_vault(&self.connected_ids)
            .expect("a Vault is open");
        self.encrypted_reply(
            request,
            json!({
                "count": entries.len(),
                "entries": entries,
                "hash": browser_hash(workspace, vault),
                "id": query.id,
            }),
        )
    }

    /// The request's encrypted `message`, as JSON.
    fn decrypt(
        &self,
        request: &Map<String, Value>,
    ) -> Result<(Request, Map<String, Value>), ProtocolError> {
        let keys = self
            .keys
            .as_ref()
            .ok_or(ProtocolError::CannotDecryptMessage)?;
        let nonce = decode::<NONCE_SIZE>(text(request, "nonce"))
            .ok_or(ProtocolError::CannotDecryptMessage)?;
        let message = BASE64
            .decode(text(request, "message"))
            .ok()
            .and_then(|sealed| keys.crypto.decrypt(&nonce.into(), &sealed[..]).ok())
            .and_then(|plain| serde_json::from_slice::<Map<String, Value>>(&plain).ok())
            .ok_or(ProtocolError::CannotDecryptMessage)?;
        let request = Request {
            action: text(request, "action").to_owned(),
            nonce,
            request_id: request.get("requestID").cloned(),
        };
        Ok((request, message))
    }

    /// A successful reply, its `message` encrypted under the incremented nonce. The plain
    /// message, which may hold passwords, is wiped once encrypted.
    fn encrypted_reply(&self, request: &Request, fields: Value) -> String {
        let keys = self
            .keys
            .as_ref()
            .expect("only decrypted requests get encrypted replies");
        let nonce = increment(request.nonce);
        let mut message = success(nonce);
        message.insert("action".into(), request.action.clone().into());
        if let Value::Object(fields) = fields {
            message.extend(fields);
        }
        let mut message = Value::Object(message);
        let mut plain = message.to_string();
        let sealed = keys
            .crypto
            .encrypt(&nonce.into(), plain.as_bytes())
            .expect("encrypting in memory cannot fail");
        plain.zeroize();
        wipe(&mut message);
        let mut reply = Map::new();
        reply.insert("action".into(), request.action.clone().into());
        reply.insert("message".into(), BASE64.encode(sealed).into());
        reply.insert("nonce".into(), BASE64.encode(nonce).into());
        with_request_id(&mut reply, request.request_id.clone());
        Value::Object(reply).to_string()
    }

    fn error(&self, request: &Request, error: ProtocolError) -> String {
        error_reply(&request.action, request.request_id.clone(), error)
    }
}

/// Every Entry offered to the site in the open Vaults the extension is paired with, each with
/// its remembered Access decision unless that is to deny the site.
fn candidates<'a>(workspace: &'a Workspace, query: &SiteQuery) -> Vec<Candidate<'a>> {
    let paired = |open: &&crate::OpenVault| {
        query
            .keys
            .iter()
            .any(|(name, key)| open.vault.is_paired_with(name, key))
    };
    let hosts = query.hosts();
    workspace
        .vaults
        .iter()
        .filter(paired)
        .flat_map(|open| {
            open.vault
                .entries_for_site(&query.site)
                .into_iter()
                .map(move |found| (open.id, found))
        })
        .filter_map(|(vault, found)| {
            let decision = found.entry.access_decision(&hosts);
            (decision != Some(AccessDecision::Deny)).then_some(Candidate {
                vault,
                found,
                decision,
            })
        })
        .collect()
}

/// Wipes every string in a JSON value, best effort, before it is dropped.
fn wipe(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe),
        Value::Object(fields) => fields.values_mut().for_each(wipe),
        _ => {}
    }
}

/// A native host connected to keefast: one browser extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingId(u64);

/// A Pairing request waiting for the user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingPairing {
    pub id: PairingId,
    /// The Vault the extension would be paired with.
    pub vault: VaultId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessId(u64);

/// A site asking for Entries, waiting for the user's Access decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAccess {
    pub id: AccessId,
    /// The host of the site asking.
    pub site: String,
    /// The Entries to decide on.
    pub entries: Vec<(VaultId, EntryId)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginId(u64);

/// A captured login waiting for the user to say where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLogin {
    pub id: LoginId,
    /// The host of the site the login was submitted on.
    pub site: String,
    pub username: String,
    /// The open Vaults the login may go to: those the extension is paired with.
    pub vaults: Vec<VaultId>,
    /// The Entries the login could update: the one the extension picked, and those for the
    /// site with the same username.
    pub entries: Vec<(VaultId, EntryId)>,
    /// Where to offer to save it: updating the first of `entries`, or else a new Entry in the
    /// Browser group of the Vault last saved to, or of the Vault the browser deals with.
    pub suggested: LoginTarget,
}

/// A reply for one extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub connection: ConnectionId,
    pub message: String,
}

/// Every extension's session, and the Pairing and Access requests waiting for the user, oldest
/// first.
#[derive(Default)]
pub(crate) struct BrowserConnections {
    sessions: HashMap<ConnectionId, BrowserSession>,
    pairings: VecDeque<(PairingId, ConnectionId, PairingRequest)>,
    next_pairing: u64,
    accesses: VecDeque<(AccessId, ConnectionId, AccessRequest)>,
    next_access: u64,
    logins: VecDeque<(LoginId, ConnectionId, LoginRequest)>,
    next_login: u64,
}

impl Workspace {
    /// Handles a request from the extension on `connection`, and returns the reply to send
    /// now, if any. While browser integration is off, every request is answered that keefast
    /// is not running.
    pub fn handle_browser_request(
        &mut self,
        connection: ConnectionId,
        request: &str,
    ) -> Option<String> {
        if !self.browser_integration() {
            return Some(not_running_reply(request));
        }
        let mut session = self
            .browser
            .sessions
            .remove(&connection)
            .unwrap_or_default();
        let reply = session.handle(self, request);
        self.browser.sessions.insert(connection, session);
        match reply {
            BrowserReply::Send(reply) => Some(reply),
            BrowserReply::AskPairing(request) => {
                let id = PairingId(self.browser.next_pairing);
                self.browser.next_pairing += 1;
                self.browser.pairings.push_back((id, connection, request));
                None
            }
            BrowserReply::AskAccess(request) => {
                let id = AccessId(self.browser.next_access);
                self.browser.next_access += 1;
                self.browser.accesses.push_back((id, connection, request));
                None
            }
            BrowserReply::AskLogin(request) => {
                let id = LoginId(self.browser.next_login);
                self.browser.next_login += 1;
                self.browser.logins.push_back((id, connection, request));
                None
            }
        }
    }

    /// Forgets an extension that has gone, and its Pairing, Access and login requests.
    pub fn disconnect_browser(&mut self, connection: ConnectionId) {
        self.browser.sessions.remove(&connection);
        self.browser
            .pairings
            .retain(|(_, from, _)| *from != connection);
        self.browser
            .accesses
            .retain(|(_, from, _)| *from != connection);
        self.browser
            .logins
            .retain(|(_, from, _)| *from != connection);
    }

    /// The site asking for Entries to put to the user now.
    pub fn pending_access(&self) -> Option<PendingAccess> {
        self.browser
            .accesses
            .front()
            .map(|(id, _, request)| PendingAccess {
                id: *id,
                site: request.site().to_owned(),
                entries: request.entries().to_vec(),
            })
    }

    /// Answers the pending Access request `id` with the user's decision, and returns the
    /// replies to send: its own, and those of other waiting requests the decision settled.
    pub fn answer_access(&mut self, id: AccessId, answer: AccessAnswer) -> Vec<Outgoing> {
        let Some((pending, ..)) = self.browser.accesses.front() else {
            return Vec::new();
        };
        if *pending != id {
            return Vec::new();
        }
        let (_, connection, request) = self.browser.accesses.pop_front().expect("pending");
        let mut replies = Vec::new();
        if let Some(mut session) = self.browser.sessions.remove(&connection) {
            let message = session.answer_access(self, &request, answer);
            self.browser.sessions.insert(connection, session);
            replies.push(Outgoing {
                connection,
                message,
            });
        }
        replies.extend(self.answer_settled_accesses());
        replies
    }

    /// Answers the waiting Access requests none of whose Entries need a decision any more,
    /// now that one was remembered, and returns their replies.
    fn answer_settled_accesses(&mut self) -> Vec<Outgoing> {
        let mut replies = Vec::new();
        let mut index = 0;
        while let Some((_, connection, request)) = self.browser.accesses.get(index) {
            let still_undecided = candidates(self, &request.query).iter().any(|candidate| {
                candidate.decision.is_none() && request.undecided.contains(&candidate.id())
            });
            if still_undecided {
                index += 1;
                continue;
            }
            if let Some(mut session) = self.browser.sessions.remove(connection) {
                let message =
                    session.entries_reply(self, &request.request, &request.query, |_, _| None);
                self.browser.sessions.insert(*connection, session);
                replies.push(Outgoing {
                    connection: *connection,
                    message,
                });
            }
            self.browser.accesses.remove(index);
        }
        replies
    }

    /// The captured login to put to the user now.
    pub fn pending_login(&self) -> Option<PendingLogin> {
        let (id, _, request) = self.browser.logins.front()?;
        let vaults = self.login_vaults(request);
        let entries = self.login_entries(request, &vaults);
        let suggested = match entries.first() {
            Some(&(vault, entry)) => LoginTarget::Update { vault, entry },
            None => {
                let last = self
                    .config
                    .last_login_vault
                    .as_ref()
                    .and_then(|location| self.open_vault_at(location));
                let browser = self.browser_vault(&[]);
                let vault = [last, browser]
                    .into_iter()
                    .flatten()
                    .find(|vault| vaults.contains(vault))
                    .or_else(|| vaults.first().copied())?;
                LoginTarget::NewEntry {
                    vault,
                    group: self.browser_group(vault),
                }
            }
        };
        Some(PendingLogin {
            id: *id,
            site: request.login.site_name(),
            username: request.login.username.clone(),
            vaults,
            entries,
            suggested,
        })
    }

    /// Saves the pending login `id` where the user chose, remembers the Vault for next time,
    /// and returns the reply for its extension. A target the login can't go to is refused,
    /// and the login stays pending.
    pub fn save_login(
        &mut self,
        id: LoginId,
        target: LoginTarget,
    ) -> Result<Option<Outgoing>, EditVaultError> {
        let Some((pending, connection, request)) = self.browser.logins.front() else {
            return Ok(None);
        };
        if *pending != id {
            return Ok(None);
        }
        let (connection, request) = (*connection, request.clone());
        let vault = match target {
            LoginTarget::NewEntry { vault, .. } | LoginTarget::Update { vault, .. } => vault,
        };
        if !self.login_vaults(&request).contains(&vault) {
            return Err(EditVaultError::NotPaired);
        }
        let login = &request.login;
        match target {
            LoginTarget::NewEntry { group, .. } => {
                self.edit(vault, |open, now| open.add_login(group, login, now))?;
            }
            LoginTarget::Update { entry, .. } => {
                self.edit(vault, |open, now| open.update_login(entry, login, now))?;
            }
        }
        self.browser.logins.pop_front();
        self.config.last_login_vault = self.vault(vault).map(|open| open.location.clone());
        self.save_config();
        let message = self
            .browser
            .sessions
            .get(&connection)
            .map(|session| session.login_saved_reply(self, &request, vault));
        Ok(message.map(|message| Outgoing {
            connection,
            message,
        }))
    }

    /// Turns the pending login `id` down, and returns the reply for its extension.
    pub fn cancel_login(&mut self, id: LoginId) -> Option<Outgoing> {
        let (pending, ..) = self.browser.logins.front()?;
        if *pending != id {
            return None;
        }
        let (_, connection, request) = self.browser.logins.pop_front()?;
        let session = self.browser.sessions.get(&connection)?;
        Some(Outgoing {
            connection,
            message: session.error(&request.request, ProtocolError::ActionCancelledOrDenied),
        })
    }

    /// The open Vaults a captured login may go to: those paired with a Pairing the extension
    /// proved.
    fn login_vaults(&self, request: &LoginRequest) -> Vec<VaultId> {
        self.vaults
            .iter()
            .filter(|open| {
                request
                    .pairings
                    .iter()
                    .any(|(name, key)| open.vault.is_paired_with(name, key))
            })
            .map(|open| open.id)
            .collect()
    }

    /// The Entries in `vaults` a captured login could update, among those offered to its site:
    /// the one the extension picked, then those with the login's username.
    fn login_entries(&self, request: &LoginRequest, vaults: &[VaultId]) -> Vec<(VaultId, EntryId)> {
        let Some(site) = Site::parse(&request.login.url) else {
            return Vec::new();
        };
        let offered: Vec<(VaultId, &keefast_core::Entry)> = vaults
            .iter()
            .filter_map(|&vault| self.vault(vault))
            .flat_map(|open| {
                let found = open.vault.entries_for_site(&site);
                found.into_iter().map(move |found| (open.id, found.entry))
            })
            .collect();
        let picked = offered
            .iter()
            .filter(|(_, entry)| Some(entry.id()) == request.picked);
        let same_username = offered
            .iter()
            .filter(|(_, entry)| entry.username() == request.login.username);
        let mut entries = Vec::new();
        for &(vault, entry) in picked.chain(same_username) {
            if !entries.contains(&(vault, entry.id())) {
                entries.push((vault, entry.id()));
            }
        }
        entries
    }

    /// The Group captured logins are saved to in the Vault, if it has one yet.
    pub fn browser_group(&self, vault: VaultId) -> Option<GroupId> {
        self.vault(vault)?.vault.browser_group()
    }

    /// Makes `group` the Vault's Browser group.
    pub fn set_browser_group(
        &mut self,
        vault: VaultId,
        group: GroupId,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |open, _| open.set_browser_group(group))
    }

    /// The Pairing request to put to the user now.
    pub fn pending_pairing(&self) -> Option<PendingPairing> {
        self.browser
            .pairings
            .front()
            .map(|(id, _, request)| PendingPairing {
                id: *id,
                vault: request.vault(),
            })
    }

    /// Pairs the extension behind the pending request `id` with its Vault, as `name`, and
    /// returns the reply for it. A name the Vault can't take is refused, and the request stays
    /// pending.
    pub fn approve_pairing(
        &mut self,
        id: PairingId,
        name: &str,
    ) -> Result<Option<Outgoing>, EditVaultError> {
        let Some((connection, request)) = self.pending(id) else {
            return Ok(None);
        };
        let session = self
            .browser
            .sessions
            .remove(&connection)
            .unwrap_or_default();
        let reply = session.approve_pairing(self, &request, name);
        self.browser.sessions.insert(connection, session);
        let message = reply?;
        self.browser.pairings.pop_front();
        Ok(Some(Outgoing {
            connection,
            message,
        }))
    }

    /// Turns the pending request `id` down, and returns the reply for its extension.
    pub fn deny_pairing(&mut self, id: PairingId) -> Option<Outgoing> {
        let (connection, request) = self.pending(id)?;
        self.browser.pairings.pop_front();
        let message = self
            .browser
            .sessions
            .get(&connection)
            .map(|session| session.deny_pairing(&request))?;
        Some(Outgoing {
            connection,
            message,
        })
    }

    /// The pending request `id`, if it is the one put to the user now.
    fn pending(&self, id: PairingId) -> Option<(ConnectionId, PairingRequest)> {
        let (pending, connection, request) = self.browser.pairings.front()?;
        (*pending == id).then(|| (*connection, request.clone()))
    }

    /// Ends every session and turns every pending Pairing, Access and login request down, for
    /// browser integration turned off.
    pub(crate) fn end_browser_sessions(&mut self) -> Vec<Outgoing> {
        let browser = std::mem::take(&mut self.browser);
        let pairings = browser
            .pairings
            .into_iter()
            .map(|(_, connection, request)| (connection, request.request));
        let accesses = browser
            .accesses
            .into_iter()
            .map(|(_, connection, request)| (connection, request.request));
        let logins = browser
            .logins
            .into_iter()
            .map(|(_, connection, request)| (connection, request.request));
        pairings
            .chain(accesses)
            .chain(logins)
            .filter_map(|(connection, request)| {
                let session = browser.sessions.get(&connection)?;
                Some(Outgoing {
                    connection,
                    message: session.error(&request, ProtocolError::ActionCancelledOrDenied),
                })
            })
            .collect()
    }

    /// The Vault the browser deals with, as KeePassXC's active database: the focused Vault, or
    /// else the first open Vault paired with one of `connected_ids`, or else the first open.
    fn browser_vault(&self, connected_ids: &[String]) -> Option<VaultId> {
        let focused = self.focused_vault.filter(|&id| self.vault(id).is_some());
        let paired = || {
            self.vaults.iter().find(|open| {
                open.vault
                    .pairings()
                    .iter()
                    .any(|pairing| connected_ids.contains(&pairing.name))
            })
        };
        focused
            .or_else(|| paired().map(|open| open.id))
            .or_else(|| self.vaults.first().map(|open| open.id))
    }

    /// An open Vault paired with the extension known as `id` by `key`, preferring the Vault
    /// the browser deals with.
    fn paired_vault(&self, id: &str, key: &str, connected_ids: &[String]) -> Option<VaultId> {
        let paired = |vault: VaultId| {
            self.vault(vault)
                .is_some_and(|open| open.vault.is_paired_with(id, key))
        };
        self.browser_vault(connected_ids)
            .filter(|&vault| paired(vault))
            .or_else(|| {
                self.vaults
                    .iter()
                    .map(|open| open.id)
                    .find(|&vault| paired(vault))
            })
    }

    /// Stores a Pairing in a Vault: the extension known as `name` by `key` may ask it for
    /// credentials from now on.
    pub fn pair_browser(
        &mut self,
        vault: VaultId,
        name: &str,
        key: &str,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.add_pairing(name, key, now))
    }
}

fn browser_hash(workspace: &Workspace, vault: VaultId) -> String {
    workspace
        .vault(vault)
        .expect("chosen among open Vaults")
        .vault()
        .browser_hash()
}

fn success(nonce: [u8; NONCE_SIZE]) -> Map<String, Value> {
    let mut reply = Map::new();
    reply.insert("version".into(), KEEPASSXC_VERSION.into());
    reply.insert("success".into(), "true".into());
    reply.insert("nonce".into(), BASE64.encode(nonce).into());
    reply
}

fn error_reply(action: &str, request_id: Option<Value>, error: ProtocolError) -> String {
    let mut reply = Map::new();
    reply.insert("action".into(), action.into());
    reply.insert("errorCode".into(), (error as u8).to_string().into());
    reply.insert("error".into(), error.message().into());
    with_request_id(&mut reply, request_id);
    Value::Object(reply).to_string()
}

/// Echoes the request's `requestID`, which the extension matches replies by, if it sent one.
fn with_request_id(reply: &mut Map<String, Value>, request_id: Option<Value>) {
    if let Some(id) = request_id {
        reply.insert("requestID".into(), id);
    }
}

/// The reply to a request keefast won't answer because browser integration is off.
pub(crate) fn not_running_reply(request: &str) -> String {
    let request: Map<String, Value> = serde_json::from_str(request).unwrap_or_default();
    error_reply(
        text(&request, "action"),
        request.get("requestID").cloned(),
        ProtocolError::NotRunning,
    )
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> &'a str {
    object.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    BASE64.decode(text).ok()?.try_into().ok()
}

/// The nonce plus one, little-endian, as libsodium's `sodium_increment`: replies use the
/// request's nonce incremented.
fn increment(mut nonce: [u8; NONCE_SIZE]) -> [u8; NONCE_SIZE] {
    for byte in &mut nonce {
        let (next, carry) = byte.overflowing_add(1);
        *byte = next;
        if !carry {
            break;
        }
    }
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonces_increment_little_endian_with_carry() {
        let mut nonce = [0; NONCE_SIZE];
        nonce[0] = 0xff;
        nonce[1] = 0x01;
        let next = increment(nonce);
        assert_eq!(next[..3], [0x00, 0x02, 0x00]);

        let all = increment([0xff; NONCE_SIZE]);
        assert_eq!(all, [0; NONCE_SIZE]);
    }
}
