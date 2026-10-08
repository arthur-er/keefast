//! The browser request handler: the KeePassXC-Browser protocol (ADR 0003) spoken with one
//! browser extension, on top of the Workspace.
//!
//! Each connection from the native host gets a [`BrowserSession`], which holds that
//! connection's encryption keys. Requests are JSON text; replies are JSON text to send back.
//! Pairing needs the user's approval, so an `associate` request is handed back as a
//! [`PairingRequest`] to answer later with [`BrowserSession::approve_pairing`] or
//! [`BrowserSession::deny_pairing`].

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use crypto_box::aead::{Aead, OsRng};
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use serde_json::{Map, Value, json};

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
    ActionCancelledOrDenied = 6,
    AssociationFailed = 8,
    KeyChangeFailed = 9,
    IncorrectAction = 12,
    EmptyMessageReceived = 13,
}

impl ProtocolError {
    fn message(self) -> &'static str {
        match self {
            ProtocolError::DatabaseNotOpened => "keefast is locked: unlock a Vault in keefast",
            ProtocolError::DatabaseHashNotReceived => "Database hash not available",
            ProtocolError::ClientPublicKeyNotReceived => "Client public key not received",
            ProtocolError::CannotDecryptMessage => "Cannot decrypt message",
            ProtocolError::ActionCancelledOrDenied => "Action cancelled or denied",
            ProtocolError::AssociationFailed => "Association failed",
            ProtocolError::KeyChangeFailed => "Key change was not successful",
            ProtocolError::IncorrectAction => "Incorrect action",
            ProtocolError::EmptyMessageReceived => "Empty message received",
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
        let mut reply = success(increment(nonce));
        reply.insert("action".into(), "change-public-keys".into());
        reply.insert("publicKey".into(), host_key.into());
        if let Some(id) = request.get("requestID") {
            reply.insert("requestID".into(), id.clone());
        }
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
        let vault = workspace
            .paired_vault(id, key, &self.connected_ids)
            .ok_or(ProtocolError::AssociationFailed)?;
        Ok(BrowserReply::Send(self.encrypted_reply(
            &request,
            json!({ "hash": browser_hash(workspace, vault), "id": id }),
        )))
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

    /// A successful reply, its `message` encrypted under the incremented nonce.
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
        let plain = Value::Object(message).to_string();
        let sealed = keys
            .crypto
            .encrypt(&nonce.into(), plain.as_bytes())
            .expect("encrypting in memory cannot fail");
        let mut reply = Map::new();
        reply.insert("action".into(), request.action.clone().into());
        reply.insert("message".into(), BASE64.encode(sealed).into());
        reply.insert("nonce".into(), BASE64.encode(nonce).into());
        if let Some(id) = &request.request_id {
            reply.insert("requestID".into(), id.clone());
        }
        Value::Object(reply).to_string()
    }

    fn error(&self, request: &Request, error: ProtocolError) -> String {
        error_reply(&request.action, request.request_id.clone(), error)
    }
}

impl Workspace {
    /// Tells the Workspace which Vault the user is looking at, if one, so the browser pairs
    /// with that Vault.
    pub fn focus_vault(&mut self, vault: Option<VaultId>) {
        self.focused_vault = vault;
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
        if id.is_empty() || key.is_empty() {
            return None;
        }
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
    if let Some(id) = request_id {
        reply.insert("requestID".into(), id);
    }
    Value::Object(reply).to_string()
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
