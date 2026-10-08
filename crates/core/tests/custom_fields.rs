//! Custom fields: the fields of an Entry besides the standard five, some of them protected.
//! They must survive edits, saves and reopens with their protection, and read the same in an
//! independent KDBX reader.

use keefast_core::{CompositeKey, CustomField, EditError, Entry, EntryFields, Timestamp, Vault};
use keepass::{Database, DatabaseKey};
use secrecy::{ExposeSecret, SecretString};

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASS2_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx");
const KEEPASS2_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx3.kdbx");
const KEEWEB_KDBX4: &[u8] = include_bytes!("../../../fixtures/keeweb-kdbx4.kdbx");
const BROWSER: &[u8] = include_bytes!("../../../fixtures/keepassxc-browser.kdbx");
const PASSWORD: &str = "fixture-password";

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    open(&vault.save().expect("Vault saves"))
}

fn independent(vault: &Vault) -> Database {
    let bytes = vault.save().unwrap();
    Database::parse(&bytes, DatabaseKey::new().with_password(PASSWORD)).expect("keepass reads")
}

fn now() -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_000)
}

fn later() -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_600)
}

fn field(name: &str, value: &str, protected: bool) -> CustomField {
    CustomField {
        name: name.into(),
        value: SecretString::from(value),
        protected,
    }
}

fn with_fields(custom_fields: Vec<CustomField>) -> EntryFields {
    EntryFields {
        title: "Bank".into(),
        username: "alice".into(),
        password: SecretString::from("bank-pw"),
        url: "https://bank.example".into(),
        notes: String::new(),
        custom_fields,
    }
}

fn entry_titled<'a>(vault: &'a Vault, title: &str) -> &'a Entry {
    vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap_or_else(|| panic!("no Entry {title}"))
}

/// Each custom field as (name, value, protected), in order.
fn described(entry: &Entry) -> Vec<(String, String, bool)> {
    entry
        .custom_fields()
        .iter()
        .map(|field| {
            let value = field.value.expose_secret().to_owned();
            (field.name.clone(), value, field.protected)
        })
        .collect()
}

fn owned(fields: &[(&str, &str, bool)]) -> Vec<(String, String, bool)> {
    fields
        .iter()
        .map(|&(name, value, protected)| (name.into(), value.into(), protected))
        .collect()
}

#[test]
fn custom_fields_from_other_clients_are_read_with_their_protection() {
    for bytes in [KEEPASS2_KDBX4, KEEPASS2_KDBX3] {
        let vault = open(bytes);
        let github = entry_titled(&vault, "GitHub");
        assert_eq!(
            described(github),
            owned(&[("Recovery question", "first-pet", true)])
        );
    }

    // KeePassXC lists TOTP and extra URL fields with the other attributes, and so does keefast.
    let keeweb = open(KEEWEB_KDBX4);
    let fields = described(entry_titled(&keeweb, "GitHub"));
    assert!(
        fields
            .iter()
            .any(|(name, _, protected)| name == "otp" && *protected)
    );
    let browser = open(BROWSER);
    let names: Vec<String> = described(entry_titled(&browser, "Example"))
        .into_iter()
        .map(|(name, ..)| name)
        .collect();
    assert!(names.contains(&"KP2A_URL".to_owned()), "{names:?}");
    assert!(names.contains(&"KP2A_URL_1".to_owned()), "{names:?}");
}

#[test]
fn the_standard_fields_are_not_custom_fields() {
    let vault = open(KEEPASSXC_KDBX4);
    for entry in vault.root().all_entries() {
        for field in entry.custom_fields() {
            assert!(
                !["Title", "UserName", "Password", "URL", "Notes"].contains(&field.name.as_str()),
                "{}",
                field.name
            );
        }
    }
}

#[test]
fn custom_fields_of_a_new_entry_survive_a_save_with_their_protection() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let fields = with_fields(vec![
        field("PIN", "1234", true),
        field("Account number", "NL00 BANK 0123", false),
    ]);

    let id = vault
        .create_entry(vault.root().id(), &fields, now())
        .unwrap();

    let reopened = reopen(&vault);
    assert_eq!(
        described(reopened.entry(id).unwrap()),
        owned(&[
            ("PIN", "1234", true),
            ("Account number", "NL00 BANK 0123", false)
        ])
    );
    let database = independent(&vault);
    let bank = database
        .iter_all_entries()
        .find(|entry| entry.get_title() == Some("Bank"))
        .unwrap();
    assert!(bank.fields["PIN"].is_protected());
    assert_eq!(bank.get("PIN"), Some("1234"));
    assert!(!bank.fields["Account number"].is_protected());
    assert_eq!(bank.get("Account number"), Some("NL00 BANK 0123"));
}

