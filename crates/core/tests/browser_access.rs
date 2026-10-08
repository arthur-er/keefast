//! The Entries a Vault offers a site, and the Access decisions remembered on them, read from and
//! written to a Vault file the way KeePassXC keeps them.

use keefast_core::{AccessDecision, CompositeKey, Site, Timestamp, Vault};
use keepass::{Database, DatabaseKey};

const BROWSER_FIXTURE: &[u8] = include_bytes!("../../../fixtures/keepassxc-browser.kdbx");
const PASSWORD: &str = "fixture-password";

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    open(&vault.save().expect("Vault saves"))
}

fn now() -> Timestamp {
    Timestamp::from_unix_seconds(1_791_414_922)
}

fn site(url: &str) -> Site {
    Site::parse(url).unwrap()
}

fn titles_for(vault: &Vault, url: &str) -> Vec<String> {
    vault
        .entries_for_site(&site(url))
        .iter()
        .map(|found| found.entry.title().to_owned())
        .collect()
}

fn entry_titled<'a>(vault: &'a Vault, title: &str) -> &'a keefast_core::Entry {
    vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap_or_else(|| panic!("no Entry {title:?}"))
}

fn hosts(hosts: &[&str]) -> Vec<String> {
    hosts.iter().map(|host| host.to_string()).collect()
}

#[test]
fn a_site_is_offered_matching_entries_except_hidden_and_recycled_ones() {
    let vault = open(BROWSER_FIXTURE);

    // "Hidden" is hidden itself, "In hidden group" by its Group, and "Deleted" is in the
    // Recycle Bin. "Shown again" is in a Group under the hidden one that shows it again.
    assert_eq!(
        titles_for(&vault, "https://login.example.com/"),
        ["Example", "Remembered", "Shown again"]
    );
    assert_eq!(titles_for(&vault, "https://other.test/"), ["Other site"]);
    assert_eq!(titles_for(&vault, "https://unknown.test/"), [] as [&str; 0]);
}

#[test]
fn additional_urls_are_matched_too() {
    let vault = open(BROWSER_FIXTURE);

    assert_eq!(titles_for(&vault, "https://example.org/"), ["Example"]);
    assert_eq!(
        titles_for(&vault, "https://login.example.net/"),
        ["Example"]
    );
    assert_eq!(titles_for(&vault, "https://example.net/"), [] as [&str; 0]);
    let example = entry_titled(&vault, "Example");
    assert_eq!(
        example.additional_urls(),
        ["https://example.org", "https://login.example.net"]
    );
}

#[test]
fn offered_entries_come_with_their_group() {
    let vault = open(BROWSER_FIXTURE);

    let found = vault.entries_for_site(&site("https://example.com/"));

    let groups: Vec<&str> = found.iter().map(|found| found.group.name()).collect();
    assert_eq!(groups, ["Passwords", "Passwords", "Shown again"]);
}

#[test]
fn https_entries_are_not_offered_on_http() {
    let vault = open(BROWSER_FIXTURE);

    assert_eq!(titles_for(&vault, "http://example.com/"), [] as [&str; 0]);
}

#[test]
fn decisions_remembered_by_keepassxc_are_read() {
    let vault = open(BROWSER_FIXTURE);
    let remembered = entry_titled(&vault, "Remembered");

    assert_eq!(
        remembered.access_decision(&hosts(&["example.com"])),
        Some(AccessDecision::Allow)
    );
    assert_eq!(
        remembered.access_decision(&hosts(&["evil.example.com"])),
        Some(AccessDecision::Deny)
    );
    assert_eq!(
        remembered.access_decision(&hosts(&["login.example.com"])),
        None
    );
    let example = entry_titled(&vault, "Example");
    assert_eq!(example.access_decision(&hosts(&["example.com"])), None);
}

#[test]
fn a_form_submitting_elsewhere_needs_both_hosts_allowed_and_either_denied_denies() {
    let vault = open(BROWSER_FIXTURE);
    let remembered = entry_titled(&vault, "Remembered");

    assert_eq!(
        remembered.access_decision(&hosts(&["example.com", "login.example.com"])),
        None
    );
    assert_eq!(
        remembered.access_decision(&hosts(&["example.com", "evil.example.com"])),
        Some(AccessDecision::Deny)
    );
}

#[test]
fn a_remembered_decision_is_stored_on_the_entry_and_survives_a_save() {
    let mut vault = open(BROWSER_FIXTURE);
    let example = entry_titled(&vault, "Example").id();
    let history = entry_titled(&vault, "Example").history().len();

    vault
        .remember_access(
            example,
            &hosts(&["example.com"]),
            AccessDecision::Allow,
            now(),
        )
        .unwrap();
    vault
        .remember_access(
            example,
            &hosts(&["evil.example.com"]),
            AccessDecision::Deny,
            now(),
        )
        .unwrap();

    let vault = reopen(&vault);
    let example = entry_titled(&vault, "Example");
    assert_eq!(
        example.access_decision(&hosts(&["example.com"])),
        Some(AccessDecision::Allow)
    );
    assert_eq!(
        example.access_decision(&hosts(&["evil.example.com"])),
        Some(AccessDecision::Deny)
    );
    assert_eq!(example.last_modified(), Some(now()));
    assert_eq!(
        example.history().len(),
        history,
        "no history for a decision"
    );
}

#[test]
fn a_new_decision_replaces_the_old_one_for_that_host_only() {
    let mut vault = open(BROWSER_FIXTURE);
    let remembered = entry_titled(&vault, "Remembered").id();

    vault
        .remember_access(
            remembered,
            &hosts(&["evil.example.com"]),
            AccessDecision::Allow,
            now(),
        )
        .unwrap();
    vault
        .remember_access(
            remembered,
            &hosts(&["example.com"]),
            AccessDecision::Deny,
            now(),
        )
        .unwrap();

    let remembered = entry_titled(&vault, "Remembered");
    assert_eq!(
        remembered.access_decision(&hosts(&["evil.example.com"])),
        Some(AccessDecision::Allow)
    );
    assert_eq!(
        remembered.access_decision(&hosts(&["example.com"])),
        Some(AccessDecision::Deny)
    );
}

#[test]
fn decisions_are_written_in_keepassxcs_form() {
    let mut vault = open(BROWSER_FIXTURE);
    let example = entry_titled(&vault, "Example").id();
    vault
        .remember_access(
            example,
            &hosts(&["example.com", "login.example.com"]),
            AccessDecision::Allow,
            now(),
        )
        .unwrap();

    // What an independent KDBX reader sees in the Entry's custom data.
    let bytes = vault.save().unwrap();
    let xml = Database::get_xml(&mut &bytes[..], DatabaseKey::new().with_password(PASSWORD))
        .expect("keepass decrypts");
    let xml: String = String::from_utf8(xml)
        .unwrap()
        .chars()
        .filter(|c| !matches!(c, '\n' | '\t'))
        .collect();
    assert!(
        xml.contains(
            "</AutoType><CustomData><Item><Key>KeePassXC-Browser Settings</Key><Value>\
             {\"Allow\":[\"example.com\",\"login.example.com\"],\"Deny\":[],\"Realm\":\"\"}\
             </Value></Item></CustomData><History/>"
        ),
        "{xml}"
    );
}
