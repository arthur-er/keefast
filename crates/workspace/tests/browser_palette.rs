//! The Companion extension's command palette over keefast's protocol extensions:
//! `keefast-search-entries` finding Entries in the paired Vaults whatever their site, and
//! `keefast-pick-entry` giving the one the user picks to the site, the pick remembered as its
//! Access decision.
//!
//! The Vault is KeePassXC's browser fixture: "Example", "Remembered" and "Shown again" are for
//! https://example.com, "Other site" for https://other.test, and "Hidden", "In hidden group"
//! and "Deleted" are hidden from browsers or in the Recycle Bin.

mod support;

use std::sync::Arc;

use keefast_core::{AccessDecision, DefaultAccessPolicy, EntryId};
use keefast_workspace::{ConnectionId, UnlockSecrets, VaultId, Workspace};
use secrecy::ExposeSecret;
use serde_json::Value;
use support::extension::{FakeExtension, Sent};
use support::{BROWSER_KDBX, FakeStorage, PASSWORD, personal_vault};

const CONNECTION: ConnectionId = ConnectionId(1);
const UNRELATED: &str = "https://unrelated.test/sign-in";

fn parse(reply: &str) -> Value {
    serde_json::from_str(reply).unwrap()
}

struct Browser {
    workspace: Workspace,
    extension: FakeExtension,
    vault: VaultId,
    /// The Pairing the extension holds, as name and key.
    pairing: (String, String),
}

