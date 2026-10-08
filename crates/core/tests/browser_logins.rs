//! Logins captured in the browser, saved to a Vault: new ones go to the Vault's Browser group,
//! and changed ones update the existing Entry, keeping the old version in its history.

use keefast_core::{CapturedLogin, CompositeKey, EditError, GroupId, Timestamp, Vault};
use secrecy::{ExposeSecret, SecretString};

const BROWSER_FIXTURE: &[u8] = include_bytes!("../../../fixtures/keepassxc-browser.kdbx");
const PASSWORD: &str = "fixture-password";

fn open() -> Vault {
    Vault::open(BROWSER_FIXTURE, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    Vault::open(&vault.save().unwrap(), &CompositeKey::password(PASSWORD)).unwrap()
}

fn now() -> Timestamp {
    Timestamp::from_unix_seconds(1_791_414_922)
}

fn login(url: &str, username: &str, password: &str) -> CapturedLogin {
    CapturedLogin {
        url: url.into(),
        username: username.into(),
        password: SecretString::from(password.to_owned()),
    }
}

fn group_named(vault: &Vault, name: &str) -> GroupId {
    vault
        .root()
        .groups()
        .iter()
        .find(|group| group.name() == name)
        .unwrap_or_else(|| panic!("no Group {name:?}"))
        .id()
}

fn top_group_names(vault: &Vault) -> Vec<String> {
    let groups = vault.root().groups().iter();
    groups.map(|group| group.name().to_owned()).collect()
}

#[test]
fn a_vault_without_a_browser_group_gets_one_named_browser_when_a_login_is_saved() {
    let mut vault = open();
    assert_eq!(vault.browser_group(), None);

    let entry = vault
        .add_login(
            None,
            &login("https://shop.test/sign-in?next=/", "carol", "s3cret"),
            now(),
        )
        .unwrap();

    let browser = group_named(&vault, "Browser");
    assert_eq!(vault.browser_group(), Some(browser));
    let saved = vault.group(browser).unwrap().entries();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id(), entry);
    assert_eq!(saved[0].title(), "shop.test");
    assert_eq!(saved[0].url(), "https://shop.test/sign-in?next=/");
    assert_eq!(saved[0].username(), "carol");
    assert_eq!(saved[0].password().expose_secret(), "s3cret");

    // The Browser group is created once, and survives a save.
    vault
        .add_login(None, &login("https://shop.test/", "dave", "other"), now())
        .unwrap();
    let vault = reopen(&vault);
    let names = top_group_names(&vault);
    assert_eq!(names.iter().filter(|name| *name == "Browser").count(), 1);
    assert_eq!(vault.group(browser).unwrap().entries().len(), 2);
    assert_eq!(vault.browser_group(), Some(browser));
}

#[test]
fn an_existing_top_group_named_browser_is_the_browser_group() {
    let mut vault = open();
    let browser = vault
        .create_group(vault.root().id(), "Browser", now())
        .unwrap();

    assert_eq!(vault.browser_group(), Some(browser));
    vault
        .add_login(None, &login("https://shop.test/", "carol", "s3cret"), now())
        .unwrap();
    assert_eq!(vault.group(browser).unwrap().entries().len(), 1);
}

#[test]
fn the_browser_group_can_be_any_group_and_follows_it_when_renamed() {
    let mut vault = open();
    let chosen = group_named(&vault, "Hidden group");

    vault.set_browser_group(chosen).unwrap();
    vault.rename_group(chosen, "Captured", now()).unwrap();
    let mut vault = reopen(&vault);

    assert_eq!(vault.browser_group(), Some(chosen));
    vault
        .add_login(None, &login("https://shop.test/", "carol", "s3cret"), now())
        .unwrap();
    assert!(!top_group_names(&vault).contains(&"Browser".to_owned()));
    let entries = vault.group(chosen).unwrap().entries();
    assert!(entries.iter().any(|entry| entry.username() == "carol"));
}

#[test]
fn a_browser_group_that_was_deleted_is_made_again() {
    let mut vault = open();
    let chosen = vault
        .create_group(vault.root().id(), "Logins", now())
        .unwrap();
    vault.set_browser_group(chosen).unwrap();
    vault.delete_group(chosen, now()).unwrap();

    // In the Recycle Bin it is no place for new logins.
    assert_eq!(vault.browser_group(), None);
    vault
        .add_login(None, &login("https://shop.test/", "carol", "s3cret"), now())
        .unwrap();
    let browser = group_named(&vault, "Browser");
    assert_eq!(vault.browser_group(), Some(browser));
}

#[test]
fn a_login_can_be_saved_to_a_chosen_group() {
    let mut vault = open();
    let root = vault.root().id();

    vault
        .add_login(
            Some(root),
            &login("https://shop.test/", "carol", "s3cret"),
            now(),
        )
        .unwrap();

    assert!(
        vault
            .root()
            .entries()
            .iter()
            .any(|e| e.username() == "carol")
    );
    assert!(!top_group_names(&vault).contains(&"Browser".to_owned()));
    let mut elsewhere = open();
    let other = elsewhere
        .create_group(elsewhere.root().id(), "Elsewhere", now())
        .unwrap();
    assert_eq!(
        vault.add_login(Some(other), &login("https://a.test/", "", ""), now()),
        Err(EditError::GroupNotFound)
    );
}

#[test]
fn a_changed_password_updates_the_entry_and_keeps_the_old_version_in_history() {
    let mut vault = open();
    let example = vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == "Example")
        .unwrap()
        .id();
    let history_before = vault.entry(example).unwrap().history().len();

    vault
        .update_login(
            example,
            &login("https://example.com/login", "alice", "new-secret"),
            now(),
        )
        .unwrap();
    let vault = reopen(&vault);

    let entry = vault.entry(example).unwrap();
    assert_eq!(entry.title(), "Example");
    assert_eq!(entry.url(), "https://example.com");
    assert_eq!(entry.username(), "alice");
    assert_eq!(entry.password().expose_secret(), "new-secret");
    assert_eq!(entry.history().len(), history_before + 1);
    let old = entry.history().last().unwrap();
    assert_eq!(old.password().expose_secret(), "example-secret");
}
