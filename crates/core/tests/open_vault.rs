use keefast_core::{CompositeKey, OpenError, Vault};
use secrecy::ExposeSecret;

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASSXC_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx3.kdbx");
const PASSWORD: &str = "fixture-password";

fn open_fixture() -> Vault {
    Vault::open(KEEPASSXC_KDBX4, &CompositeKey::password(PASSWORD)).expect("fixture opens")
}

#[test]
fn opens_a_keepassxc_kdbx4_vault_file_with_its_master_password() {
    let vault = open_fixture();

    let root = vault.root();
    let group_names: Vec<&str> = root.groups().iter().map(|g| g.name()).collect();
    assert_eq!(group_names, ["Work", "Personal"]);

    let root_titles: Vec<&str> = root.entries().iter().map(|e| e.title()).collect();
    assert_eq!(root_titles, ["Root Entry"]);
}

#[test]
fn opens_a_keepassxc_kdbx3_1_vault_file() {
    let vault = Vault::open(KEEPASSXC_KDBX3, &CompositeKey::password(PASSWORD)).unwrap();

    let titles: Vec<&str> = vault.root().all_entries().map(|e| e.title()).collect();
    assert_eq!(titles, ["Root Entry", "GitHub", "Prod DB", "Email"]);
}

#[test]
fn exposes_entry_fields_with_the_password_kept_secret() {
    let vault = open_fixture();

    let work = &vault.root().groups()[0];
    let github = &work.entries()[0];
    assert_eq!(github.title(), "GitHub");
    assert_eq!(github.username(), "octocat");
    assert_eq!(github.url(), "https://github.com/login");
    assert_eq!(github.notes(), "Work account");
    assert_eq!(github.password().expose_secret(), "gh-secret");
    assert!(!format!("{github:?}").contains("gh-secret"));
}

#[test]
fn nested_groups_are_preserved() {
    let vault = open_fixture();

    let work = &vault.root().groups()[0];
    let servers = &work.groups()[0];
    assert_eq!(servers.name(), "Servers");
    assert_eq!(servers.entries()[0].title(), "Prod DB");
}

#[test]
fn groups_and_entries_can_be_looked_up_by_id() {
    let vault = open_fixture();

    let personal = &vault.root().groups()[1];
    let email = &personal.entries()[0];
    assert_eq!(
        vault.group(personal.id()).map(|g| g.name()),
        Some("Personal")
    );
    assert_eq!(vault.entry(email.id()).map(|e| e.title()), Some("Email"));
}

#[test]
fn all_entries_walks_the_group_and_its_descendants() {
    let vault = open_fixture();

    let titles = |group: &keefast_core::Group| -> Vec<String> {
        group.all_entries().map(|e| e.title().to_owned()).collect()
    };
    assert_eq!(titles(&vault.root().groups()[0]), ["GitHub", "Prod DB"]);
    assert_eq!(titles(vault.root()).len(), 4);
}

#[test]
fn a_wrong_master_password_is_reported_as_a_wrong_key() {
    let result = Vault::open(KEEPASSXC_KDBX4, &CompositeKey::password("not it"));

    assert!(matches!(result, Err(OpenError::WrongKey)));
}

#[test]
fn bytes_that_are_not_a_vault_file_are_reported_as_corrupt() {
    let result = Vault::open(
        b"definitely not a kdbx file",
        &CompositeKey::password(PASSWORD),
    );

    assert!(matches!(result, Err(OpenError::Corrupt(_))));
}

#[test]
fn a_truncated_vault_file_is_reported_as_corrupt() {
    let truncated = &KEEPASSXC_KDBX4[..KEEPASSXC_KDBX4.len() / 2];

    let result = Vault::open(truncated, &CompositeKey::password(PASSWORD));

    assert!(matches!(result, Err(OpenError::Corrupt(_))));
}

#[test]
fn a_kdbx3_1_vault_file_with_an_altered_header_is_reported_as_corrupt() {
    // Insert a comment field: decryption is unaffected, but the header no longer matches the
    // hash KDBX 3.1 keeps of it.
    let mut altered = KEEPASSXC_KDBX3[..12].to_vec();
    altered.extend_from_slice(&[1, 4, 0, b'n', b'o', b't', b'e']);
    altered.extend_from_slice(&KEEPASSXC_KDBX3[12..]);

    let result = Vault::open(&altered, &CompositeKey::password(PASSWORD));

    assert!(matches!(result, Err(OpenError::Corrupt(_))));
}
