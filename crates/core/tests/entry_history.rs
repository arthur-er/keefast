//! An Entry's history: restoring an earlier version makes it current and keeps the version it
//! replaces, and the history stays within the Vault's limits on its count and size.

use keefast_core::{
    CompositeKey, CustomField, EditError, EntryColor, EntryFields, EntryIcon, EntryId, Group,
    GroupId, Timestamp, Totp, Vault,
};
use secrecy::{ExposeSecret, SecretString};

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    open(&vault.save().expect("Vault saves"))
}

/// Times well after everything in the fixture, in order.
fn at(minute: i64) -> Timestamp {
    Timestamp::from_unix_seconds(1_900_000_000 + minute * 60)
}

fn fields(password: &str) -> EntryFields {
    EntryFields {
        title: "Bank".into(),
        username: "alice".into(),
        password: SecretString::from(password),
        url: "https://bank.example".into(),
        notes: String::new(),
        ..EntryFields::default()
    }
}

fn passwords(vault: &Vault, entry: EntryId) -> Vec<String> {
    vault
        .entry(entry)
        .unwrap()
        .history()
        .iter()
        .map(|version| version.password().expose_secret().to_owned())
        .collect()
}

fn password(vault: &Vault, entry: EntryId) -> String {
    vault
        .entry(entry)
        .unwrap()
        .password()
        .expose_secret()
        .to_owned()
}

fn group_named(vault: &Vault, name: &str) -> GroupId {
    fn search(group: &Group, name: &str) -> Option<GroupId> {
        if group.name() == name {
            return Some(group.id());
        }
        group.groups().iter().find_map(|child| search(child, name))
    }
    search(vault.root(), name).unwrap_or_else(|| panic!("no Group {name}"))
}

/// A Bank Entry with the passwords `pw-0` (its first version) to `pw-{edits}` (current).
fn edited_entry(vault: &mut Vault, edits: i64) -> EntryId {
    let id = vault
        .create_entry(vault.root().id(), &fields("pw-0"), at(0))
        .unwrap();
    for edit in 1..=edits {
        vault
            .update_entry(id, &fields(&format!("pw-{edit}")), at(edit))
            .unwrap();
    }
    id
}

#[test]
fn restoring_a_version_makes_it_current_and_keeps_the_replaced_one() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = edited_entry(&mut vault, 2);
    assert_eq!(passwords(&vault, id), ["pw-0", "pw-1"]);

    vault.restore_entry_version(id, 0, at(10)).unwrap();

    let vault = reopen(&vault);
    assert_eq!(password(&vault, id), "pw-0");
    assert_eq!(passwords(&vault, id), ["pw-0", "pw-1", "pw-2"]);
    let entry = vault.entry(id).unwrap();
    assert_eq!(entry.last_modified(), Some(at(10)));
    assert_eq!(entry.history()[2].last_modified(), Some(at(2)));
}

#[test]
fn a_restored_version_brings_back_its_custom_fields_and_totp() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let mut first = fields("pw-0");
    first.custom_fields = vec![CustomField {
        name: "PIN".into(),
        value: SecretString::from("1234"),
        protected: true,
    }];
    let id = vault
        .create_entry(vault.root().id(), &first, at(0))
        .unwrap();
    let totp = Totp::from_setup("JBSWY3DPEHPK3PXP", Default::default(), "6", "30").unwrap();
    vault.set_totp(id, Some(&totp), at(1)).unwrap();
    vault.update_entry(id, &fields("pw-2"), at(2)).unwrap();
    assert!(vault.entry(id).unwrap().custom_fields().is_empty());

    vault.restore_entry_version(id, 1, at(3)).unwrap();

    let vault = reopen(&vault);
    let entry = vault.entry(id).unwrap();
    assert_eq!(entry.password().expose_secret(), "pw-0");
    assert_eq!(entry.totp().unwrap().unwrap(), &totp);
    let pins: Vec<_> = entry
        .custom_fields()
        .iter()
        .filter(|field| field.name == "PIN")
        .collect();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].value.expose_secret(), "1234");
    assert!(pins[0].protected);
}

