//! Filling on demand over the KeePassXC-Browser protocol: `get-logins` driven with real
//! NaCl-encrypted messages from a fake extension through a Workspace, with Access decisions put
//! to the user and remembered on the Entries.
//!
//! The Vault is KeePassXC's browser fixture: for https://example.com it offers "Example",
//! "Remembered" (already allowed example.com, and denied evil.example.com) and "Shown again".

mod support;

use std::sync::Arc;

use keefast_core::AccessDecision;
use keefast_workspace::{
    AccessAnswer, ConnectionId, Credentials, Outgoing, PendingAccess, VaultId, Workspace,
};
use serde_json::Value;
use support::extension::{FakeExtension, Sent};
use support::{BROWSER_KDBX, FakeStorage, PASSWORD, personal_vault, work_vault};

const FIRST: ConnectionId = ConnectionId(1);
const SECOND: ConnectionId = ConnectionId(2);
const EXAMPLE: &str = "https://example.com/login";

const ALLOW: AccessAnswer = AccessAnswer {
    decision: AccessDecision::Allow,
    remember: false,
};
const REMEMBER_ALLOW: AccessAnswer = AccessAnswer {
    decision: AccessDecision::Allow,
    remember: true,
};
const DENY: AccessAnswer = AccessAnswer {
    decision: AccessDecision::Deny,
    remember: false,
};
const REMEMBER_DENY: AccessAnswer = AccessAnswer {
    decision: AccessDecision::Deny,
    remember: true,
};

fn storage() -> Arc<FakeStorage> {
    Arc::new(
        FakeStorage::default()
            .with_file(&personal_vault(), BROWSER_KDBX)
            .with_file(&work_vault(), BROWSER_KDBX),
    )
}

fn parse(reply: &str) -> Value {
    serde_json::from_str(reply).unwrap()
}

/// An extension connected over `connection` to a Workspace with browser integration on.
struct Browser {
    workspace: Workspace,
    extension: FakeExtension,
    /// The Pairing the extension holds, as name and key.
    pairing: (String, String),
}

impl Browser {
    /// Opens the personal Vault and pairs an extension with it as "Home".
    fn paired(storage: Arc<FakeStorage>) -> (Self, VaultId) {
        let mut workspace = Workspace::new(storage);
        let vault = workspace
            .open_vault(&personal_vault(), &Credentials::password(PASSWORD))
            .unwrap();
        workspace.set_browser_integration(true);
        let mut extension = FakeExtension::new();
        let keys = workspace
            .handle_browser_request(FIRST, &extension.change_public_keys().json)
            .unwrap();
        extension.accept_keys(&parse(&keys));
        workspace.handle_browser_request(FIRST, &extension.associate().json);
        let pairing = workspace.pending_pairing().unwrap();
        workspace.approve_pairing(pairing.id, "Home").unwrap();
        let pairing = ("Home".to_owned(), extension.id_key());
        (
            Self {
                workspace,
                extension,
                pairing,
            },
            vault,
        )
    }

    fn get_logins(&self, url: &str) -> Sent {
        self.get_logins_with(url, None, &[self.key()])
    }

    fn get_logins_with(&self, url: &str, submit: Option<&str>, keys: &[(&str, &str)]) -> Sent {
        self.extension.get_logins(url, submit, keys)
    }

    fn key(&self) -> (&str, &str) {
        (&self.pairing.0, &self.pairing.1)
    }

    fn request(&mut self, sent: &Sent) -> Option<String> {
        self.workspace.handle_browser_request(FIRST, &sent.json)
    }

    /// Sends the request and expects keefast to ask the user about it.
    #[track_caller]
    fn ask(&mut self, sent: &Sent) -> PendingAccess {
        assert_eq!(self.request(sent), None, "answered without asking");
        self.workspace.pending_access().expect("asks the user")
    }

    /// Sends the request and expects the titles of the Entries keefast answers with at once.
    #[track_caller]
    fn entries_now(&mut self, sent: &Sent) -> Vec<String> {
        let reply = self.request(sent).expect("answered without asking");
        titles(&self.extension.open(sent, &reply))
    }

    fn titles_of(&self, pending: &PendingAccess) -> Vec<String> {
        pending
            .entries
            .iter()
            .map(|&(vault, entry)| {
                self.workspace
                    .entry(vault, entry)
                    .unwrap()
                    .title()
                    .to_owned()
            })
            .collect()
    }

