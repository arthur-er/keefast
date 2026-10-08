//! Pairings: stored in the Vault's custom data under KeePassXC's keys, so they travel with the
//! Vault file and work in KeePassXC too.

use keefast_core::{CompositeKey, EditError, Pairing, Timestamp, Vault};
use keepass::{Database, DatabaseKey};

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASSXC_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx3.kdbx");
const PASSWORD: &str = "fixture-password";
const ID_KEY: &str = "q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=";

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    open(&vault.save().expect("Vault saves"))
}

fn now() -> Timestamp {
    // 2026-10-07 23:15:22 UTC.
    Timestamp::from_unix_seconds(1_791_414_922)
}

fn independent_xml(vault: &Vault) -> String {
    let bytes = vault.save().unwrap();
    let xml = Database::get_xml(&mut &bytes[..], DatabaseKey::new().with_password(PASSWORD))
        .expect("keepass decrypts");
    String::from_utf8(xml).unwrap()
}

#[test]
fn the_browser_hash_is_keepassxcs_database_hash() {
    let vault = open(KEEPASSXC_KDBX4);

    // SHA-256 of the root Group's UUID in hex, as KeePassXC's BrowserService computes it.
    assert_eq!(
        vault.browser_hash(),
        "e664e0d1a663bc5bf6293d1955671b5b1c87d5d0cef01daf1fdbe87e477ed419"
    );
}

#[test]
fn a_vault_without_pairings_has_none() {
    assert_eq!(open(KEEPASSXC_KDBX3).pairings(), []);
}

#[test]
fn pairings_written_by_keepassxc_are_read() {
    // The corpus script stores one under KeePassXC's key, with a made-up value.
    let names: Vec<String> = open(KEEPASSXC_KDBX4)
        .pairings()
        .into_iter()
        .map(|pairing| pairing.name)
        .collect();
    assert_eq!(names, ["fixture-browser"]);
}

#[test]
fn a_pairing_survives_a_save_under_keepassxcs_custom_data_keys() {
    let mut vault = open(KEEPASSXC_KDBX4);

    vault.add_pairing("Work laptop", ID_KEY, now()).unwrap();

    let reopened = reopen(&vault);
    assert_eq!(
        reopened.pairings()[1..],
        [Pairing {
            name: "Work laptop".into(),
            key: ID_KEY.into(),
        }]
    );
    assert!(reopened.is_paired_with("Work laptop", ID_KEY));
    let xml: String = independent_xml(&vault)
        .chars()
        .filter(|c| !matches!(c, '\n' | '\t'))
        .collect();
    assert!(xml.contains(&format!(
        "<Item><Key>KPXC_BROWSER_Work laptop</Key><Value>{ID_KEY}</Value></Item>"
    )));
    assert!(xml.contains(
        "<Item><Key>_CREATED_Work laptop</Key><Value>2026-10-07T23:15:22Z</Value></Item>"
    ));
}

#[test]
fn a_kdbx3_vault_can_be_paired_too() {
    let mut vault = open(KEEPASSXC_KDBX3);

    vault.add_pairing("Home", ID_KEY, now()).unwrap();

    assert!(reopen(&vault).is_paired_with("Home", ID_KEY));
}

#[test]
fn pairing_needs_a_matching_name_and_key() {
    let mut vault = open(KEEPASSXC_KDBX4);
    vault.add_pairing("Home", ID_KEY, now()).unwrap();

    assert!(!vault.is_paired_with("Work", ID_KEY));
    assert!(!vault.is_paired_with("Home", "c29tZSBvdGhlciBrZXk="));
}

#[test]
fn pairing_names_are_trimmed_unique_and_not_empty() {
    let mut vault = open(KEEPASSXC_KDBX4);
    vault.add_pairing("  Home ", ID_KEY, now()).unwrap();

    assert_eq!(
        vault.add_pairing("Home", "b3RoZXI=", now()),
        Err(EditError::PairingNameTaken)
    );
    assert_eq!(
        vault.add_pairing("   ", "b3RoZXI=", now()),
        Err(EditError::PairingNameEmpty)
    );
    assert_eq!(vault.pairings().len(), 2);
    assert_eq!(vault.pairings()[1].name, "Home");
}

#[test]
fn several_pairings_are_kept_in_order() {
    let mut vault = open(KEEPASSXC_KDBX4);
    vault.add_pairing("Home", ID_KEY, now()).unwrap();
    vault.add_pairing("Work", "b3RoZXI=", now()).unwrap();

    let names: Vec<String> = reopen(&vault)
        .pairings()
        .into_iter()
        .map(|pairing| pairing.name)
        .collect();
    assert_eq!(names, ["fixture-browser", "Home", "Work"]);
}
