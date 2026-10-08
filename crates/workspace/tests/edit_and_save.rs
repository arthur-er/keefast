//! Editing a Vault through the Workspace and saving it: edit → save → reopen, with the
//! Storage provider and clock faked.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{CompositeKey, EntryFields, Timestamp, Vault};
use keefast_workspace::{
    EditVaultError, EntryScope, LocalStorage, SaveStatus, SaveVaultError, StorageProvider, VaultId,
    VaultLocation, Workspace,
};
use secrecy::{ExposeSecret, SecretString};
use support::{FakeClock, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault};

const START: u64 = 1_800_000_000;

struct Harness {
    storage: Arc<FakeStorage>,
    clock: Arc<FakeClock>,
    workspace: Workspace,
    vault: VaultId,
}

fn harness() -> Harness {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let clock = Arc::new(FakeClock::at_unix_seconds(START));
    let mut workspace = Workspace::with_clock(storage.clone(), clock.clone());
    let vault = workspace
        .open_vault(&personal_vault(), &key())
        .expect("fixture opens");
    Harness {
        storage,
        clock,
        workspace,
        vault,
    }
}

fn key() -> CompositeKey {
    CompositeKey::password(PASSWORD)
}

fn fields(title: &str, password: &str) -> EntryFields {
    EntryFields {
        title: title.into(),
        username: "alice".into(),
        password: SecretString::from(password),
        url: "https://bank.example".into(),
        notes: String::new(),
    }
}

impl Harness {
    /// Opens what the Storage provider holds now, as a fresh Workspace would.
    fn reopen(&self) -> Vault {
        let bytes = self.storage.file(&personal_vault()).unwrap();
        Vault::open(&bytes, &key()).expect("saved Vault file opens")
    }

    fn root(&self) -> keefast_core::GroupId {
        self.workspace
            .vault(self.vault)
            .unwrap()
            .vault()
            .root()
            .id()
    }

    fn group(&self, name: &str) -> keefast_core::GroupId {
        let open = self.workspace.vault(self.vault).unwrap();
        let mut groups = vec![open.vault().root()];
        while let Some(group) = groups.pop() {
            if group.name() == name {
                return group.id();
            }
            groups.extend(group.groups());
        }
        panic!("no Group {name}");
    }

    fn unsaved(&self) -> bool {
        self.workspace
            .vault(self.vault)
            .unwrap()
            .has_unsaved_changes()
    }
}

#[test]
fn a_created_entry_is_in_the_vault_file_after_saving_and_reopening() {
    let mut h = harness();
    let work = h.group("Work");

    let id = h
        .workspace
        .create_entry(h.vault, work, &fields("Bank", "bank-pw"))
        .unwrap();
    assert!(h.unsaved());
    h.workspace.save_vault(h.vault).unwrap();

    assert!(!h.unsaved());
    let reopened = h.reopen();
    let entry = reopened.entry(id).expect("Entry was saved");
    assert_eq!(entry.title(), "Bank");
    assert_eq!(entry.password().expose_secret(), "bank-pw");
    assert_eq!(
        entry.last_modified(),
        Some(Timestamp::from_unix_seconds(START as i64))
    );
}

#[test]
fn edits_are_not_in_the_vault_file_until_saved() {
    let mut h = harness();
    let root = h.root();

    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "pw"))
        .unwrap();

    assert_eq!(h.storage.writes(), 0);
    assert_eq!(h.reopen().root().all_entries().count(), 4);
}

#[test]
fn an_edited_entry_keeps_its_previous_version_in_history_after_reopening() {
    let mut h = harness();
    let root = h.root();
    let id = h
        .workspace
        .create_entry(h.vault, root, &fields("Bank", "first"))
        .unwrap();
    h.clock.advance(Duration::from_secs(60));

    h.workspace
        .update_entry(h.vault, id, &fields("Bank", "second"))
        .unwrap();
    h.workspace.save_vault(h.vault).unwrap();

    let reopened = h.reopen();
    let entry = reopened.entry(id).unwrap();
    assert_eq!(entry.password().expose_secret(), "second");
    assert_eq!(
        entry.last_modified(),
        Some(Timestamp::from_unix_seconds(START as i64 + 60))
    );
    let history: Vec<&str> = entry
        .history()
        .iter()
        .map(|old| old.password().expose_secret())
        .collect();
    assert_eq!(history, ["first"]);
}

