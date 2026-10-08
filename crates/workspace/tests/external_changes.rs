//! A Vault file changed elsewhere, by another device through a sync tool: the change is
//! noticed, Merged into the open Vault, the Merge saved and reported, and nothing is lost.

mod support;

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use keefast_core::{CompositeKey, EntryFields, EntryId, Timestamp, Vault};
use keefast_workspace::{
    LocalStorage, MergeReport, SaveStatus, SaveVaultError, StorageError, StorageProvider,
    UnlockSecrets, VaultId, VaultLocation, Workspace,
};
use secrecy::{ExposeSecret, SecretString};
use support::{FakeClock, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault, work_vault};

const START: u64 = 1_800_000_000;

struct Harness {
    storage: Arc<FakeStorage>,
    workspace: Workspace,
    vault: VaultId,
    /// The Vaults whose Vault files the Workspace was told changed.
    changed: Arc<Mutex<Vec<VaultId>>>,
}

fn harness() -> Harness {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let clock = Arc::new(FakeClock::at_unix_seconds(START));
    let mut workspace = Workspace::with_clock(storage.clone(), clock);
    let vault = workspace
        .open_vault(&personal_vault(), &secrets())
        .expect("fixture opens");
    let changed = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&changed);
    workspace.watch_vault_files(move |vault| sink.lock().unwrap().push(vault));
    Harness {
        storage,
        workspace,
        vault,
        changed,
    }
}

fn key() -> CompositeKey {
    CompositeKey::password(PASSWORD)
}

fn secrets() -> UnlockSecrets {
    UnlockSecrets::password(PASSWORD)
}

fn fields(title: &str, password: &str) -> EntryFields {
    EntryFields {
        title: title.into(),
        password: SecretString::from(password),
        ..Default::default()
    }
}

/// After everything the Workspace's clock records, as another device's clock might be.
fn elsewhere_later() -> Timestamp {
    Timestamp::from_unix_seconds(START as i64 + 600)
}

impl Harness {
    /// What another device has: the Vault file as the Storage provider holds it now.
    fn other_device(&self) -> Vault {
        Vault::open(&self.storage.file(&personal_vault()).unwrap(), &key()).unwrap()
    }

    /// Another device edits the Vault and its sync tool replaces the Vault file.
    fn edit_elsewhere(&self, edit: impl FnOnce(&mut Vault)) {
        let mut theirs = self.other_device();
        edit(&mut theirs);
        self.storage
            .change_elsewhere(&personal_vault(), &theirs.save().unwrap());
    }

    fn github(&self) -> EntryId {
        self.entry_titled("GitHub")
    }

    fn entry_titled(&self, title: &str) -> EntryId {
        let open = self.workspace.vault(self.vault).unwrap();
        open.vault()
            .root()
            .all_entries()
            .find(|entry| entry.title() == title)
            .unwrap_or_else(|| panic!("no Entry {title}"))
            .id()
    }

    fn has_entry_titled(&self, vault: &Vault, title: &str) -> bool {
        vault
            .root()
            .all_entries()
            .any(|entry| entry.title() == title)
    }

    /// Runs what the Workspace starts after a change notice, as the app does.
    fn handle_changes(&mut self) -> Vec<Result<(), SaveVaultError>> {
        for vault in self.changed.lock().unwrap().drain(..) {
            self.workspace.vault_file_changed(vault);
        }
        let saves = self.workspace.begin_autosaves();
        saves
            .into_iter()
            .map(|save| self.workspace.finish_save(save.run()))
            .collect()
    }

    fn password(&self, entry: EntryId) -> String {
        let entry = self.workspace.entry(self.vault, entry).unwrap();
        entry.password().expose_secret().to_owned()
    }
}

fn set_password(vault: &mut Vault, title: &str, password: &str, now: Timestamp) {
    let entry = vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap();
    let id = entry.id();
    let mut fields = EntryFields::from(entry);
    fields.password = SecretString::from(password);
    vault.update_entry(id, &fields, now).unwrap();
}

