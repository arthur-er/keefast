mod support;

use std::path::PathBuf;
use std::sync::Arc;

use keefast_core::{KeyFileError, OpenError};
use keefast_workspace::{Credentials, EntryScope, OpenVaultError, VaultLocation, Workspace};
use secrecy::SecretString;
use support::{FakeStorage, PASSWORD};

fn key_files() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/key-files")
}

fn vault() -> VaultLocation {
    VaultLocation::Local(PathBuf::from("/vaults/keyed.kdbx"))
}

fn workspace_with(fixture: &str) -> Workspace {
    let bytes = std::fs::read(key_files().join(fixture)).unwrap();
    Workspace::new(Arc::new(FakeStorage::default().with_file(&vault(), &bytes)))
}

fn credentials(password: &str, key_file: &str) -> Credentials {
    Credentials::new(
        SecretString::from(password),
        Some(key_files().join(key_file)),
    )
}

#[test]
fn a_vault_opens_with_its_password_and_key_file() {
    let mut workspace = workspace_with("keepassxc-xml-v2.kdbx");

    let id = workspace
        .open_vault(&vault(), &credentials(PASSWORD, "xml-v2.keyx"))
        .unwrap();

    assert_eq!(workspace.visible_entries(EntryScope::Vault(id)).len(), 1);
}

#[test]
fn with_a_key_file_an_empty_password_means_the_key_file_alone() {
    let mut workspace = workspace_with("keepassxc-raw-32.kdbx");

    let opened = workspace.open_vault(&vault(), &credentials("", "raw-32.key"));

    assert!(opened.is_ok());
}

#[test]
fn the_key_file_is_read_off_the_ui_thread_with_the_vault_file() {
    let mut workspace = workspace_with("keepass2-xml-v2-kdbx4.kdbx");

    let pending = workspace
        .begin_open(vault(), credentials(PASSWORD, "xml-v2.keyx"))
        .unwrap();
    let unlocked = std::thread::spawn(move || pending.unlock())
        .join()
        .unwrap()
        .unwrap();

    workspace.add_unlocked(unlocked);
    assert_eq!(workspace.vaults().count(), 1);
}

#[test]
fn a_missing_key_file_is_reported_as_such() {
    let mut workspace = workspace_with("keepassxc-xml-v2.kdbx");

    let result = workspace.open_vault(&vault(), &credentials(PASSWORD, "not-here.key"));

    assert!(matches!(result, Err(OpenVaultError::KeyFileNotFound(_))));
}

#[test]
fn a_damaged_key_file_is_reported_as_such() {
    let dir = tempfile::tempdir().unwrap();
    let damaged = dir.path().join("damaged.keyx");
    let original = std::fs::read_to_string(key_files().join("xml-v2.keyx")).unwrap();
    std::fs::write(&damaged, original.replace("Hash=\"", "Hash=\"0")).unwrap();
    let mut workspace = workspace_with("keepassxc-xml-v2.kdbx");

    let result = workspace.open_vault(
        &vault(),
        &Credentials::new(SecretString::from(PASSWORD), Some(damaged)),
    );

    assert!(matches!(
        result,
        Err(OpenVaultError::KeyFile(KeyFileError::Corrupt(_)))
    ));
}

#[test]
fn the_wrong_key_file_is_a_wrong_key() {
    let mut workspace = workspace_with("keepassxc-xml-v2.kdbx");

    let result = workspace.open_vault(&vault(), &credentials(PASSWORD, "any-file.txt"));

    assert!(matches!(
        result,
        Err(OpenVaultError::Vault(OpenError::WrongKey))
    ));
}