#[test]
fn custom_fields_are_added_edited_protected_and_removed_keeping_the_old_version_in_history() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = vault
        .create_entry(
            vault.root().id(),
            &with_fields(vec![
                field("PIN", "1234", false),
                field("Doomed", "gone soon", false),
                field("Answer", "blue", true),
            ]),
            now(),
        )
        .unwrap();

    vault
        .update_entry(
            id,
            &with_fields(vec![
                field("PIN", "9876", true),
                field("Answer", "blue", false),
                field("Added", "new", false),
            ]),
            later(),
        )
        .unwrap();

    let vault = reopen(&vault);
    let entry = vault.entry(id).unwrap();
    assert_eq!(
        described(entry),
        owned(&[
            ("PIN", "9876", true),
            ("Answer", "blue", false),
            ("Added", "new", false)
        ])
    );
    assert_eq!(entry.last_modified(), Some(later()));
    let [previous] = entry.history() else {
        panic!("expected one history entry");
    };
    assert_eq!(
        described(previous),
        owned(&[
            ("PIN", "1234", false),
            ("Doomed", "gone soon", false),
            ("Answer", "blue", true)
        ])
    );
}

#[test]
fn editing_other_fields_keeps_a_protected_custom_field_protected() {
    for bytes in [KEEPASS2_KDBX4, KEEPASS2_KDBX3] {
        let mut vault = open(bytes);
        let github = entry_titled(&vault, "GitHub");
        let id = github.id();
        let mut fields = EntryFields::from(github);
        fields.username = "renamed".into();

        vault.update_entry(id, &fields, now()).unwrap();

        let database = independent(&vault);
        let github = database
            .iter_all_entries()
            .find(|entry| entry.get_title() == Some("GitHub"))
            .unwrap();
        assert_eq!(github.get_username(), Some("renamed"));
        assert!(github.fields["Recovery question"].is_protected());
        assert_eq!(github.get("Recovery question"), Some("first-pet"));
    }
}

#[test]
fn editing_other_fields_keeps_keewebs_protected_totp_field_protected() {
    let mut vault = open(KEEWEB_KDBX4);
    let github = entry_titled(&vault, "GitHub");
    let id = github.id();
    let otp = github
        .custom_fields()
        .iter()
        .find(|field| field.name == "otp")
        .map(|field| field.value.expose_secret().to_owned())
        .unwrap();
    let mut fields = EntryFields::from(github);
    fields.notes = "edited".into();

    vault.update_entry(id, &fields, now()).unwrap();

    let database = independent(&vault);
    let github = database
        .iter_all_entries()
        .find(|entry| entry.get_title() == Some("GitHub"))
        .unwrap();
    assert!(github.fields["otp"].is_protected());
    assert_eq!(github.get("otp"), Some(otp.as_str()));
}

#[test]
fn custom_fields_are_kept_in_the_order_given_so_a_renamed_field_keeps_its_place() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = vault
        .create_entry(
            vault.root().id(),
            &with_fields(vec![
                field("First", "1", false),
                field("Second", "2", true),
                field("Third", "3", false),
            ]),
            now(),
        )
        .unwrap();

    vault
        .update_entry(
            id,
            &with_fields(vec![
                field("Third", "3", false),
                field("Renamed", "2", true),
                field("First", "1", false),
            ]),
            later(),
        )
        .unwrap();

    assert_eq!(
        described(reopen(&vault).entry(id).unwrap()),
        owned(&[
            ("Third", "3", false),
            ("Renamed", "2", true),
            ("First", "1", false)
        ])
    );
}

#[test]
fn saving_unchanged_custom_fields_records_no_history() {
    let mut vault = open(KEEPASS2_KDBX4);
    let github = entry_titled(&vault, "GitHub");
    let (id, history) = (github.id(), github.history().len());
    let fields = EntryFields::from(github);

    vault.update_entry(id, &fields, later()).unwrap();

    assert_eq!(vault.entry(id).unwrap().history().len(), history);
}

#[test]
fn a_custom_field_needs_a_name_of_its_own() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let email = entry_titled(&vault, "Email").id();
    let root = vault.root().id();
    let before = vault.clone();

    for (fields, error) in [
        (vec![field("", "x", false)], EditError::FieldNameEmpty),
        (vec![field("  ", "x", false)], EditError::FieldNameEmpty),
        (
            vec![field("PIN", "1", false), field("PIN", "2", true)],
            EditError::FieldNameTaken,
        ),
        (
            vec![field("Password", "x", false)],
            EditError::FieldNameTaken,
        ),
        (
            vec![field("UserName", "x", false)],
            EditError::FieldNameTaken,
        ),
        (
            vec![field("Password ", "x", false)],
            EditError::FieldNameTaken,
        ),
        (
            vec![field("PIN", "1", false), field(" PIN", "2", false)],
            EditError::FieldNameTaken,
        ),
    ] {
        let fields = with_fields(fields);
        assert_eq!(
            vault.update_entry(email, &fields, now()),
            Err(error.clone())
        );
        assert_eq!(vault.create_entry(root, &fields, now()), Err(error));
    }
    assert!(vault == before);
}

#[test]
fn custom_field_values_are_never_printed() {
    let secret = field("PIN", "do-not-print", true);
    assert!(!format!("{secret:?}").contains("do-not-print"));

    let mut vault = open(KEEPASSXC_KDBX4);
    let id = vault
        .create_entry(vault.root().id(), &with_fields(vec![secret]), now())
        .unwrap();
    assert!(!format!("{:?}", vault.entry(id).unwrap()).contains("do-not-print"));
}
