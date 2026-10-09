//! TOTP codes and generated passwords over the KeePassXC-Browser protocol: `get-totp` and
//! `generate-password` driven with real NaCl-encrypted messages from a fake extension through
//! a Workspace.
//!
//! The Vault is KeePassXC's browser fixture: for https://example.com it offers "Example",
//! "Remembered" (already allowed example.com) and "Shown again"; "Other site" is for
//! https://other.test and "Hidden" is hidden from the browser.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{
    AccessDecision, CharacterRecipe, DefaultAccessPolicy, EntryId, PassphraseRecipe, Totp,
    TotpAlgorithm,
};
use keefast_workspace::{
    AccessAnswer, ConnectionId, GeneratorMode, GeneratorSettings, UnlockSecrets, VaultId, Workspace,
};
use serde_json::{Value, json};
use support::extension::{FakeExtension, Sent};
use support::{BROWSER_KDBX, FakeClock, FakeStorage, PASSWORD, personal_vault};

const FIRST: ConnectionId = ConnectionId(1);
const SECOND: ConnectionId = ConnectionId(2);
const EXAMPLE: &str = "https://example.com/login";

/// RFC 6238's SHA-1 test secret, Base32-encoded.
const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
/// One of RFC 6238's test times, whose SHA-1 code is 07081804, so 081804 in 6 digits.
const RFC_TIME: u64 = 1_111_111_109;
const RFC_CODE: &str = "081804";

const ALLOW: AccessAnswer = AccessAnswer {
    decision: AccessDecision::Allow,
    remember: false,
};
const DENY: AccessAnswer = AccessAnswer {
    decision: AccessDecision::Deny,
    remember: false,
};

fn parse(reply: &str) -> Value {
    serde_json::from_str(reply).unwrap()
}

/// An extension paired as "Home" with the personal Vault, in a Workspace with browser
/// integration on, its clock at RFC 6238's test time.
struct Browser {
    workspace: Workspace,
    clock: Arc<FakeClock>,
    vault: VaultId,
    extension: FakeExtension,
    id_key: String,
}

impl Browser {
    fn paired() -> Self {
        let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), BROWSER_KDBX));
        let clock = Arc::new(FakeClock::at_unix_seconds(RFC_TIME));
        let mut workspace = Workspace::with_clock(storage, clock.clone());
        let vault = workspace
            .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
            .unwrap();
        workspace.set_browser_integration(true);
        let extension = connect(&mut workspace, FIRST);
        workspace.handle_browser_request(FIRST, &extension.associate().json);
        let pairing = workspace.pending_pairing().unwrap();
        workspace.approve_pairing(pairing.id, "Home").unwrap();
        let id_key = extension.id_key();
        Self {
            workspace,
            clock,
            vault,
            extension,
            id_key,
        }
    }

    fn entry(&self, title: &str) -> EntryId {
        self.workspace
            .vault(self.vault)
            .unwrap()
            .vault()
            .root()
            .all_entries()
            .find(|entry| entry.title() == title)
            .unwrap_or_else(|| panic!("no Entry {title}"))
            .id()
    }

    /// Sets up RFC 6238's test TOTP on the Entry.
    fn with_totp(&mut self, title: &str) -> EntryId {
        let entry = self.entry(title);
        let totp = Totp::new(SECRET, TotpAlgorithm::Sha1, 6, 30).unwrap();
        self.workspace
            .set_totp(self.vault, entry, Some(&totp))
            .unwrap();
        entry
    }

    /// Asks for the logins of `url` and answers the Access prompt, if any, with `answer`.
    /// Returns the Entries given to the extension.
    fn get_logins(&mut self, url: &str, answer: AccessAnswer) -> Vec<Value> {
        let sent = self
            .extension
            .get_logins(url, None, &[("Home", &self.id_key)]);
        let reply = match self.workspace.handle_browser_request(FIRST, &sent.json) {
            Some(reply) => reply,
            None => {
                let pending = self.workspace.pending_access().unwrap();
                let mut replies = self.workspace.answer_access(pending.id, answer);
                replies.remove(0).message
            }
        };
        self.extension.open(&sent, &reply)["entries"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn get_totp(&mut self, uuid: &str) -> Value {
        let sent = get_totp_request(&self.extension, uuid);
        let reply = self
            .workspace
            .handle_browser_request(FIRST, &sent.json)
            .expect("answered at once");
        self.extension.open(&sent, &reply)
    }

    fn generate_password(&mut self) -> Value {
        let sent = generate_password_request(&self.extension);
        let reply = self
            .workspace
            .handle_browser_request(FIRST, &sent.json)
            .expect("answered at once");
        self.extension.open(&sent, &reply)
    }
}

/// A new extension exchanging keys with keefast over `connection`.
fn connect(workspace: &mut Workspace, connection: ConnectionId) -> FakeExtension {
    let mut extension = FakeExtension::new();
    let keys = workspace
        .handle_browser_request(connection, &extension.change_public_keys().json)
        .unwrap();
    extension.accept_keys(&parse(&keys));
    extension
}

/// A `get-totp` request as the stock extension sends it.
fn get_totp_request(extension: &FakeExtension, uuid: &str) -> Sent {
    extension.encrypted("get-totp", json!({ "action": "get-totp", "uuid": uuid }))
}

/// A `generate-password` request as the stock extension sends it: its encrypted message
/// repeats the request's fields, and its `requestID` is echoed outside it too.
fn generate_password_request(extension: &FakeExtension) -> Sent {
    let mut sent = extension.encrypted(
        "generate-password",
        json!({
            "action": "generate-password",
            "nonce": "",
            "clientID": extension.client_id,
            "requestID": "gen-1",
        }),
    );
    let mut request = parse(&sent.json);
    request["requestID"] = "gen-1".into();
    sent.json = request.to_string();
    sent
}

#[test]
fn allowed_entries_with_totp_carry_their_current_code() {
    let mut browser = Browser::paired();
    browser.with_totp("Example");

    let entries = browser.get_logins(EXAMPLE, ALLOW);

    let example = entries.iter().find(|entry| entry["name"] == "Example");
    assert_eq!(example.unwrap()["totp"], RFC_CODE);
    let mut others = entries.iter().filter(|entry| entry["name"] != "Example");
    assert!(others.clone().count() > 0);
    assert!(others.all(|entry| entry.get("totp").is_none()));
}

#[test]
fn get_totp_answers_the_current_code_of_an_entry_the_site_was_allowed() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    let message = browser.get_totp(&example.to_hex());

    assert_eq!(message["action"], "get-totp");
    assert_eq!(message["totp"], RFC_CODE);
}

