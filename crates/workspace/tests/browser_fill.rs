//! Filling on demand over the KeePassXC-Browser protocol: `get-logins` driven with real
//! NaCl-encrypted messages from a fake extension through a Workspace, with Access decisions put
//! to the user and remembered on the Entries, the Vault's Default access policy standing in for
//! those not made, and Pairings revoked.
//!
//! The Vault is KeePassXC's browser fixture: for https://example.com it offers "Example",
//! "Remembered" (already allowed example.com, and denied evil.example.com) and "Shown again".

mod support;

use std::sync::Arc;

use keefast_core::{AccessDecision, DefaultAccessPolicy, EditError, EntryId};
use keefast_workspace::{
    AccessAnswer, ConnectionId, EditVaultError, Outgoing, PendingAccess, UnlockSecrets,
    UnsavedLock, VaultId, Workspace,
};
use serde_json::Value;
use support::extension::{FakeExtension, Sent};
use support::{BROWSER_KDBX, FakeClipboard, FakeStorage, PASSWORD, personal_vault, work_vault};

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
            .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
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

impl Browser {
    /// Saves the Vault, whose Pairing is an unsaved change, and locks it.
    fn lock(&mut self, vault: VaultId) {
        self.workspace.save_vault(vault).unwrap();
        let saves = self
            .workspace
            .lock_vault(vault, &mut FakeClipboard::default());
        assert!(saves.is_empty());
        assert!(self.workspace.locked_vault(vault).is_some());
    }

    fn unlock(&mut self, location: &keefast_workspace::VaultLocation) {
        self.workspace
            .open_vault(location, &UnlockSecrets::password(PASSWORD))
            .unwrap();
    }