    fn answer(&mut self, pending: &PendingAccess, answer: AccessAnswer) -> Vec<Outgoing> {
        self.workspace.answer_access(pending.id, answer)
    }

    /// Answers the pending request and returns the titles of the Entries sent back for it.
    #[track_caller]
    fn answer_entries(&mut self, sent: &Sent, answer: AccessAnswer) -> Vec<String> {
        let pending = self.workspace.pending_access().unwrap();
        let replies = self.answer(&pending, answer);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].connection, FIRST);
        titles(&self.extension.open(sent, &replies[0].message))
    }
}

fn titles(message: &Value) -> Vec<String> {
    let entries = message["entries"].as_array().unwrap();
    assert_eq!(message["count"], entries.len());
    entries
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_first_access_asks_the_user_about_the_undecided_entries() {
    let (mut browser, vault) = Browser::paired(storage());
    let sent = browser.get_logins(EXAMPLE);

    let pending = browser.ask(&sent);

    assert_eq!(pending.site, "example.com");
    assert!(pending.entries.iter().all(|&(of, _)| of == vault));
    // "Remembered" already allows example.com, so only the others are asked about.
    assert_eq!(browser.titles_of(&pending), ["Example", "Shown again"]);
}

#[test]
fn allowed_entries_carry_what_the_extension_fills() {
    let (mut browser, vault) = Browser::paired(storage());
    let sent = browser.get_logins(EXAMPLE);
    browser.ask(&sent);

    let replies = browser.answer(&browser.workspace.pending_access().unwrap(), ALLOW);

    let message = browser.extension.open(&sent, &replies[0].message);
    assert_eq!(message["action"], "get-logins");
    assert_eq!(message["count"], 3);
    let example = &message["entries"][0];
    assert_eq!(example["name"], "Example");
    assert_eq!(example["login"], "alice");
    assert_eq!(example["password"], "example-secret");
    assert_eq!(example["group"], "Passwords");
    let open = browser.workspace.vault(vault).unwrap().vault();
    let id = open
        .root()
        .all_entries()
        .find(|entry| entry.title() == "Example")
        .unwrap()
        .id();
    assert_eq!(example["uuid"], id.to_hex());
    assert_eq!(message["entries"][2]["group"], "Shown again");
    assert_eq!(message["hash"], open.browser_hash());
}

#[test]
fn allowing_once_is_not_remembered() {
    let (mut browser, vault) = Browser::paired(storage());
    let sent = browser.get_logins(EXAMPLE);
    browser.ask(&sent);

    let entries = browser.answer_entries(&sent, ALLOW);

    assert_eq!(entries, ["Example", "Remembered", "Shown again"]);
    let open = browser.workspace.vault(vault).unwrap().vault();
    let example = open
        .root()
        .all_entries()
        .find(|entry| entry.title() == "Example")
        .unwrap();
    assert_eq!(example.access_decision(&["example.com".to_owned()]), None);
    let again = browser.get_logins(EXAMPLE);
    let pending = browser.ask(&again);
    assert_eq!(browser.titles_of(&pending), ["Example", "Shown again"]);
}

#[test]
fn a_remembered_allow_is_stored_on_the_entries_and_not_asked_again() {
    let storage = storage();
    let (mut browser, vault) = Browser::paired(storage.clone());
    let sent = browser.get_logins(EXAMPLE);
    browser.ask(&sent);

    let entries = browser.answer_entries(&sent, REMEMBER_ALLOW);

    assert_eq!(entries, ["Example", "Remembered", "Shown again"]);
    let open = browser.workspace.vault(vault).unwrap();
    assert!(open.has_unsaved_changes());
    let example = open
        .vault()
        .root()
        .all_entries()
        .find(|entry| entry.title() == "Example")
        .unwrap();
    assert_eq!(
        example.access_decision(&["example.com".to_owned()]),
        Some(AccessDecision::Allow)
    );
    let again = browser.get_logins(EXAMPLE);
    assert_eq!(
        browser.entries_now(&again),
        ["Example", "Remembered", "Shown again"]
    );

    // The decision travels with the Vault file.
    browser.workspace.save_vault(vault).unwrap();
    let (key_name, key) = browser.pairing.clone();
    let mut workspace = Workspace::new(storage);
    workspace
        .open_vault(&personal_vault(), &Credentials::password(PASSWORD))
        .unwrap();
    workspace.set_browser_integration(true);
    let mut later = FakeExtension::new();
    let keys = workspace
        .handle_browser_request(SECOND, &later.change_public_keys().json)
        .unwrap();
    later.accept_keys(&parse(&keys));
    let sent = later.get_logins(EXAMPLE, None, &[(&key_name, &key)]);
    let reply = workspace
        .handle_browser_request(SECOND, &sent.json)
        .unwrap();
    assert_eq!(titles(&later.open(&sent, &reply)).len(), 3);
}

#[test]
fn denying_once_leaves_out_the_entries_and_asks_again_next_time() {
    let (mut browser, _) = Browser::paired(storage());
    let sent = browser.get_logins(EXAMPLE);
    browser.ask(&sent);

    assert_eq!(browser.answer_entries(&sent, DENY), ["Remembered"]);

    let again = browser.get_logins(EXAMPLE);
    assert_eq!(browser.ask(&again).entries.len(), 2);
}

#[test]
fn a_remembered_deny_leaves_out_the_entries_without_asking_again() {
    let (mut browser, _) = Browser::paired(storage());
    let sent = browser.get_logins(EXAMPLE);
    browser.ask(&sent);

    assert_eq!(browser.answer_entries(&sent, REMEMBER_DENY), ["Remembered"]);

    let again = browser.get_logins(EXAMPLE);
    assert_eq!(browser.entries_now(&again), ["Remembered"]);
}

#[test]
fn a_decision_is_per_site() {
    let (mut browser, _) = Browser::paired(storage());
    let sent = browser.get_logins(EXAMPLE);
    browser.ask(&sent);
    browser.answer_entries(&sent, REMEMBER_ALLOW);

    // A subdomain is another site, and is asked about; there "Remembered" was denied.
    let elsewhere = browser.get_logins("https://evil.example.com/");
    let pending = browser.ask(&elsewhere);
    assert_eq!(pending.site, "evil.example.com");
    assert_eq!(browser.titles_of(&pending), ["Example", "Shown again"]);
}

#[test]
fn a_form_submitting_to_another_host_needs_that_host_allowed_too() {
    let (mut browser, vault) = Browser::paired(storage());
    let key = browser.key();
    let (name, key) = (key.0.to_owned(), key.1.to_owned());
    let sent = browser.get_logins_with(
        EXAMPLE,
        Some("https://auth.example.com/session"),
        &[(&name, &key)],
    );

    let pending = browser.ask(&sent);
    assert_eq!(
        browser.titles_of(&pending),
        ["Example", "Remembered", "Shown again"]
    );
    browser.answer_entries(&sent, REMEMBER_ALLOW);

    let open = browser.workspace.vault(vault).unwrap().vault();
    let example = open
        .root()
        .all_entries()
        .find(|entry| entry.title() == "Example")
        .unwrap();
    let both = ["example.com".to_owned(), "auth.example.com".to_owned()];
    assert_eq!(example.access_decision(&both), Some(AccessDecision::Allow));
}

#[test]
fn an_unpaired_vault_is_invisible() {
    let (mut browser, personal) = Browser::paired(storage());
    // The work Vault holds the same Entries, but isn't paired with this browser.
    browser
        .workspace
        .open_vault(&work_vault(), &Credentials::password(PASSWORD))
        .unwrap();

    let sent = browser.get_logins(EXAMPLE);
    let pending = browser.ask(&sent);
    assert!(pending.entries.iter().all(|&(vault, _)| vault == personal));
    assert_eq!(browser.answer_entries(&sent, ALLOW).len(), 3);

    // Without a Pairing key that a Vault knows, there is nothing to find.
    let other = FakeExtension::new();
    for keys in [
        vec![],
        vec![("Home", other.id_key())],
        vec![("Work", browser.pairing.1.clone())],
    ] {
        let keys: Vec<(&str, &str)> = keys.iter().map(|(n, k)| (*n, k.as_str())).collect();
        let sent = browser.get_logins_with(EXAMPLE, None, &keys);
        let reply = parse(&browser.request(&sent).unwrap());
        assert_eq!(reply["errorCode"], "15", "{keys:?}");
        assert_eq!(reply["action"], "get-logins");
    }
    assert_eq!(browser.workspace.pending_access(), None);
}

#[test]
fn sites_without_entries_get_no_logins_found_without_asking() {
    let (mut browser, _) = Browser::paired(storage());

    for url in [
        "https://unknown.test/",
        // https Entries are never offered on http.
        "http://example.com/",
        "file:///home/me/login.html",
    ] {
        let sent = browser.get_logins(url);
        let reply = parse(&browser.request(&sent).unwrap());
        assert_eq!(reply["errorCode"], "15", "{url}");
    }
    let sent = browser.get_logins("");
    assert_eq!(parse(&browser.request(&sent).unwrap())["errorCode"], "14");
    assert_eq!(browser.workspace.pending_access(), None);
}

#[test]
fn a_site_turned_down_for_every_entry_gets_an_empty_list_then_no_logins_found() {
    let (mut browser, _) = Browser::paired(storage());

    // Only "Other site" is for other.test. As in KeePassXC, the answer to the request the
    // user turned down is an empty list, and a later request finds nothing.
    let sent = browser.get_logins("https://other.test/");
    browser.ask(&sent);
    assert_eq!(
        browser.answer_entries(&sent, REMEMBER_DENY),
        [] as [&str; 0]
    );

    let again = browser.get_logins("https://other.test/");
    assert_eq!(parse(&browser.request(&again).unwrap())["errorCode"], "15");
}

#[test]
fn waiting_requests_a_remembered_decision_settles_are_answered_with_it() {
    let (mut browser, _) = Browser::paired(storage());
    let first = browser.get_logins(EXAMPLE);
    let reload = browser.get_logins(EXAMPLE);
    let other = browser.get_logins("https://other.test/");
    browser.ask(&first);
    browser.ask(&reload);
    browser.ask(&other);

    let pending = browser.workspace.pending_access().unwrap();
    let replies = browser.answer(&pending, REMEMBER_ALLOW);

    assert_eq!(replies.len(), 2);
    assert_eq!(
        titles(&browser.extension.open(&first, &replies[0].message)).len(),
        3
    );
    assert_eq!(
        titles(&browser.extension.open(&reload, &replies[1].message)).len(),
        3
    );
    let next = browser.workspace.pending_access().unwrap();
    assert_eq!(next.site, "other.test");
}

#[test]
fn waiting_requests_are_still_asked_after_allowing_once() {
    let (mut browser, _) = Browser::paired(storage());
    let first = browser.get_logins(EXAMPLE);
    let reload = browser.get_logins(EXAMPLE);
    browser.ask(&first);
    browser.ask(&reload);

    let pending = browser.workspace.pending_access().unwrap();
    assert_eq!(browser.answer(&pending, ALLOW).len(), 1);

    let next = browser.workspace.pending_access().unwrap();
    assert_ne!(next.id, pending.id);
    assert_eq!(browser.answer_entries(&reload, DENY), ["Remembered"]);
}

#[test]
fn only_the_request_put_to_the_user_can_be_answered() {
    let (mut browser, _) = Browser::paired(storage());
    let first = browser.get_logins(EXAMPLE);
    let second = browser.get_logins("https://other.test/");
    let pending = browser.ask(&first);
    browser.ask(&second);
    browser.answer(&pending, ALLOW);

    // Answering the first again does nothing.
    assert_eq!(browser.answer(&pending, ALLOW), []);
    assert_eq!(
        browser.workspace.pending_access().unwrap().site,
        "other.test"
    );
}

#[test]
fn an_extension_that_goes_away_takes_its_access_requests_with_it() {
    let (mut browser, _) = Browser::paired(storage());
    let pending = browser.ask(&browser.get_logins(EXAMPLE));

    browser.workspace.disconnect_browser(FIRST);

    assert_eq!(browser.workspace.pending_access(), None);
    assert_eq!(browser.answer(&pending, ALLOW), []);
}

#[test]
fn turning_integration_off_turns_waiting_access_requests_down() {
    let (mut browser, _) = Browser::paired(storage());
    browser.ask(&browser.get_logins(EXAMPLE));

    let replies = browser.workspace.set_browser_integration(false);

    assert_eq!(replies.len(), 1);
    let reply = parse(&replies[0].message);
    assert_eq!(reply["errorCode"], "6");
    assert_eq!(reply["action"], "get-logins");
    assert_eq!(browser.workspace.pending_access(), None);
}