#[test]
fn get_totp_follows_the_clock() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    browser.clock.advance(Duration::from_secs(30));
    let message = browser.get_totp(&example.to_hex());

    let next = browser.workspace.totp_code(browser.vault, example);
    let next = next.unwrap().unwrap();
    assert_ne!(next.code(), RFC_CODE);
    assert_eq!(message["totp"], next.code());
}

#[test]
fn get_totp_accepts_the_uuid_in_any_case() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    let message = browser.get_totp(&example.to_hex().to_uppercase());

    assert_eq!(message["totp"], RFC_CODE);
}

#[test]
fn get_totp_gives_no_code_for_an_entry_no_site_was_allowed() {
    let mut browser = Browser::paired();
    let other = browser.with_totp("Other site");
    browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    let message = browser.get_totp(&other.to_hex());

    assert_eq!(message["totp"], "");
}

#[test]
fn get_totp_gives_no_code_for_an_entry_the_site_was_denied() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    let entries = browser.get_logins(EXAMPLE, DENY);
    // Only "Remembered", allowed before, is handed over.
    assert!(entries.iter().all(|entry| entry["name"] != "Example"));

    let message = browser.get_totp(&example.to_hex());

    assert_eq!(message["totp"], "");
}

#[test]
fn get_totp_gives_no_code_once_the_site_is_denied_an_entry_allowed_before() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    browser.get_logins(EXAMPLE, DENY);
    let message = browser.get_totp(&example.to_hex());

    assert_eq!(message["totp"], "");
}

#[test]
fn get_totp_gives_no_code_after_the_extension_changes_keys() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    let mut extension = FakeExtension::new();
    let keys = browser
        .workspace
        .handle_browser_request(FIRST, &extension.change_public_keys().json)
        .unwrap();
    extension.accept_keys(&parse(&keys));
    let sent = get_totp_request(&extension, &example.to_hex());
    let reply = browser
        .workspace
        .handle_browser_request(FIRST, &sent.json)
        .unwrap();

    assert_eq!(extension.open(&sent, &reply)["totp"], "");
}

#[test]
fn get_totp_gives_no_code_for_an_entry_hidden_from_the_browser() {
    let mut browser = Browser::paired();
    let hidden = browser.with_totp("Hidden");
    browser.get_logins(EXAMPLE, ALLOW);

    let message = browser.get_totp(&hidden.to_hex());

    assert_eq!(message["totp"], "");
}

#[test]
fn get_totp_gives_no_code_before_any_site_asked_for_the_entry() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");

    let message = browser.get_totp(&example.to_hex());

    assert_eq!(message["totp"], "");
}

#[test]
fn get_totp_gives_no_code_once_the_entry_is_no_longer_offered_to_the_site() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    browser
        .workspace
        .delete_entry(browser.vault, example)
        .unwrap();
    let message = browser.get_totp(&example.to_hex());

    assert_eq!(message["totp"], "");
}