    /// The actions of the messages keefast sent the extension unasked.
    fn announcements(&mut self) -> Vec<String> {
        self.workspace
            .take_browser_messages()
            .into_iter()
            .map(|outgoing| {
                assert_eq!(outgoing.connection, FIRST);
                parse(&outgoing.message)["action"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    fn set_policy(&mut self, vault: VaultId, policy: DefaultAccessPolicy) -> Vec<Outgoing> {
        self.workspace
            .set_default_access_policy(vault, policy)
            .unwrap()
    }

    fn entry_id(&self, vault: VaultId, title: &str) -> EntryId {
        self.workspace
            .vault(vault)
            .unwrap()
            .vault()
            .root()
            .all_entries()
            .find(|entry| entry.title() == title)
            .unwrap()
            .id()
    }

    /// Sends the request and expects keefast to find nothing for it, without asking.
    #[track_caller]
    fn nothing_found(&mut self, sent: &Sent) {
        let reply = parse(&self.request(sent).expect("answered without asking"));
        assert_eq!(reply["errorCode"], "15");
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
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
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
        .open_vault(&work_vault(), &UnlockSecrets::password(PASSWORD))
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

mod locking {
    use super::*;

    #[test]
    fn a_locked_vault_answers_that_keefast_is_locked() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.lock(vault);

        let reply = parse(&browser.request(&browser.get_logins(EXAMPLE)).unwrap());

        assert_eq!(reply["errorCode"], "1");
        assert_eq!(reply["action"], "get-logins");
    }

    #[test]
    fn a_locked_vault_is_invisible_while_another_is_unlocked() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.unlock(&work_vault());
        browser.lock(vault);

        let reply = parse(&browser.request(&browser.get_logins(EXAMPLE)).unwrap());
        assert_eq!(
            reply["errorCode"], "15",
            "the unpaired Vault offers nothing"
        );
        let sent = browser
            .extension
            .test_associate(&browser.pairing.0, &browser.pairing.1);
        let reply = parse(&browser.request(&sent).unwrap());
        assert_eq!(
            reply["errorCode"], "8",
            "the Pairing is in the locked Vault"
        );
    }

    #[test]
    fn an_unlocked_vault_is_paired_with_the_browser_again() {
        let (mut browser, vault) = Browser::paired(storage());
        let sent = browser.get_logins(EXAMPLE);
        browser.ask(&sent);
        browser.answer_entries(&sent, REMEMBER_ALLOW);
        browser.lock(vault);

        browser.unlock(&personal_vault());

        let entries = browser.entries_now(&browser.get_logins(EXAMPLE));
        assert_eq!(entries, ["Example", "Remembered", "Shown again"]);
    }

    #[test]
    fn locking_answers_the_requests_waiting_on_the_vault() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.ask(&browser.get_logins(EXAMPLE));

        browser.lock(vault);

        assert_eq!(browser.workspace.pending_access(), None);
        let replies = browser.workspace.take_browser_messages();
        let errors: Vec<Value> = replies
            .iter()
            .map(|outgoing| parse(&outgoing.message))
            .filter(|message| message["action"] == "get-logins")
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["errorCode"], "1");
    }

    #[test]
    fn locking_turns_down_a_pairing_request_for_the_vault() {
        let (mut browser, vault) = Browser::paired(storage());
        assert_eq!(browser.request(&browser.extension.associate()), None);
        assert!(browser.workspace.pending_pairing().is_some());

        browser.lock(vault);

        assert_eq!(browser.workspace.pending_pairing(), None);
        let replies = browser.workspace.take_browser_messages();
        let error = replies
            .iter()
            .map(|outgoing| parse(&outgoing.message))
            .find(|message| message["action"] == "associate")
            .expect("the Pairing request is answered");
        assert_eq!(error["errorCode"], "1");
    }

    #[test]
    fn requests_waiting_on_other_vaults_stay_pending() {
        let (mut browser, _) = Browser::paired(storage());
        let work = browser
            .workspace
            .open_vault(&work_vault(), &UnlockSecrets::password(PASSWORD))
            .unwrap();
        let pending = browser.ask(&browser.get_logins(EXAMPLE));

        browser.lock(work);

        assert_eq!(browser.workspace.pending_access(), Some(pending));
    }

    #[test]
    fn the_extension_hears_when_vaults_lock_and_unlock() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.workspace.take_browser_messages();

        browser.lock(vault);
        assert_eq!(browser.announcements(), ["database-locked"]);

        browser.unlock(&personal_vault());
        assert_eq!(browser.announcements(), ["database-unlocked"]);
    }

    #[test]
    fn locking_every_vault_is_announced_once() {
        let (mut browser, personal) = Browser::paired(storage());
        browser.unlock(&work_vault());
        // The Pairing is an unsaved change, which locking would ask about.
        browser.workspace.save_vault(personal).unwrap();
        browser.workspace.take_browser_messages();

        let saves = browser.workspace.lock_all(&mut FakeClipboard::default());

        assert!(saves.is_empty());
        assert_eq!(browser.announcements(), ["database-locked"]);
    }

    #[test]
    fn locking_one_of_several_vaults_has_the_extension_reconnect() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.unlock(&work_vault());
        browser.workspace.take_browser_messages();

        browser.lock(vault);

        assert_eq!(
            browser.announcements(),
            ["database-locked", "database-unlocked"]
        );
    }

    /// Locks the Vault the way idle time or an OS lock does, with its Pairing still an unsaved
    /// change, so it waits for the user to save or discard.
    fn lock_waits_for_the_user(browser: &mut Browser, vault: VaultId) {
        let saves = browser.workspace.os_locked(&mut FakeClipboard::default());
        assert!(saves.is_empty());
        assert!(browser.workspace.vault(vault).unwrap().is_locking());
    }

    #[test]
    fn a_vault_waiting_to_lock_answers_that_keefast_is_locked() {
        let (mut browser, vault) = Browser::paired(storage());
        lock_waits_for_the_user(&mut browser, vault);

        let reply = parse(&browser.request(&browser.get_logins(EXAMPLE)).unwrap());

        assert_eq!(reply["errorCode"], "1");
        assert_eq!(reply["action"], "get-logins");
    }

