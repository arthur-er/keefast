//! A fake KeePassXC-Browser extension: it builds requests the way the stock extension does
//! (NaCl box under its session key) and opens the replies.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use crypto_box::aead::{Aead, AeadCore, OsRng};
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use serde_json::{Value, json};

pub struct FakeExtension {
    secret: SecretKey,
    /// The identification key pair it pairs with.
    id_secret: SecretKey,
    host: Option<SalsaBox>,
    pub client_id: String,
}

/// A request sent, with the nonce its reply must carry.
pub struct Sent {
    pub json: String,
    pub expected_nonce: String,
}

impl FakeExtension {
    pub fn new() -> Self {
        Self {
            secret: SecretKey::generate(&mut OsRng),
            id_secret: SecretKey::generate(&mut OsRng),
            host: None,
            client_id: "client-1".into(),
        }
    }

    pub fn public_key(&self) -> String {
        BASE64.encode(self.secret.public_key().as_bytes())
    }

    pub fn id_key(&self) -> String {
        BASE64.encode(self.id_secret.public_key().as_bytes())
    }

    pub fn change_public_keys(&self) -> Sent {
        let nonce = SalsaBox::generate_nonce(&mut OsRng);
        Sent {
            json: json!({
                "action": "change-public-keys",
                "publicKey": self.public_key(),
                "nonce": BASE64.encode(nonce),
                "clientID": self.client_id,
                "requestID": "r1",
            })
            .to_string(),
            expected_nonce: BASE64.encode(increment(nonce.into())),
        }
    }

    /// Takes the host's public key from its `change-public-keys` reply.
    pub fn accept_keys(&mut self, reply: &Value) {
        let key = BASE64.decode(reply["publicKey"].as_str().unwrap()).unwrap();
        let key = PublicKey::from(<[u8; 32]>::try_from(key).unwrap());
        self.host = Some(SalsaBox::new(&key, &self.secret));
    }

    /// An encrypted request for `action`, with `message` as its payload.
    pub fn encrypted(&self, action: &str, message: Value) -> Sent {
        let nonce = SalsaBox::generate_nonce(&mut OsRng);
        let sealed = self
            .host
            .as_ref()
            .expect("keys exchanged")
            .encrypt(&nonce, message.to_string().as_bytes())
            .unwrap();
        Sent {
            json: json!({
                "action": action,
                "message": BASE64.encode(sealed),
                "nonce": BASE64.encode(nonce),
                "clientID": self.client_id,
                "triggerUnlock": "false",
            })
            .to_string(),
            expected_nonce: BASE64.encode(increment(nonce.into())),
        }
    }

    pub fn get_database_hash(&self, connected: &[&str]) -> Sent {
        self.encrypted(
            "get-databasehash",
            json!({ "action": "get-databasehash", "connectedKeys": connected }),
        )
    }

    pub fn associate(&self) -> Sent {
        self.encrypted(
            "associate",
            json!({ "action": "associate", "key": self.public_key(), "idKey": self.id_key() }),
        )
    }

    pub fn test_associate(&self, id: &str, key: &str) -> Sent {
        self.encrypted(
            "test-associate",
            json!({ "action": "test-associate", "id": id, "key": key }),
        )
    }

    /// Asks for the logins for the page at `url`, its form submitting to `submit_url`, proving
    /// the Pairings in `keys` (name and key).
    pub fn get_logins(&self, url: &str, submit_url: Option<&str>, keys: &[(&str, &str)]) -> Sent {
        let keys: Vec<Value> = keys
            .iter()
            .map(|(id, key)| json!({ "id": id, "key": key }))
            .collect();
        let mut message = json!({ "action": "get-logins", "id": "", "url": url, "keys": keys });
        if let Some(submit_url) = submit_url {
            message["submitUrl"] = submit_url.into();
        }
        self.encrypted("get-logins", message)
    }