#[test]
fn a_change_elsewhere_is_noticed_merged_and_reported() {
    let mut h = harness();
    let github = h.github();

    h.edit_elsewhere(|theirs| {
        set_password(theirs, "GitHub", "changed-elsewhere", elsewhere_later())
    });

    assert_eq!(*h.changed.lock().unwrap(), [h.vault]);
    let results = h.handle_changes();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert_eq!(h.password(github), "changed-elsewhere");
    let merges = h.workspace.take_merges();
    assert!(matches!(
        merges.as_slice(),
        [MergeReport { vault, summary }] if *vault == h.vault && summary.entries_updated == 1
    ));
    assert!(h.workspace.take_merges().is_empty(), "reported once");
    assert_eq!(
        h.storage.writes(),
        0,
        "the Vault file had everything already"
    );
    assert_eq!(h.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn with_autosave_a_change_elsewhere_is_merged_with_unsaved_edits_and_the_merge_saved() {
    let mut h = harness();
    h.workspace.set_autosave(true);
    let root = h.workspace.vault(h.vault).unwrap().vault().root().id();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "made-here"))
        .unwrap();

    h.edit_elsewhere(|theirs| {
        set_password(theirs, "GitHub", "changed-elsewhere", elsewhere_later())
    });
    h.handle_changes();

    let saved = h.other_device();
    assert!(
        h.has_entry_titled(&saved, "Bank"),
        "the edit made here is saved"
    );
    let github = h.github();
    assert_eq!(
        saved.entry(github).unwrap().password().expose_secret(),
        "changed-elsewhere"
    );
    assert_eq!(h.password(github), "changed-elsewhere");
    assert_eq!(h.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn without_autosave_unsaved_edits_wait_for_a_save_after_a_change_elsewhere() {
    let mut h = harness();
    let root = h.workspace.vault(h.vault).unwrap().vault().root().id();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "made-here"))
        .unwrap();

    h.edit_elsewhere(|theirs| {
        set_password(theirs, "GitHub", "changed-elsewhere", elsewhere_later())
    });
    let writes = h.storage.writes();
    h.handle_changes();

    let github = h.github();
    assert_eq!(h.password(github), "changed-elsewhere", "Merged in");
    assert_eq!(h.storage.writes(), writes, "but nothing saved unasked");
    assert_eq!(h.workspace.save_status(), SaveStatus::Unsaved);

    h.workspace.save_vault(h.vault).unwrap();
    let saved = h.other_device();
    assert!(h.has_entry_titled(&saved, "Bank"));
    assert_eq!(
        saved.entry(github).unwrap().password().expose_secret(),
        "changed-elsewhere"
    );
    assert_eq!(h.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn a_vault_file_missing_saved_changes_gets_them_back_after_the_merge() {
    let mut h = harness();
    let root = h.workspace.vault(h.vault).unwrap().vault().root().id();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "made-here"))
        .unwrap();
    h.workspace.save_vault(h.vault).unwrap();

    // A sync tool puts back an older copy of the Vault file.
    h.storage
        .change_elsewhere(&personal_vault(), KEEPASSXC_KDBX4);
    h.handle_changes();

    assert!(h.has_entry_titled(&h.other_device(), "Bank"));
    assert_eq!(h.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn the_vaults_own_saves_are_not_taken_for_changes_elsewhere() {
    let mut h = harness();
    let root = h.workspace.vault(h.vault).unwrap().vault().root().id();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "made-here"))
        .unwrap();
    h.workspace.save_vault(h.vault).unwrap();
    let writes = h.storage.writes();

    // A file watcher sees keefast's own write too.
    h.workspace.vault_file_changed(h.vault);
    let results = h.handle_changes();

    assert_eq!(results.len(), 1, "the Vault file is checked");
    assert_eq!(h.storage.writes(), writes, "and left alone");
    assert!(h.workspace.take_merges().is_empty());
}

#[test]
fn a_save_never_overwrites_a_change_made_elsewhere_meanwhile() {
    let mut h = harness();
    let root = h.workspace.vault(h.vault).unwrap().vault().root().id();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "made-here"))
        .unwrap();
    let mut theirs = h.other_device();
    set_password(
        &mut theirs,
        "GitHub",
        "changed-elsewhere",
        elsewhere_later(),
    );
    h.storage
        .change_before_next_write(&personal_vault(), &theirs.save().unwrap());

    h.workspace.save_vault(h.vault).unwrap();

    let saved = h.other_device();
    assert!(h.has_entry_titled(&saved, "Bank"));
    let github = h.github();
    assert_eq!(
        saved.entry(github).unwrap().password().expose_secret(),
        "changed-elsewhere"
    );
    assert_eq!(h.password(github), "changed-elsewhere");
    assert_eq!(h.workspace.take_merges().len(), 1);
}

#[test]
fn a_change_noticed_during_a_save_is_checked_after_it() {
    let mut h = harness();
    let root = h.workspace.vault(h.vault).unwrap().vault().root().id();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "made-here"))
        .unwrap();
    let save = h.workspace.begin_save(h.vault).unwrap();
    let finished = save.run();

    h.edit_elsewhere(|theirs| {
        set_password(theirs, "GitHub", "changed-elsewhere", elsewhere_later())
    });
    for vault in h.changed.lock().unwrap().drain(..) {
        h.workspace.vault_file_changed(vault);
    }
    h.workspace.finish_save(finished).unwrap();
    let results = h.handle_changes();

    assert_eq!(results.len(), 1);
    assert_eq!(h.password(h.github()), "changed-elsewhere");
}