    #[test]
    fn a_vault_waiting_to_lock_offers_nothing_while_another_is_open() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.unlock(&work_vault());
        // Only this Vault is asked to lock; the work Vault stays open.
        let saves = browser
            .workspace
            .lock_vault(vault, &mut FakeClipboard::default());
        assert!(saves.is_empty());
        assert!(browser.workspace.vault(vault).unwrap().is_locking());

        let reply = parse(&browser.request(&browser.get_logins(EXAMPLE)).unwrap());

        assert_eq!(
            reply["errorCode"], "15",
            "the unpaired Vault offers nothing"
        );
        assert_eq!(browser.workspace.pending_access(), None);
    }

    #[test]
    fn requests_waiting_on_a_vault_that_is_about_to_lock_are_answered_as_locked() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.ask(&browser.get_logins(EXAMPLE));

        lock_waits_for_the_user(&mut browser, vault);

        assert_eq!(browser.workspace.pending_access(), None);
        let errors: Vec<Value> = browser
            .workspace
            .take_browser_messages()
            .iter()
            .map(|outgoing| parse(&outgoing.message))
            .filter(|message| message["action"] == "get-logins")
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["errorCode"], "1");
    }

    const SHOP: &str = "https://shop.example/login";

    /// Sends a `set-login` that the user is asked about, from an extension that has proved its
    /// Pairing.
    fn ask_to_save_a_login(browser: &mut Browser, password: &str) {
        let (name, key) = browser.pairing.clone();
        browser.request(&browser.extension.test_associate(&name, &key));
        let login = browser
            .extension
            .set_login(SHOP, "carol", password, None, None);
        assert_eq!(browser.request(&login), None, "the user is asked");
        assert!(browser.workspace.pending_login().is_some());
    }

    #[test]
    fn a_pending_set_login_is_answered_as_locked_when_its_vault_is_about_to_lock() {
        let (mut browser, vault) = Browser::paired(storage());
        ask_to_save_a_login(&mut browser, "s3cret");
        browser.workspace.take_browser_messages();

        lock_waits_for_the_user(&mut browser, vault);

        assert!(browser.workspace.pending_login().is_none());
        let errors: Vec<Value> = browser
            .workspace
            .take_browser_messages()
            .iter()
            .map(|outgoing| parse(&outgoing.message))
            .filter(|message| message["action"] == "set-login")
            .collect();
        assert_eq!(errors.len(), 1, "the extension gets its reply");
        assert_eq!(errors[0]["errorCode"], "1");
    }

    #[test]
    fn a_stale_set_login_does_not_come_back_aimed_at_another_vault() {
        let (mut browser, vault) = Browser::paired(storage());
        browser.unlock(&work_vault());
        let work = browser
            .workspace
            .vaults()
            .map(|open| open.id())
            .find(|&id| id != vault)
            .unwrap();
        browser
            .workspace
            .pair_browser(work, "Home", &browser.pairing.1)
            .unwrap();
        ask_to_save_a_login(&mut browser, "first");

        let saves = browser
            .workspace
            .lock_vault(vault, &mut FakeClipboard::default());
        assert!(saves.is_empty());
        assert!(browser.workspace.vault(vault).unwrap().is_locking());

        // The login the personal Vault was asked about is gone, not queued for the work Vault.
        assert!(browser.workspace.pending_login().is_none());
        ask_to_save_a_login(&mut browser, "second");
    }

    #[test]
    fn a_lock_the_user_cancels_leaves_set_login_working() {
        let (mut browser, vault) = Browser::paired(storage());
        ask_to_save_a_login(&mut browser, "s3cret");

        // The user's own lock answers the waiting login too, and Cancel brings the Vault back.
        let saves = browser
            .workspace
            .lock_vault(vault, &mut FakeClipboard::default());
        assert!(saves.is_empty());
        assert!(browser.workspace.pending_login().is_none());
        browser.workspace.answer_lock(vault, UnsavedLock::Cancel);

        ask_to_save_a_login(&mut browser, "again");
    }

    #[test]
    fn a_lock_the_user_cancels_gives_the_vault_back_to_the_browser() {
        let (mut browser, vault) = Browser::paired(storage());
        let saves = browser
            .workspace
            .lock_vault(vault, &mut FakeClipboard::default());
        assert!(saves.is_empty());
        let hidden = parse(&browser.request(&browser.get_logins(EXAMPLE)).unwrap());
        assert_eq!(hidden["errorCode"], "1");

        browser.workspace.answer_lock(vault, UnsavedLock::Cancel);

        browser.ask(&browser.get_logins(EXAMPLE));
    }
}