#[test]
fn groups_and_moves_survive_saving_and_reopening() {
    let mut h = harness();
    let root = h.root();
    let archive = h.workspace.create_group(h.vault, root, "Archive").unwrap();
    let old = h.workspace.create_group(h.vault, archive, "Old").unwrap();
    let servers = h.group("Servers");
    let email = h.workspace.visible_entries(EntryScope::Vault(h.vault))[0]
        .entry
        .id();

    h.workspace.rename_group(h.vault, old, "Older").unwrap();
    h.workspace.move_group(h.vault, servers, old).unwrap();
    h.workspace.move_entry(h.vault, email, archive).unwrap();
    h.workspace.save_vault(h.vault).unwrap();

    let reopened = h.reopen();
    let archive = reopened.group(archive).unwrap();
    assert_eq!(archive.entries()[0].title(), "Email");
    let older = &archive.groups()[0];
    assert_eq!(older.name(), "Older");
    assert_eq!(older.groups()[0].name(), "Servers");
    assert_eq!(older.groups()[0].entries()[0].title(), "Prod DB");
}

#[test]
fn deleted_entries_and_groups_are_in_the_recycle_bin_after_reopening() {
    let mut h = harness();
    let personal = h.group("Personal");
    let github = h
        .workspace
        .visible_entries(EntryScope::Vault(h.vault))
        .into_iter()
        .find(|listed| listed.entry.title() == "GitHub")
        .unwrap()
        .entry
        .id();

    h.workspace.delete_entry(h.vault, github).unwrap();
    h.workspace.delete_group(h.vault, personal).unwrap();
    h.workspace.save_vault(h.vault).unwrap();

    let reopened = h.reopen();
    let bin = reopened.group(reopened.recycle_bin().unwrap()).unwrap();
    assert_eq!(bin.entries()[0].title(), "GitHub");
    assert_eq!(bin.groups()[0].name(), "Personal");
}

#[test]
fn edits_to_a_vault_that_is_not_open_are_rejected() {
    let mut h = harness();
    let root = h.root();
    let mut other = Workspace::new(Arc::new(FakeStorage::default()));

    let result = other.create_group(h.vault, root, "Nope");

    assert_eq!(result, Err(EditVaultError::VaultNotOpen));
    assert!(!h.unsaved());
    assert!(matches!(
        other.save_vault(h.vault),
        Err(SaveVaultError::VaultNotOpen)
    ));
    // The harness Workspace is untouched too.
    h.workspace.save_vault(h.vault).unwrap();
}

#[test]
fn a_failed_save_keeps_the_changes_unsaved_and_the_vault_file_intact() {
    let mut h = harness();
    let root = h.root();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "pw"))
        .unwrap();
    h.storage.fail_writes(true);

    let result = h.workspace.save_vault(h.vault);

    assert!(matches!(result, Err(SaveVaultError::Storage(_))));
    assert!(h.unsaved());
    assert!(h.workspace.vault(h.vault).unwrap().save_failed());
    assert_eq!(h.storage.file(&personal_vault()).unwrap(), KEEPASSXC_KDBX4);

    h.storage.fail_writes(false);
    h.workspace.save_vault(h.vault).unwrap();
    assert!(!h.workspace.vault(h.vault).unwrap().save_failed());
    assert_eq!(h.reopen().root().all_entries().count(), 5);
}

#[test]
fn saving_can_run_on_another_thread_while_editing_goes_on() {
    let mut h = harness();
    let root = h.root();
    let first = h
        .workspace
        .create_entry(h.vault, root, &fields("First", "pw"))
        .unwrap();

    let pending = h.workspace.begin_save(h.vault).unwrap();
    assert!(h.workspace.vault(h.vault).unwrap().is_saving());
    assert!(matches!(
        h.workspace.begin_save(h.vault),
        Err(SaveVaultError::InProgress)
    ));
    let saving = std::thread::spawn(move || pending.run());
    let second = h
        .workspace
        .create_entry(h.vault, root, &fields("Second", "pw"))
        .unwrap();
    h.workspace.finish_save(saving.join().unwrap()).unwrap();

    // The save captured the Vault before the second edit, which is still unsaved.
    assert!(h.unsaved());
    let reopened = h.reopen();
    assert!(reopened.entry(first).is_some());
    assert!(reopened.entry(second).is_none());
}

#[test]
fn autosave_is_off_by_default() {
    let mut h = harness();
    let root = h.root();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "pw"))
        .unwrap();

    assert!(!h.workspace.autosave());
    assert!(h.workspace.begin_autosaves().is_empty());
}

#[test]
fn autosave_saves_every_vault_with_unsaved_changes() {
    let mut h = harness();
    h.workspace.set_autosave(true);
    assert!(h.workspace.begin_autosaves().is_empty(), "nothing to save");
    let root = h.root();
    let id = h
        .workspace
        .create_entry(h.vault, root, &fields("Bank", "pw"))
        .unwrap();

    let saves = h.workspace.begin_autosaves();
    assert_eq!(saves.len(), 1);
    assert!(h.workspace.begin_autosaves().is_empty(), "already saving");
    for save in saves {
        h.workspace.finish_save(save.run()).unwrap();
    }

    assert!(!h.unsaved());
    assert!(h.reopen().entry(id).is_some());
    assert!(h.workspace.begin_autosaves().is_empty());
}