#[test]
fn a_vault_file_that_cant_be_merged_is_not_overwritten() {
    let mut h = harness();
    let root = h.workspace.vault(h.vault).unwrap().vault().root().id();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "made-here"))
        .unwrap();
    h.storage
        .change_elsewhere(&personal_vault(), b"half-written by a sync tool");

    let results = h.handle_changes();
    let save = h.workspace.save_vault(h.vault);

    assert!(matches!(
        results.as_slice(),
        [Err(SaveVaultError::Merge(_))]
    ));
    assert!(matches!(save, Err(SaveVaultError::Merge(_))));
    assert_eq!(
        h.storage.file(&personal_vault()).unwrap(),
        b"half-written by a sync tool"
    );
    assert_eq!(h.workspace.save_status(), SaveStatus::Failed);

    // Once the sync tool is done, the Merge goes through, and saving does too.
    h.storage
        .change_elsewhere(&personal_vault(), KEEPASSXC_KDBX4);
    let results = h.handle_changes();
    assert!(matches!(results.as_slice(), [Ok(())]));
    h.workspace.save_vault(h.vault).unwrap();
    assert!(h.has_entry_titled(&h.other_device(), "Bank"));
    assert_eq!(h.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn open_vaults_and_those_opened_later_are_watched() {
    let storage = Arc::new(
        FakeStorage::default()
            .with_file(&personal_vault(), KEEPASSXC_KDBX4)
            .with_file(&work_vault(), KEEPASSXC_KDBX4),
    );
    let mut workspace = Workspace::new(storage.clone());
    workspace.open_vault(&personal_vault(), &secrets()).unwrap();
    let changed = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&changed);
    workspace.watch_vault_files(move |vault| sink.lock().unwrap().push(vault));

    let work = workspace.open_vault(&work_vault(), &secrets()).unwrap();
    storage.change_elsewhere(&work_vault(), KEEPASSXC_KDBX4);

    assert_eq!(storage.watching(&personal_vault()), 1);
    assert_eq!(*changed.lock().unwrap(), [work]);
}

#[test]
fn the_local_storage_provider_refuses_to_overwrite_a_vault_file_changed_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let location = VaultLocation::Local(dir.path().join("personal.kdbx"));
    let VaultLocation::Local(path) = &location;
    std::fs::write(path, b"first").unwrap();
    let seen = LocalStorage.read(&location).unwrap().revision;
    std::fs::write(path, b"changed elsewhere").unwrap();

    let result = LocalStorage.write(&location, b"ours", Some(&seen));

    assert!(matches!(result, Err(StorageError::Conflict)));
    assert_eq!(std::fs::read(path).unwrap(), b"changed elsewhere");
    let current = LocalStorage.read(&location).unwrap().revision;
    let written = LocalStorage
        .write(&location, b"ours", Some(&current))
        .unwrap();
    assert_eq!(LocalStorage.read(&location).unwrap().revision, written);
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["personal.kdbx"], "no temporary file is left");
}

#[test]
fn the_local_storage_provider_sees_a_vault_file_replaced_elsewhere_but_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let location = VaultLocation::Local(dir.path().join("personal.kdbx"));
    let VaultLocation::Local(path) = &location;
    std::fs::write(path, b"first").unwrap();
    std::fs::write(dir.path().join("unrelated.txt"), b"x").unwrap();
    let (sender, changes) = mpsc::channel();
    let sender = Mutex::new(sender);
    let _watch = LocalStorage
        .watch(
            &location,
            Arc::new(move || {
                let _ = sender.lock().unwrap().send(());
            }),
        )
        .expect("local Vault files can be watched");

    LocalStorage.read(&location).unwrap();
    std::fs::write(dir.path().join("unrelated.txt"), b"y").unwrap();
    assert!(
        changes.recv_timeout(Duration::from_millis(300)).is_err(),
        "reading the Vault file or changing another file is no change"
    );

    // As sync tools save: a temporary file renamed over the Vault file.
    let temp = dir.path().join(".personal.kdbx.syncthing.tmp");
    std::fs::write(&temp, b"second").unwrap();
    std::fs::rename(&temp, path).unwrap();
    assert!(changes.recv_timeout(Duration::from_secs(5)).is_ok());
}