#[test]
fn the_default_access_policy_asks() {
    let (mut browser, vault) = Browser::paired(storage());

    assert_eq!(
        browser.workspace.default_access_policy(vault),
        Some(DefaultAccessPolicy::Ask)
    );
    browser.ask(&browser.get_logins(EXAMPLE));
}

#[test]
fn always_allow_gives_undecided_entries_without_asking_but_not_denied_ones() {
    let (mut browser, vault) = Browser::paired(storage());

    browser.set_policy(vault, DefaultAccessPolicy::AlwaysAllow);

    let sent = browser.get_logins(EXAMPLE);
    assert_eq!(
        browser.entries_now(&sent),
        ["Example", "Remembered", "Shown again"]
    );
    // "Remembered" denied evil.example.com, and that Deny beats the policy.
    let elsewhere = browser.get_logins("https://evil.example.com/");
    assert_eq!(browser.entries_now(&elsewhere), ["Example", "Shown again"]);
    assert_eq!(browser.workspace.pending_access(), None);
    // Nothing is remembered on the Entries: the policy decides each time.
    let example = browser.entry_id(vault, "Example");
    assert_eq!(
        browser
            .workspace
            .entry(vault, example)
            .unwrap()
            .remembered_access(),
        []
    );
}

#[test]
fn always_deny_gives_only_allowed_entries_without_asking() {
    let (mut browser, vault) = Browser::paired(storage());

    browser.set_policy(vault, DefaultAccessPolicy::AlwaysDeny);

    // "Remembered" allowed example.com, and that Allow beats the policy.
    let sent = browser.get_logins(EXAMPLE);
    assert_eq!(browser.entries_now(&sent), ["Remembered"]);
    browser.nothing_found(&browser.get_logins("https://other.test/"));
    assert_eq!(browser.workspace.pending_access(), None);
}

#[test]
fn a_decision_remembered_under_one_policy_beats_another() {
    let (mut browser, vault) = Browser::paired(storage());
    let sent = browser.get_logins("https://other.test/");
    browser.ask(&sent);
    browser.answer_entries(&sent, REMEMBER_ALLOW);

    browser.set_policy(vault, DefaultAccessPolicy::AlwaysDeny);

    let again = browser.get_logins("https://other.test/");
    assert_eq!(browser.entries_now(&again), ["Other site"]);
}