    /// Submits a login captured on the page at `url`, as the stock extension does: `uuid` is
    /// the Entry the user picked to update, if one, and `group` the Group it would like a new
    /// Entry in (name and UUID), which keefast ignores.
    pub fn set_login(
        &self,
        url: &str,
        login: &str,
        password: &str,
        uuid: Option<&str>,
        group: Option<(&str, &str)>,
    ) -> Sent {
        let mut message = json!({
            "action": "set-login",
            "id": "Home",
            "login": login,
            "password": password,
            "url": url,
            "submitUrl": url,
        });
        if let Some(uuid) = uuid {
            message["uuid"] = uuid.into();
        }
        if let Some((name, uuid)) = group {
            message["group"] = name.into();
            message["groupUuid"] = uuid.into();
        }
        self.encrypted("set-login", message)
    }

    /// Searches the paired Vaults for `query` from the command palette on the page at `url`,
    /// proving the Pairings in `keys` (name and key).
    pub fn search_entries(&self, query: &str, url: &str, keys: &[(&str, &str)]) -> Sent {
        self.encrypted(
            "keefast-search-entries",
            json!({
                "action": "keefast-search-entries",
                "query": query,
                "url": url,
                "submitUrl": url,
                "keys": key_list(keys),
            }),
        )
    }

    /// Picks the Entry `uuid` in the command palette for the page at `url`, asking keefast to
    /// add the site to its URLs if `add_url`.
    pub fn pick_entry(&self, uuid: &str, url: &str, keys: &[(&str, &str)], add_url: bool) -> Sent {
        self.encrypted(
            "keefast-pick-entry",
            json!({
                "action": "keefast-pick-entry",
                "id": "",
                "uuid": uuid,
                "url": url,
                "submitUrl": url,
                "keys": key_list(keys),
                "addUrl": add_url,
            }),
        )
    }

    /// Searches the paired Vaults from the command palette for Entries that fill a `form`, such
    /// as `payment`, on the page at `url`.
    pub fn search_form(&self, form: &str, query: &str, url: &str, keys: &[(&str, &str)]) -> Sent {
        self.encrypted(
            "keefast-search-entries",
            json!({
                "action": "keefast-search-entries",
                "query": query,
                "form": form,
                "url": url,
                "submitUrl": url,
                "keys": key_list(keys),
            }),
        )
    }

    /// Picks the Entry `uuid` in the command palette to fill a `form` on the page at `url`,
    /// the form being in a frame at `frame_url` if it is given.
    pub fn fill_form(
        &self,
        form: &str,
        uuid: &str,
        url: &str,
        frame_url: Option<&str>,
        keys: &[(&str, &str)],
    ) -> Sent {
        self.encrypted(
            "keefast-fill-form",
            json!({
                "action": "keefast-fill-form",
                "id": "",
                "form": form,
                "uuid": uuid,
                "url": url,
                "submitUrl": url,
                "frameUrl": frame_url.unwrap_or(url),
                "keys": key_list(keys),
            }),
        )
    }

    /// Opens an encrypted reply, checking it answers `sent`, and returns its message.
    pub fn open(&self, sent: &Sent, reply: &str) -> Value {
        let reply: Value = serde_json::from_str(reply).unwrap();
        assert!(reply.get("errorCode").is_none(), "error reply: {reply}");
        assert_eq!(reply["nonce"], sent.expected_nonce);
        let nonce = BASE64.decode(reply["nonce"].as_str().unwrap()).unwrap();
        let sealed = BASE64.decode(reply["message"].as_str().unwrap()).unwrap();
        let plain = self
            .host
            .as_ref()
            .unwrap()
            .decrypt(nonce.as_slice().into(), &sealed[..])
            .expect("the reply decrypts with the session keys");
        let message: Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(message["success"], "true");
        assert_eq!(message["nonce"], sent.expected_nonce);
        message
    }
}

fn key_list(keys: &[(&str, &str)]) -> Vec<Value> {
    keys.iter()
        .map(|(id, key)| json!({ "id": id, "key": key }))
        .collect()
}

fn increment(mut nonce: [u8; 24]) -> [u8; 24] {
    for byte in &mut nonce {
        let (next, carry) = byte.overflowing_add(1);
        *byte = next;
        if !carry {
            break;
        }
    }
    nonce
}
