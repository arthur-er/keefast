//! Unlocking with a key file, alone or with the master password, in every KeePass key file
//! format. The fixtures were written by KeePassXC (KDBX 3.1) and KeePass2 (KDBX 4.1).

use keefast_core::{CompositeKey, KeyFileError, OpenError, Vault};

const PASSWORD: &str = "fixture-password";

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("../../../fixtures/key-files/", $name)).as_slice()
    };
}

const XML_V1: &[u8] = fixture!("xml-v1.key");
const XML_V2: &[u8] = fixture!("xml-v2.keyx");
const RAW_32: &[u8] = fixture!("raw-32.key");
const HEX_64: &[u8] = fixture!("hex-64.key");
const ANY_FILE: &[u8] = fixture!("any-file.txt");

fn single_entry_title(vault: &Vault) -> &str {
    vault
        .root()
        .all_entries()
        .next()
        .expect("one Entry")
        .title()
}

#[test]
fn an_xml_v1_key_file_alone_unlocks() {
    let key = CompositeKey::key_file(XML_V1).unwrap();

    let vault = Vault::open(fixture!("keepassxc-xml-v1.kdbx"), &key).unwrap();

    assert_eq!(single_entry_title(&vault), "Key file Entry");
}

#[test]
fn an_xml_v2_key_file_with_the_master_password_unlocks() {
    let key = CompositeKey::password_and_key_file(PASSWORD, XML_V2).unwrap();

    let vault = Vault::open(fixture!("keepassxc-xml-v2.kdbx"), &key).unwrap();

    assert_eq!(single_entry_title(&vault), "Key file Entry");
}

#[test]
fn a_32_byte_raw_key_file_alone_unlocks() {
    let key = CompositeKey::key_file(RAW_32).unwrap();

    let vault = Vault::open(fixture!("keepassxc-raw-32.kdbx"), &key).unwrap();

    assert_eq!(single_entry_title(&vault), "Key file Entry");
}

#[test]
fn a_64_digit_hex_key_file_alone_unlocks() {
    let key = CompositeKey::key_file(HEX_64).unwrap();

    let vault = Vault::open(fixture!("keepassxc-hex-64.kdbx"), &key).unwrap();

    assert_eq!(single_entry_title(&vault), "Key file Entry");
}

#[test]
fn any_other_file_is_hashed_into_a_key() {
    let key = CompositeKey::password_and_key_file(PASSWORD, ANY_FILE).unwrap();

    let vault = Vault::open(fixture!("keepassxc-any-file.kdbx"), &key).unwrap();

    assert_eq!(single_entry_title(&vault), "Key file Entry");
}

#[test]
fn a_kdbx4_vault_file_unlocks_with_password_and_key_file() {
    let key = CompositeKey::password_and_key_file(PASSWORD, XML_V2).unwrap();

    let vault = Vault::open(fixture!("keepass2-xml-v2-kdbx4.kdbx"), &key).unwrap();

    let groups: Vec<&str> = vault.root().groups().iter().map(|g| g.name()).collect();
    assert_eq!(groups, ["Work", "Personal"]);
}

#[test]
fn a_vault_with_a_key_file_saves_and_reopens_with_the_same_key() {
    let key = CompositeKey::password_and_key_file(PASSWORD, XML_V2).unwrap();
    let vault = Vault::open(fixture!("keepassxc-xml-v2.kdbx"), &key).unwrap();

    let saved = vault.save().unwrap();

    let reopened = Vault::open(&saved, &key).unwrap();
    assert_eq!(single_entry_title(&reopened), "Key file Entry");
}

#[test]
fn the_password_alone_does_not_unlock_a_vault_that_needs_a_key_file() {
    let result = Vault::open(
        fixture!("keepassxc-xml-v2.kdbx"),
        &CompositeKey::password(PASSWORD),
    );

    assert!(matches!(result, Err(OpenError::WrongKey)));
}

#[test]
fn the_wrong_key_file_is_reported_as_a_wrong_key() {
    let key = CompositeKey::password_and_key_file(PASSWORD, ANY_FILE).unwrap();

    let kdbx3 = Vault::open(fixture!("keepassxc-xml-v2.kdbx"), &key);
    let kdbx4 = Vault::open(fixture!("keepass2-xml-v2-kdbx4.kdbx"), &key);

    assert!(matches!(kdbx3, Err(OpenError::WrongKey)));
    assert!(matches!(kdbx4, Err(OpenError::WrongKey)));
}

#[test]
fn the_key_file_without_its_password_is_reported_as_a_wrong_key() {
    let result = Vault::open(
        fixture!("keepassxc-xml-v2.kdbx"),
        &CompositeKey::key_file(XML_V2).unwrap(),
    );

    assert!(matches!(result, Err(OpenError::WrongKey)));
}

#[test]
fn an_xml_v2_key_file_whose_hash_does_not_match_is_corrupt() {
    let text = std::str::from_utf8(XML_V2).unwrap();
    let hash = text.split("Hash=\"").nth(1).unwrap().get(..8).unwrap();
    let tampered = text.replace(
        hash,
        if hash == "00000000" {
            "11111111"
        } else {
            "00000000"
        },
    );

    let result = CompositeKey::key_file(tampered.as_bytes());

    assert!(matches!(result, Err(KeyFileError::Corrupt(_))));
}

#[test]
fn an_xml_v2_key_file_with_data_that_is_not_hex_is_corrupt() {
    let key_file = br#"<?xml version="1.0" encoding="UTF-8"?>
<KeyFile><Meta><Version>2.0</Version></Meta><Key><Data>not hex at all</Data></Key></KeyFile>"#;

    let result = CompositeKey::key_file(key_file);

    assert!(matches!(result, Err(KeyFileError::Corrupt(_))));
}

#[test]
fn an_xml_key_file_of_an_unknown_version_is_unsupported() {
    let key_file = br#"<?xml version="1.0" encoding="UTF-8"?>
<KeyFile><Meta><Version>3.0</Version></Meta><Key><Data>AAAA</Data></Key></KeyFile>"#;

    let result = CompositeKey::key_file(key_file);

    assert!(matches!(result, Err(KeyFileError::Unsupported(_))));
}

#[test]
fn an_empty_password_with_a_key_file_is_not_the_key_file_alone() {
    let vault = fixture!("keepass2-empty-password-kdbx4.kdbx");

    let alone = Vault::open(vault, &CompositeKey::key_file(ANY_FILE).unwrap());
    let with_empty = Vault::open(
        vault,
        &CompositeKey::password_and_key_file("", ANY_FILE).unwrap(),
    );

    assert!(matches!(alone, Err(OpenError::WrongKey)));
    assert!(with_empty.is_ok());
}