#[test]
fn a_failed_autosave_is_retried_only_after_the_next_edit() {
    let mut h = harness();
    h.workspace.set_autosave(true);
    let root = h.root();
    h.storage.fail_writes(true);
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "pw"))
        .unwrap();
    for save in h.workspace.begin_autosaves() {
        assert!(h.workspace.finish_save(save.run()).is_err());
    }

    assert!(h.workspace.begin_autosaves().is_empty());

    h.storage.fail_writes(false);
    h.workspace.create_group(h.vault, root, "Retry").unwrap();
    let saves = h.workspace.begin_autosaves();
    assert_eq!(saves.len(), 1);
    for save in saves {
        h.workspace.finish_save(save.run()).unwrap();
    }
    assert_eq!(h.reopen().root().all_entries().count(), 5);
}

#[test]
fn the_local_storage_provider_saves_atomically_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("personal.kdbx");
    std::fs::write(&path, KEEPASSXC_KDBX4).unwrap();
    let location = VaultLocation::Local(path.clone());
    let mut workspace = Workspace::new(Arc::new(LocalStorage));
    let vault = workspace.open_vault(&location, &key()).unwrap();
    let root = workspace.vault(vault).unwrap().vault().root().id();

    let id = workspace
        .create_entry(vault, root, &fields("Bank", "pw"))
        .unwrap();
    workspace.save_vault(vault).unwrap();

    let mut reopened = Workspace::new(Arc::new(LocalStorage));
    let vault = reopened.open_vault(&location, &key()).unwrap();
    assert!(reopened.entry(vault, id).is_some());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["personal.kdbx"], "no temporary file is left");
}

#[cfg(unix)]
#[test]
fn the_local_storage_provider_keeps_the_vault_files_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("personal.kdbx");
    std::fs::write(&path, b"old").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    LocalStorage
        .write(&VaultLocation::Local(path.clone()), b"new")
        .unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}

#[test]
fn a_failed_local_write_leaves_the_vault_file_and_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    // A directory where the Vault file should be: the final rename cannot replace it.
    let path = dir.path().join("personal.kdbx");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("inside"), b"untouched").unwrap();

    let result = LocalStorage.write(&VaultLocation::Local(path.clone()), b"new");

    assert!(result.is_err());
    assert_eq!(std::fs::read(path.join("inside")).unwrap(), b"untouched");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["personal.kdbx"]);
}

#[test]
fn edits_made_while_a_save_fails_are_still_autosaved() {
    let mut h = harness();
    h.workspace.set_autosave(true);
    let root = h.root();
    h.workspace
        .create_entry(h.vault, root, &fields("First", "pw"))
        .unwrap();
    let saves = h.workspace.begin_autosaves();
    h.storage.fail_writes(true);
    let failed: Vec<_> = saves.into_iter().map(|save| save.run()).collect();
    let second = h
        .workspace
        .create_entry(h.vault, root, &fields("Second", "pw"))
        .unwrap();
    for finished in failed {
        assert!(h.workspace.finish_save(finished).is_err());
    }
    h.storage.fail_writes(false);

    let saves = h.workspace.begin_autosaves();

    assert_eq!(
        saves.len(),
        1,
        "the edit made during the failed save is due"
    );
    for save in saves {
        h.workspace.finish_save(save.run()).unwrap();
    }
    assert!(h.reopen().entry(second).is_some());
}

#[test]
fn saving_all_saves_unsaved_and_failed_vaults_and_reports_the_status() {
    let mut h = harness();
    assert_eq!(h.workspace.save_status(), SaveStatus::Saved);
    assert!(h.workspace.begin_saves().is_empty());
    let root = h.root();
    h.workspace
        .create_entry(h.vault, root, &fields("Bank", "pw"))
        .unwrap();
    assert_eq!(h.workspace.save_status(), SaveStatus::Unsaved);

    h.storage.fail_writes(true);
    let saves = h.workspace.begin_saves();
    assert_eq!(saves.len(), 1);
    assert_eq!(h.workspace.save_status(), SaveStatus::Saving);
    for save in saves {
        assert!(h.workspace.finish_save(save.run()).is_err());
    }
    assert_eq!(h.workspace.save_status(), SaveStatus::Failed);

    h.storage.fail_writes(false);
    for save in h.workspace.begin_saves() {
        h.workspace.finish_save(save.run()).unwrap();
    }
    assert_eq!(h.workspace.save_status(), SaveStatus::Saved);
}

#[cfg(unix)]
#[test]
fn the_local_storage_provider_saves_through_a_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("synced.kdbx");
    let link = dir.path().join("personal.kdbx");
    std::fs::write(&target, b"old").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    LocalStorage
        .write(&VaultLocation::Local(link.clone()), b"new")
        .unwrap();

    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
}