#[test]
fn a_restored_version_brings_back_its_tags_colour_icon_and_expiry() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let mut first = fields("pw-0");
    first.tags = vec!["work".into(), "bank".into()];
    first.color = Some(EntryColor::Green);
    first.icon = EntryIcon::Standard(7);
    first.expires = Some(at(500));
    let id = vault
        .create_entry(vault.root().id(), &first, at(0))
        .unwrap();
    vault.update_entry(id, &fields("pw-1"), at(1)).unwrap();
    let current = vault.entry(id).unwrap();
    assert!(current.tags().is_empty() && current.color().is_none() && current.expires().is_none());

    vault.restore_entry_version(id, 0, at(2)).unwrap();

    let vault = reopen(&vault);
    let entry = vault.entry(id).unwrap();
    assert_eq!(entry.tags(), ["work", "bank"]);
    assert_eq!(entry.color(), Some(EntryColor::Green));
    assert_eq!(entry.icon(), EntryIcon::Standard(7));
    assert_eq!(entry.expires(), Some(at(500)));
    // The version it replaced keeps its own (empty) settings in the history.
    assert!(entry.history().last().unwrap().tags().is_empty());
}

#[test]
fn restoring_does_not_move_the_entry_back() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = edited_entry(&mut vault, 1);
    let work = group_named(&vault, "Work");
    vault.move_entry(id, work, at(5)).unwrap();

    vault.restore_entry_version(id, 0, at(6)).unwrap();

    let vault = reopen(&vault);
    let work = vault.group(work).unwrap();
    assert!(work.entries().iter().any(|entry| entry.id() == id));
    assert_eq!(password(&vault, id), "pw-0");
}

#[test]
fn restoring_a_version_that_is_not_there_changes_nothing() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = edited_entry(&mut vault, 1);
    let before = vault.clone();

    let missing = vault.restore_entry_version(id, 1, at(5));

    assert_eq!(missing, Err(EditError::VersionNotFound));
    assert!(vault == before);
    let elsewhere = EntryId::from_hex("00000000000000000000000000000001").unwrap();
    assert_eq!(
        vault.restore_entry_version(elsewhere, 0, at(5)),
        Err(EditError::EntryNotFound)
    );
}

#[test]
fn restoring_keeps_the_history_within_its_max_items() {
    let mut vault = open(KEEPASSXC_KDBX4);
    // Ten versions in history, as many as the Vault keeps.
    let id = edited_entry(&mut vault, 10);
    assert_eq!(passwords(&vault, id).len(), 10);

    vault.restore_entry_version(id, 0, at(20)).unwrap();

    assert_eq!(password(&vault, id), "pw-0");
    let expected: Vec<String> = (1..=10).map(|edit| format!("pw-{edit}")).collect();
    assert_eq!(passwords(&vault, id), expected);
}

#[test]
fn history_is_trimmed_to_the_vaults_history_max_size() {
    let mut vault = open(KEEPASSXC_KDBX4);
    // About 1 MB a version: KeePass's default HistoryMaxSize of 6 MiB holds six of them.
    let notes = "n".repeat(1_000_000);
    let large = |password: &str| EntryFields {
        notes: notes.clone(),
        ..fields(password)
    };
    let id = vault
        .create_entry(vault.root().id(), &large("pw-0"), at(0))
        .unwrap();
    for edit in 1..=8 {
        vault
            .update_entry(id, &large(&format!("pw-{edit}")), at(edit))
            .unwrap();
    }

    let expected: Vec<String> = (2..=7).map(|edit| format!("pw-{edit}")).collect();
    assert_eq!(passwords(&reopen(&vault), id), expected);
}

#[test]
fn a_merge_keeps_the_history_within_its_max_size() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let notes = "n".repeat(1_000_000);
    let large = |password: &str| EntryFields {
        notes: notes.clone(),
        ..fields(password)
    };
    let id = vault
        .create_entry(vault.root().id(), &large("pw-0"), at(0))
        .unwrap();
    let mut theirs = vault.clone();
    // Each side edits four times: together, eight versions to keep.
    for edit in 1..=4 {
        vault
            .update_entry(id, &large(&format!("mine-{edit}")), at(edit))
            .unwrap();
        theirs
            .update_entry(id, &large(&format!("theirs-{edit}")), at(edit * 10))
            .unwrap();
    }

    vault.merge(&theirs);

    assert_eq!(password(&vault, id), "theirs-4");
    assert_eq!(passwords(&vault, id).len(), 6);
}
