//! Filling a payment form from a Card through keefast's protocol extensions (ADR 0010):
//! `keefast-search-entries` with a `form` listing the Entries with a Card, and
//! `keefast-fill-form` giving the one the user picks in the command palette its fields by
//! `autocomplete` token, the pick remembered as its Access decision.
//!
//! The Vault is KeePassXC's browser fixture, with Cards added: "Company Visa" and "Shop card",
//! whose URL is https://shop.test. The Workspace is told what the Card Plugin fills, as the app
//! tells it while the Plugin runs with its `browserFill` Plugin capability.

mod support;

use std::sync::Arc;

use keefast_core::{AccessDecision, CustomField, DefaultAccessPolicy, EntryFields, EntryId};
use keefast_workspace::{AccessAnswer, ConnectionId, FormFill, UnlockSecrets, VaultId, Workspace};
use secrecy::SecretString;
use serde_json::Value;
use support::extension::{FakeExtension, Sent};
use support::{BROWSER_KDBX, FakeClipboard, FakeStorage, PASSWORD, personal_vault};

const CONNECTION: ConnectionId = ConnectionId(1);
const SHOP: &str = "https://shop.test/checkout";
const PSP: &str = "https://js.payments.test/elements";

fn parse(reply: &str) -> Value {
    serde_json::from_str(reply).unwrap()
}

/// What the Card Plugin fills, as the app hands it to the Workspace.
fn card_fill() -> FormFill {
    let field = |name: &str, token: &str| (name.to_owned(), token.to_owned());
    FormFill {
        form: "payment".into(),
        detect: vec!["Card number".into()],
        fields: vec![
            field("Card number", "cc-number"),
            field("Cardholder", "cc-name"),
            field("Expiry", "cc-exp"),
            field("CVV", "cc-csc"),
        ],
    }
}

fn custom(name: &str, value: &str, protected: bool) -> CustomField {
    CustomField {
        name: name.into(),
        value: SecretString::from(value.to_owned()),
        protected,
    }
}

struct Browser {
    workspace: Workspace,
    extension: FakeExtension,
    vault: VaultId,
    /// The Pairing the extension holds, as name and key.
    pairing: (String, String),
}

/// A reply as text without its nonce and Entry uuids: they are random, so they may hold any
/// digits a test checks a reply doesn't.
fn without_random_ids(mut reply: Value) -> String {
    if let Some(reply) = reply.as_object_mut() {
        reply.remove("nonce");
        reply.remove("uuid");
    }
    if let Some(entries) = reply.get_mut("entries").and_then(Value::as_array_mut) {
        for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
            entry.remove("uuid");
        }
    }
    reply.to_string()
}

impl Browser {
    /// Opens the personal Vault, adds the Cards, tells the Workspace the Card Plugin fills
    /// payment forms, and pairs an extension with the Vault as "Home".
    fn paired() -> Self {
        let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), BROWSER_KDBX));
        let mut workspace = Workspace::new(storage);
        let vault = workspace
            .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
            .unwrap();
        let root = workspace.vault(vault).unwrap().vault().root().id();
        let card = |title: &str, url: &str, cvv: &str| EntryFields {
            title: title.into(),
            url: url.into(),
            custom_fields: vec![
                custom("Card number", "4111 1111 1111 1111", true),
                custom("Cardholder", "Ada Lovelace", false),
                custom("Expiry", "08/31", false),
                custom("CVV", cvv, true),
                custom("PIN", "4321", true),
            ],
            ..EntryFields::default()
        };
        workspace
            .create_entry(vault, root, &card("Company Visa", "", "737"))
            .unwrap();
        workspace
            .create_entry(vault, root, &card("Shop card", "https://shop.test", ""))
            .unwrap();
        workspace.set_form_fills(vec![card_fill()]);
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

    /// The error code a request is answered with, and the whole reply, which must hold no
    /// message to open.
    #[track_caller]
    fn error(&mut self, sent: &Sent) -> Value {
        let reply = self
            .workspace
            .handle_browser_request(CONNECTION, &sent.json)
            .expect("answered at once");
        let reply = parse(&reply);
        assert!(reply.get("message").is_none(), "{reply}");
        reply["errorCode"].clone()
    }

    /// The titles of the Entries with a Card that searching for `query` finds.
    #[track_caller]
    fn search(&mut self, query: &str) -> Vec<String> {
        let sent = self
            .extension
            .search_form("payment", query, SHOP, &[self.key()]);
        let message = self.send(&sent);
        message["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_owned())
            .collect()
    }

    fn fill(&self, title: &str, frame: Option<&str>) -> Sent {
        let uuid = self.entry_id(title).to_hex();
        self.extension
            .fill_form("payment", &uuid, SHOP, frame, &[self.key()])
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

    /// Turns "Shop card" down for shop.test, as the user does when the site asks for it.
    fn deny_shop_card(&mut self) {
        let key = self.key();
        let logins = self.extension.get_logins(SHOP, None, &[(key.0, key.1)]);
        assert!(
            self.workspace
                .handle_browser_request(CONNECTION, &logins.json)
                .is_none(),
            "keefast asks the user"
        );
        let pending = self.workspace.pending_access().unwrap();
        self.workspace.answer_access(
            pending.id,
            AccessAnswer {
                decision: AccessDecision::Deny,
                remember: true,
            },
        );
    }
}

/// The values a reply fills, as `token=value`.
fn filled(message: &Value) -> Vec<String> {
    message["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            format!(
                "{}={}",
                field["autocomplete"].as_str().unwrap(),
                field["value"].as_str().unwrap()
            )
        })
        .collect()
}