#[test]
fn get_totp_gives_no_code_to_another_extension_than_the_one_allowed() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser.get_logins(EXAMPLE, ALLOW);
    let stranger = connect(&mut browser.workspace, SECOND);

    let sent = get_totp_request(&stranger, &example.to_hex());
    let reply = browser
        .workspace
        .handle_browser_request(SECOND, &sent.json)
        .unwrap();

    assert_eq!(stranger.open(&sent, &reply)["totp"], "");
}

#[test]
fn get_totp_gives_no_code_once_the_allow_it_was_given_by_is_forgotten() {
    let mut browser = Browser::paired();
    let remembered = browser.with_totp("Remembered");
    // "Remembered" already allows example.com, so it is given without asking.
    browser.get_logins(EXAMPLE, DENY);
    assert_eq!(browser.get_totp(&remembered.to_hex())["totp"], RFC_CODE);

    browser
        .workspace
        .forget_access(browser.vault, remembered, None)
        .unwrap();

    assert_eq!(browser.get_totp(&remembered.to_hex())["totp"], "");
}

#[test]
fn get_totp_gives_no_code_once_the_default_access_policy_that_gave_it_changes() {
    let mut browser = Browser::paired();
    let example = browser.with_totp("Example");
    browser
        .workspace
        .set_default_access_policy(browser.vault, DefaultAccessPolicy::AlwaysAllow)
        .unwrap();
    browser.get_logins(EXAMPLE, DENY);
    assert_eq!(browser.get_totp(&example.to_hex())["totp"], RFC_CODE);

    browser
        .workspace
        .set_default_access_policy(browser.vault, DefaultAccessPolicy::Ask)
        .unwrap();

    assert_eq!(browser.get_totp(&example.to_hex())["totp"], "");
}

#[test]
fn get_totp_gives_no_code_for_an_entry_without_totp() {
    let mut browser = Browser::paired();
    let example = browser.entry("Example");
    browser.get_logins(EXAMPLE, ALLOW);

    let message = browser.get_totp(&example.to_hex());

    assert_eq!(message["totp"], "");
}

#[test]
fn get_totp_without_a_valid_uuid_is_an_error() {
    let mut browser = Browser::paired();
    for uuid in ["", "not-a-uuid", "0123456789abcdef0123456789abcdeg"] {
        let sent = get_totp_request(&browser.extension, uuid);
        let reply = browser
            .workspace
            .handle_browser_request(FIRST, &sent.json)
            .unwrap();

        assert_eq!(parse(&reply)["errorCode"], "18", "for {uuid:?}");
        assert_eq!(parse(&reply)["action"], "get-totp");
    }
}

#[test]
fn generate_password_uses_the_generator_settings() {
    let mut browser = Browser::paired();
    browser.workspace.set_generator_settings(GeneratorSettings {
        mode: GeneratorMode::Characters,
        characters: CharacterRecipe {
            length: 31,
            ..CharacterRecipe::default()
        },
        passphrase: PassphraseRecipe::default(),
    });

    let message = browser.generate_password();

    assert_eq!(message["action"], "generate-password");
    let password = message["password"].as_str().unwrap();
    assert_eq!(password.chars().count(), 31);
    assert_ne!(browser.generate_password()["password"], password);
}

#[test]
fn generate_password_makes_a_passphrase_in_passphrase_mode() {
    let mut browser = Browser::paired();
    let passphrase = PassphraseRecipe {
        words: 7,
        separator: "+".into(),
    };
    browser.workspace.set_generator_settings(GeneratorSettings {
        mode: GeneratorMode::Passphrase,
        characters: CharacterRecipe::default(),
        passphrase,
    });

    let message = browser.generate_password();

    let password = message["password"].as_str().unwrap();
    assert_eq!(password.split('+').count(), 7);
}

#[test]
fn generate_password_with_settings_that_cannot_generate_is_refused() {
    let mut browser = Browser::paired();
    browser.workspace.set_generator_settings(GeneratorSettings {
        mode: GeneratorMode::Characters,
        characters: CharacterRecipe {
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: false,
            ..CharacterRecipe::default()
        },
        passphrase: PassphraseRecipe::default(),
    });

    let sent = generate_password_request(&browser.extension);
    let reply = browser
        .workspace
        .handle_browser_request(FIRST, &sent.json)
        .unwrap();

    assert_eq!(parse(&reply)["errorCode"], "6");
    assert_eq!(parse(&reply)["requestID"], "gen-1");
}

#[test]
fn generate_password_echoes_the_request_id() {
    let mut browser = Browser::paired();
    let sent = generate_password_request(&browser.extension);

    let reply = browser
        .workspace
        .handle_browser_request(FIRST, &sent.json)
        .unwrap();

    assert_eq!(parse(&reply)["requestID"], "gen-1");
    assert!(browser.extension.open(&sent, &reply)["password"].is_string());
}