impl Browser {
    /// Opens the personal Vault and pairs an extension with it as "Home".
    fn paired() -> Self {
        let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), BROWSER_KDBX));
        let mut workspace = Workspace::new(storage);
        let vault = workspace
            .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
            .unwrap();
        workspace.set_browser_integration(true);
        let mut extension = FakeExtension::new();
        let keys = workspace
            .handle_browser_request(CONNECTION, &extension.change_public_keys().json)
            .unwrap();
        extension.accept_keys(&parse(&keys));
        workspace.handle_browser_request(CONNECTION, &extension.associate().json);
        let pairing = workspace.pending_pairing().unwrap();
        workspace.approve_pairing(pairing.id, "Home").unwrap();
        let pairing = ("Home".to_owned(), extension.id_key());
        Self {
            workspace,
            extension,
            vault,
            pairing,
        }
    }

    fn key(&self) -> (&str, &str) {
        (&self.pairing.0, &self.pairing.1)
    }

    #[track_caller]
    fn send(&mut self, sent: &Sent) -> Value {
        let reply = self
            .workspace
            .handle_browser_request(CONNECTION, &sent.json)
            .expect("answered at once");
        self.extension.open(sent, &reply)
    }

    #[track_caller]
    fn error(&mut self, sent: &Sent) -> Value {
        let reply = self
            .workspace
            .handle_browser_request(CONNECTION, &sent.json)
            .expect("answered at once");
        parse(&reply)["errorCode"].clone()
    }

    /// The titles of the Entries searching for `query` from `url` finds.
    #[track_caller]
    fn search(&mut self, query: &str, url: &str) -> Vec<String> {
        let sent = self.extension.search_entries(query, url, &[self.key()]);
        titles(&self.send(&sent))
    }

    fn pick(&self, title: &str, url: &str, add_url: bool) -> Sent {
        let uuid = self.entry_id(title).to_hex();
        self.extension
            .pick_entry(&uuid, url, &[self.key()], add_url)
    }

    fn entry_id(&self, title: &str) -> EntryId {
        self.workspace
            .vault(self.vault)
            .unwrap()
            .vault()
            .root()
            .all_entries()
            .find(|entry| entry.title() == title)
            .unwrap()
            .id()
    }

    fn entry(&self, title: &str) -> &keefast_core::Entry {
        self.workspace
            .entry(self.vault, self.entry_id(title))
            .unwrap()
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
fn searching_finds_entries_whatever_their_site() {
    let mut browser = Browser::paired();

    assert_eq!(browser.search("other", UNRELATED), ["Other site"]);
    assert_eq!(browser.search("alice", UNRELATED), ["Example"]);
}

#[test]
fn search_results_carry_what_the_palette_lists_but_no_password() {
    let mut browser = Browser::paired();
    let sent = browser
        .extension
        .search_entries("shown", UNRELATED, &[browser.key()]);

    let message = browser.send(&sent);

    let entry = &message["entries"][0];
    assert_eq!(entry["name"], "Shown again");
    assert_eq!(entry["login"], "shown");
    assert_eq!(entry["url"], "example.com");
    assert_eq!(entry["group"], "Hidden group / Shown again");
    assert_eq!(entry["hasTotp"], false);
    assert_eq!(entry["uuid"], browser.entry_id("Shown again").to_hex());
    assert!(entry.get("password").is_none());
    assert!(entry.get("totp").is_none());
}

#[test]
fn an_empty_query_finds_nothing() {
    let mut browser = Browser::paired();

    assert!(browser.search("", UNRELATED).is_empty());
    assert!(browser.search("   ", UNRELATED).is_empty());
}

#[test]
fn entries_hidden_from_browsers_or_deleted_are_not_found() {
    let mut browser = Browser::paired();

    assert!(browser.search("hidden", UNRELATED).is_empty());
    assert!(browser.search("grouped", UNRELATED).is_empty());
    assert!(browser.search("deleted", UNRELATED).is_empty());
}

#[test]
fn entries_denied_the_site_are_not_found_there() {
    let mut browser = Browser::paired();

    // "Remembered" is denied evil.example.com.
    assert!(
        browser
            .search("remembered", "https://evil.example.com/")
            .is_empty()
    );
    assert_eq!(browser.search("remembered", UNRELATED), ["Remembered"]);
}

#[test]
fn a_vault_that_always_denies_is_not_searched() {
    let mut browser = Browser::paired();
    let vault = browser.vault;
    browser
        .workspace
        .set_default_access_policy(vault, DefaultAccessPolicy::AlwaysDeny)
        .unwrap();

    assert!(browser.search("other", UNRELATED).is_empty());
}

#[test]
fn only_vaults_the_extension_is_paired_with_are_searched() {
    let mut browser = Browser::paired();
    let stranger = FakeExtension::new().id_key();
    let sent = browser
        .extension
        .search_entries("other", UNRELATED, &[("Home", &stranger)]);

    assert!(titles(&browser.send(&sent)).is_empty());
}

#[test]
fn picking_an_entry_gives_it_to_the_site() {
    let mut browser = Browser::paired();
    let sent = browser.pick("Other site", UNRELATED, false);

    let message = browser.send(&sent);

    assert_eq!(titles(&message), ["Other site"]);
    let entry = &message["entries"][0];
    assert_eq!(entry["login"], "other");
    assert_eq!(entry["password"], "other-secret");
}

#[test]
fn picking_an_entry_remembers_it_as_allowed_for_the_site() {
    let mut browser = Browser::paired();
    let sent = browser.pick("Other site", UNRELATED, false);

    browser.send(&sent);

    let entry = browser.entry("Other site");
    assert_eq!(
        entry.access_decision(&["unrelated.test".to_owned()]),
        Some(AccessDecision::Allow)
    );
    assert!(entry.additional_urls().is_empty());
    assert!(
        browser
            .workspace
            .vault(browser.vault)
            .unwrap()
            .has_unsaved_changes()
    );
}

#[test]
fn picking_with_add_url_offers_the_entry_to_the_site_from_then_on() {
    let mut browser = Browser::paired();
    let sent = browser.pick("Other site", UNRELATED, true);
    browser.send(&sent);

    assert_eq!(
        browser.entry("Other site").additional_urls(),
        ["https://unrelated.test"]
    );
    // Allowed already, so the site gets it without the user being asked.
    let key = browser.key();
    let logins = browser
        .extension
        .get_logins(UNRELATED, None, &[(key.0, key.1)]);
    assert_eq!(titles(&browser.send(&logins)), ["Other site"]);
}

#[test]
fn an_added_url_takes_the_next_free_field_name() {
    let mut browser = Browser::paired();
    // "Example" has KP2A_URL and KP2A_URL_1 already.
    let sent = browser.pick("Example", UNRELATED, true);
    browser.send(&sent);

    let entry = browser.entry("Example");
    let added = entry
        .custom_fields()
        .iter()
        .find(|field| field.name == "KP2A_URL_2")
        .expect("added as KP2A_URL_2");
    assert_eq!(added.value.expose_secret(), "https://unrelated.test");
    assert!(!added.protected);
}

#[test]
fn an_entry_already_for_the_site_gets_no_url_added() {
    let mut browser = Browser::paired();
    let sent = browser.pick("Example", "https://example.com/login", true);
    browser.send(&sent);

    assert_eq!(browser.entry("Example").additional_urls().len(), 2);
}

#[test]
fn an_entry_denied_the_site_cannot_be_picked() {
    let mut browser = Browser::paired();
    let sent = browser.pick("Remembered", "https://evil.example.com/", false);

    assert_eq!(browser.error(&sent), "15");
}

#[test]
fn an_entry_hidden_from_browsers_cannot_be_picked() {
    let mut browser = Browser::paired();
    let sent = browser.pick("Hidden", UNRELATED, false);

    assert_eq!(browser.error(&sent), "15");
}

#[test]
fn an_unpaired_extension_cannot_pick() {
    let mut browser = Browser::paired();
    let uuid = browser.entry_id("Other site").to_hex();
    let stranger = FakeExtension::new().id_key();
    let sent = browser
        .extension
        .pick_entry(&uuid, UNRELATED, &[("Home", &stranger)], false);

    assert_eq!(browser.error(&sent), "15");
}