const NO_LOGINS_FOUND: &str = "15";

#[test]
fn picking_a_card_gives_its_fields_by_autocomplete_token_but_never_its_pin() {
    let mut browser = Browser::paired();
    let sent = browser.fill("Company Visa", None);

    let message = browser.send(&sent);

    assert_eq!(message["name"], "Company Visa");
    assert_eq!(message["form"], "payment");
    assert_eq!(message["uuid"], browser.entry_id("Company Visa").to_hex());
    assert_eq!(
        filled(&message),
        [
            "cc-number=4111 1111 1111 1111",
            "cc-name=Ada Lovelace",
            "cc-exp=08/31",
            "cc-csc=737",
        ]
    );
    assert!(!without_random_ids(message).contains("4321"), "no PIN");
}

#[test]
fn a_card_without_a_cvv_fills_the_rest() {
    let mut browser = Browser::paired();
    let sent = browser.fill("Shop card", None);

    let message = browser.send(&sent);

    assert!(filled(&message).iter().all(|f| !f.starts_with("cc-csc")));
    assert_eq!(filled(&message).len(), 3);
}

#[test]
fn picking_a_card_remembers_it_as_allowed_for_the_site_and_the_frame() {
    let mut browser = Browser::paired();
    let sent = browser.fill("Company Visa", Some(PSP));

    browser.send(&sent);

    let entry = browser.entry("Company Visa");
    for host in ["shop.test", "js.payments.test"] {
        assert_eq!(
            entry.access_decision(&[host.to_owned()]),
            Some(AccessDecision::Allow),
            "{host}"
        );
    }
    // Nothing else changes: the Card isn't for the site from now on.
    assert!(entry.additional_urls().is_empty());
}

#[test]
fn a_card_denied_the_site_or_the_frame_gives_nothing() {
    let mut browser = Browser::paired();
    browser.deny_shop_card();

    // Denied the site.
    let sent = browser.fill("Shop card", None);
    assert_eq!(browser.error(&sent), NO_LOGINS_FOUND);
    assert!(!browser.search("shop").contains(&"Shop card".to_owned()));

    // Denied the site, the form in a frame from elsewhere.
    let sent = browser.fill("Shop card", Some(PSP));
    assert_eq!(browser.error(&sent), NO_LOGINS_FOUND);

    // Denied only the frame's host: the values don't go there either.
    let mut browser = Browser::paired();
    let uuid = browser.entry_id("Shop card").to_hex();
    let key = browser.key();
    let sent = browser.extension.fill_form(
        "payment",
        &uuid,
        "https://other.test/pay",
        Some(SHOP),
        &[key],
    );
    browser.deny_shop_card();
    assert_eq!(browser.error(&sent), NO_LOGINS_FOUND);
}