#[test]
fn the_default_access_policy_is_per_vault() {
    let (mut browser, personal) = Browser::paired(storage());
    let work = browser
        .workspace
        .open_vault(&work_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    browser
        .workspace
        .pair_browser(work, "Home", &browser.pairing.1)
        .unwrap();

    browser.set_policy(personal, DefaultAccessPolicy::AlwaysAllow);

    // Only the work Vault's Entries are asked about.
    let sent = browser.get_logins(EXAMPLE);
    let pending = browser.ask(&sent);
    assert!(pending.entries.iter().all(|&(vault, _)| vault == work));
    assert_eq!(browser.titles_of(&pending), ["Example", "Shown again"]);
    assert_eq!(browser.answer_entries(&sent, DENY).len(), 4);
}

#[test]
fn the_default_access_policy_travels_with_the_vault_file() {
    let storage = storage();
    let (mut browser, vault) = Browser::paired(storage.clone());
    browser.set_policy(vault, DefaultAccessPolicy::AlwaysDeny);
    assert!(
        browser
            .workspace
            .vault(vault)
            .unwrap()
            .has_unsaved_changes()
    );

    browser.workspace.save_vault(vault).unwrap();

    let mut workspace = Workspace::new(storage);
    let reopened = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    assert_eq!(
        workspace.default_access_policy(reopened),
        Some(DefaultAccessPolicy::AlwaysDeny)
    );
}

#[test]
fn a_new_policy_answers_the_requests_it_settles() {
    let (mut browser, vault) = Browser::paired(storage());
    let first = browser.get_logins(EXAMPLE);
    let other = browser.get_logins("https://other.test/");
    browser.ask(&first);
    browser.ask(&other);

    let replies = browser.set_policy(vault, DefaultAccessPolicy::AlwaysAllow);

    assert_eq!(replies.len(), 2);
    assert_eq!(
        titles(&browser.extension.open(&first, &replies[0].message)),
        ["Example", "Remembered", "Shown again"]
    );
    assert_eq!(
        titles(&browser.extension.open(&other, &replies[1].message)),
        ["Other site"]
    );
    assert_eq!(browser.workspace.pending_access(), None);
}

#[test]
fn back_to_asking_leaves_waiting_requests_waiting() {
    let (mut browser, vault) = Browser::paired(storage());
    browser.ask(&browser.get_logins(EXAMPLE));

    assert_eq!(browser.set_policy(vault, DefaultAccessPolicy::Ask), []);

    assert!(browser.workspace.pending_access().is_some());
}

#[test]
fn a_forgotten_decision_is_asked_again() {
    let (mut browser, vault) = Browser::paired(storage());
    let remembered = browser.entry_id(vault, "Remembered");

    browser
        .workspace
        .forget_access(vault, remembered, Some("example.com"))
        .unwrap();

    assert!(
        browser
            .workspace
            .vault(vault)
            .unwrap()
            .has_unsaved_changes()
    );
    let pending = browser.ask(&browser.get_logins(EXAMPLE));
    assert_eq!(
        browser.titles_of(&pending),
        ["Example", "Remembered", "Shown again"]
    );
}

#[test]
fn a_forgotten_deny_lets_the_default_access_policy_decide() {
    let (mut browser, vault) = Browser::paired(storage());
    browser.set_policy(vault, DefaultAccessPolicy::AlwaysAllow);
    let remembered = browser.entry_id(vault, "Remembered");

    browser
        .workspace
        .forget_access(vault, remembered, None)
        .unwrap();

    let elsewhere = browser.get_logins("https://evil.example.com/");
    assert_eq!(
        browser.entries_now(&elsewhere),
        ["Example", "Remembered", "Shown again"]
    );
}

#[test]
fn a_revoked_pairing_finds_nothing_and_can_save_nothing() {
    let (mut browser, vault) = Browser::paired(storage());
    browser.set_policy(vault, DefaultAccessPolicy::AlwaysAllow);
    let (name, key) = browser.pairing.clone();
    browser.request(&browser.extension.test_associate(&name, &key));

    browser.workspace.revoke_pairing(vault, "Home").unwrap();

    assert!(
        browser
            .workspace
            .vault(vault)
            .unwrap()
            .has_unsaved_changes()
    );
    assert_eq!(
        browser.workspace.vault(vault).unwrap().vault().pairings(),
        []
    );
    browser.nothing_found(&browser.get_logins(EXAMPLE));
    let check = browser.extension.test_associate(&name, &key);
    assert_eq!(parse(&browser.request(&check).unwrap())["errorCode"], "8");
    let login = browser
        .extension
        .set_login(EXAMPLE, "alice", "new-secret", None, None);
    assert_eq!(parse(&browser.request(&login).unwrap())["errorCode"], "8");
    assert_eq!(
        browser.workspace.revoke_pairing(vault, "Home"),
        Err(EditVaultError::Edit(EditError::PairingNotFound))
    );
}

#[test]
fn a_request_waiting_when_its_pairing_is_revoked_gets_nothing() {
    let (mut browser, vault) = Browser::paired(storage());
    let sent = browser.get_logins(EXAMPLE);
    browser.ask(&sent);

    browser.workspace.revoke_pairing(vault, "Home").unwrap();

    assert_eq!(browser.answer_entries(&sent, ALLOW), [] as [&str; 0]);
}
