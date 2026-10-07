use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use keefast_core::{CompositeKey, OpenError};
use keefast_workspace::{
    LocalStorage, OpenVaultError, StorageError, StorageProvider, VaultLocation, Workspace,
};

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";

/// An in-memory Storage provider holding Vault files by location.
#[derive(Default)]
struct FakeStorage {
    files: HashMap<VaultLocation, Vec<u8>>,
}

impl FakeStorage {
    fn with_file(mut self, location: &VaultLocation, bytes: &[u8]) -> Self {
        self.files.insert(location.clone(), bytes.to_vec());
        self
    }
}

impl StorageProvider for FakeStorage {
    fn read(&self, location: &VaultLocation) -> Result<Vec<u8>, StorageError> {
        self.files
            .get(location)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(location.clone()))
    }
}

fn personal_vault() -> VaultLocation {
    VaultLocation::Local(PathBuf::from("/vaults/personal.kdbx"))
}

fn workspace_with_fixture() -> Workspace {
    let storage = FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4);
    Workspace::new(Arc::new(storage))
}

fn titles<'a>(entries: impl IntoIterator<Item = &'a keefast_core::Entry>) -> Vec<&'a str> {
    entries.into_iter().map(|entry| entry.title()).collect()
}

#[test]
fn opening_a_vault_shows_every_entry_sorted_by_title() {
    let mut workspace = workspace_with_fixture();

    let id = workspace
        .open_vault(&personal_vault(), &CompositeKey::password(PASSWORD))
        .expect("vault opens");

    let entries = workspace.visible_entries(id, None);
    assert_eq!(
        titles(entries),
        ["Email", "GitHub", "Prod DB", "Root Entry"]
    );
}

#[test]
fn selecting_a_group_shows_its_entries_and_those_of_its_subgroups() {
    let mut workspace = workspace_with_fixture();
    let id = workspace
        .open_vault(&personal_vault(), &CompositeKey::password(PASSWORD))
        .unwrap();

    let work = workspace.vault(id).unwrap().vault().root().groups()[0].id();
    let entries = workspace.visible_entries(id, Some(work));

    assert_eq!(titles(entries), ["GitHub", "Prod DB"]);
}

#[test]
fn an_opened_vault_is_listed_with_its_location_and_display_name() {
    let mut workspace = workspace_with_fixture();
    let id = workspace
        .open_vault(&personal_vault(), &CompositeKey::password(PASSWORD))
        .unwrap();

    let open: Vec<_> = workspace.vaults().map(|vault| vault.id()).collect();
    assert_eq!(open, [id]);
    let vault = workspace.vault(id).unwrap();
    assert_eq!(vault.location(), &personal_vault());
    assert_eq!(vault.display_name(), "personal");
}

#[test]
fn several_vaults_can_be_open_at_once() {
    let work_vault = VaultLocation::Local(PathBuf::from("/vaults/work.kdbx"));
    let storage = FakeStorage::default()
        .with_file(&personal_vault(), KEEPASSXC_KDBX4)
        .with_file(&work_vault, KEEPASSXC_KDBX4);
    let mut workspace = Workspace::new(Arc::new(storage));
    let key = CompositeKey::password(PASSWORD);

    let personal = workspace.open_vault(&personal_vault(), &key).unwrap();
    let work = workspace.open_vault(&work_vault, &key).unwrap();

    assert_ne!(personal, work);
    assert_eq!(workspace.vaults().count(), 2);
}

#[test]
fn a_wrong_password_leaves_no_vault_open() {
    let mut workspace = workspace_with_fixture();

    let result = workspace.open_vault(&personal_vault(), &CompositeKey::password("wrong"));

    assert!(matches!(
        result,
        Err(OpenVaultError::Vault(OpenError::WrongKey))
    ));
    assert_eq!(workspace.vaults().count(), 0);
}

#[test]
fn a_missing_vault_file_is_a_storage_error() {
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default()));

    let result = workspace.open_vault(&personal_vault(), &CompositeKey::password(PASSWORD));

    assert!(matches!(
        result,
        Err(OpenVaultError::Storage(StorageError::NotFound(_)))
    ));
}

#[test]
fn unlocking_can_run_on_another_thread() {
    let mut workspace = workspace_with_fixture();

    let pending = workspace.begin_open(personal_vault(), CompositeKey::password(PASSWORD));
    let unlocked = std::thread::spawn(move || pending.unlock())
        .join()
        .unwrap()
        .unwrap();
    let id = workspace.add_unlocked(unlocked);

    assert_eq!(workspace.visible_entries(id, None).len(), 4);
}

#[test]
fn the_local_storage_provider_reads_vault_files_from_disk() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/keepassxc-kdbx4.kdbx");
    let mut workspace = Workspace::new(Arc::new(LocalStorage));

    let id = workspace
        .open_vault(
            &VaultLocation::Local(path),
            &CompositeKey::password(PASSWORD),
        )
        .unwrap();

    assert_eq!(workspace.visible_entries(id, None).len(), 4);
}

#[test]
fn the_local_storage_provider_reports_a_missing_file_as_not_found() {
    let location = VaultLocation::Local(PathBuf::from("/definitely/not/here.kdbx"));

    assert!(matches!(
        LocalStorage.read(&location),
        Err(StorageError::NotFound(_))
    ));
}