#[test]
fn a_vault_that_always_denies_fills_nothing() {
    let mut browser = Browser::paired();
    let vault = browser.vault;
    browser
        .workspace
        .set_default_access_policy(vault, DefaultAccessPolicy::AlwaysDeny)
        .unwrap();

    let sent = browser.fill("Company Visa", None);
    assert_eq!(browser.error(&sent), NO_LOGINS_FOUND);
    assert!(browser.search("").is_empty());
}

#[test]
fn a_vault_that_asks_takes_the_pick_as_the_answer() {
    let mut browser = Browser::paired();
    let vault = browser.vault;
    browser
        .workspace
        .set_default_access_policy(vault, DefaultAccessPolicy::Ask)
        .unwrap();

    let sent = browser.fill("Company Visa", None);
    assert_eq!(filled(&browser.send(&sent)).len(), 4);
    assert!(browser.workspace.pending_access().is_none());
}

#[test]
fn only_vaults_the_extension_is_paired_with_fill() {
    let mut browser = Browser::paired();
    let uuid = browser.entry_id("Company Visa").to_hex();
    let stranger = FakeExtension::new().id_key();

    let sent = browser
        .extension
        .fill_form("payment", &uuid, SHOP, None, &[("Home", &stranger)]);

    assert_eq!(browser.error(&sent), NO_LOGINS_FOUND);
}

#[test]
fn without_a_plugin_that_fills_payment_forms_nothing_is_filled() {
    // The Card Plugin disabled or removed, or running without browser fill.
    let mut browser = Browser::paired();
    browser.workspace.set_form_fills(Vec::new());

    let sent = browser.fill("Company Visa", None);
    assert_eq!(browser.error(&sent), NO_LOGINS_FOUND);
    assert!(browser.search("").is_empty());
    assert!(browser.search("visa").is_empty());
}

#[test]
fn a_plugin_filling_fewer_fields_gives_only_those() {
    let mut browser = Browser::paired();
    let mut fill = card_fill();
    fill.fields.retain(|(name, _)| name != "CVV");
    browser.workspace.set_form_fills(vec![fill]);

    let sent = browser.fill("Company Visa", None);
    let message = browser.send(&sent);
    let message = without_random_ids(message);
    assert!(!message.contains("737"), "{message}");
}

#[test]
fn an_entry_without_a_card_or_another_form_fills_nothing() {
    let mut browser = Browser::paired();
    let sent = browser.fill("Other site", None);
    assert_eq!(browser.error(&sent), NO_LOGINS_FOUND);

    let uuid = browser.entry_id("Company Visa").to_hex();
    let (name, key) = browser.pairing.clone();
    for form in ["login", ""] {
        let sent = browser
            .extension
            .fill_form(form, &uuid, SHOP, None, &[(&name, &key)]);
        assert_eq!(browser.error(&sent), NO_LOGINS_FOUND, "{form:?}");
    }
}

#[test]
fn searching_for_a_payment_form_lists_the_cards_without_their_values() {
    let mut browser = Browser::paired();

    // Every Card, for an empty query; only Cards.
    assert_eq!(browser.search(""), ["Company Visa", "Shop card"]);
    assert_eq!(browser.search("visa"), ["Company Visa"]);
    assert!(browser.search("other").is_empty());

    let sent = browser
        .extension
        .search_form("payment", "", SHOP, &[browser.key()]);
    let message = without_random_ids(browser.send(&sent));
    for value in ["4111", "737", "4321", "Ada"] {
        assert!(!message.contains(value), "{value} in {message}");
    }
    // A login search still finds nothing for nothing typed.
    let sent = browser.extension.search_entries("", SHOP, &[browser.key()]);
    assert_eq!(browser.send(&sent)["count"], 0);
}

#[test]
fn a_locked_keefast_fills_nothing() {
    let mut browser = Browser::paired();
    let sent = browser.fill("Company Visa", None);
    let vault = browser.vault;
    browser
        .workspace
        .lock_vault(vault, &mut FakeClipboard::default());

    let reply = parse(
        &browser
            .workspace
            .handle_browser_request(CONNECTION, &sent.json)
            .unwrap(),
    );
    assert_eq!(reply["errorCode"], "1");
    assert!(reply.get("message").is_none());
}
