//! Locking: on demand, after inactivity and when the OS locks or suspends, with the clock, the
//! clipboard and storage faked. A locked Vault keeps only where its Vault file is; unlocking
//! it returns the user to where they were.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use keefast_core::{EntryFields, EntryId, GroupId, OpenError, Vault};
use keefast_workspace::{
    AppConfig, ConfigStore, CopyError, CopyField, EntryScope, FileConfigStore, OpenVaultError,
    PendingSave, UnlockSecrets, UnsavedLock, VaultId, VaultLocation, View, Workspace,
};
use support::{
    FakeClipboard, FakeClock, FakeConfigStore, FakeStorage, KEEPASSXC_KDBX4, PASSWORD,
    personal_vault, work_vault,
};

struct Harness {
    storage: Arc<FakeStorage>,
    clock: Arc<FakeClock>,
    clipboard: FakeClipboard,
    workspace: Workspace,
    personal: VaultId,
    work: VaultId,
}

fn harness_with(config: AppConfig) -> Harness {
    let storage = Arc::new(
        FakeStorage::default()
            .with_file(&personal_vault(), KEEPASSXC_KDBX4)
            .with_file(&work_vault(), KEEPASSXC_KDBX4),
    );
    let clock = Arc::new(FakeClock::at_unix_seconds(1_800_000_000));
    let mut workspace = Workspace::with_clock(storage.clone(), clock.clone())
        .with_config(Arc::new(FakeConfigStore::holding(config)));
    let personal = workspace.open_vault(&personal_vault(), &key()).unwrap();
    let work = workspace.open_vault(&work_vault(), &key()).unwrap();
    Harness {
        storage,
        clock,
        clipboard: FakeClipboard::default(),
        workspace,
        personal,
        work,
    }
}

fn harness() -> Harness {
    harness_with(AppConfig::default())
}

fn key() -> UnlockSecrets {
    UnlockSecrets::password(PASSWORD)
}

/// Runs saves to the end, as the app does off the UI thread.
fn run(workspace: &mut Workspace, saves: Vec<PendingSave>) {
    for save in saves {
        let _ = workspace.finish_save(save.run());
    }
}

impl Harness {
    fn lock_all(&mut self) {
        let saves = self.workspace.lock_all(&mut self.clipboard);
        run(&mut self.workspace, saves);
    }

    fn lock(&mut self, vault: VaultId) {
        let saves = self.workspace.lock_vault(vault, &mut self.clipboard);
        run(&mut self.workspace, saves);
    }

    fn unlock(&mut self, location: &VaultLocation) -> Result<VaultId, OpenVaultError> {
        self.workspace.open_vault(location, &key())
    }

    fn is_locked(&self, vault: VaultId) -> bool {
        self.workspace.locked_vault(vault).is_some() && self.workspace.vault(vault).is_none()
    }

    fn entry(&self, vault: VaultId, title: &str) -> EntryId {
        self.workspace
            .visible_entries(EntryScope::Vault(vault))
            .into_iter()
            .find(|listed| listed.entry.title() == title)
            .unwrap_or_else(|| panic!("no Entry {title}"))
            .entry
            .id()
    }

    fn group(&self, vault: VaultId, name: &str) -> GroupId {
        let open = self.workspace.vault(vault).unwrap();
        let mut groups = Vec::new();
        flatten(open.vault().root().groups(), &mut groups);
        groups
            .into_iter()
            .find(|(_, group_name)| group_name == name)
            .unwrap_or_else(|| panic!("no Group {name}"))
            .0
    }

    /// Edits the personal Vault's GitHub Entry, leaving the Vault with unsaved changes.
    fn edit_personal(&mut self) {
        let github = self.entry(self.personal, "GitHub");
        let fields = EntryFields {
            title: "GitHub (edited)".into(),
            ..EntryFields::default()
        };
        self.workspace
            .update_entry(self.personal, github, &fields)
            .unwrap();
    }

    fn personal_file_titles(&self) -> Vec<String> {
        let bytes = self.storage.file(&personal_vault()).unwrap();
        let vault = Vault::open(&bytes, &keefast_core::CompositeKey::password(PASSWORD)).unwrap();
        vault
            .root()
            .all_entries()
            .map(|entry| entry.title().to_owned())
            .collect()
    }

    fn idle(&mut self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
        let saves = self.workspace.lock_if_idle(&mut self.clipboard);
        run(&mut self.workspace, saves);
    }
}

fn flatten(groups: &[keefast_core::Group], out: &mut Vec<(GroupId, String)>) {
    for group in groups {
        out.push((group.id(), group.name().to_owned()));
        flatten(group.groups(), out);
    }
}

#[test]
fn the_global_lock_locks_every_vault_and_drops_its_contents() {
    let mut h = harness();
    let github = h.entry(h.personal, "GitHub");

    h.lock_all();

    assert!(h.is_locked(h.personal));
    assert!(h.is_locked(h.work));
    assert_eq!(h.workspace.vaults().count(), 0);
    assert!(h.workspace.entry(h.personal, github).is_none());
    assert!(
        h.workspace
            .visible_entries(EntryScope::AllVaults)
            .is_empty()
    );
    let locked: Vec<(VaultId, String)> = h
        .workspace
        .locked_vaults()
        .map(|locked| (locked.id(), locked.display_name()))
        .collect();
    assert_eq!(
        locked,
        [
            (h.personal, "personal".to_owned()),
            (h.work, "work".to_owned())
        ]
    );
}

#[test]
fn one_vault_can_be_locked_while_the_others_stay_open() {
    let mut h = harness();

    h.lock(h.work);

    assert!(h.is_locked(h.work));
    assert!(h.workspace.vault(h.personal).is_some());
    assert!(h.workspace.locked_vault(h.personal).is_none());
}

#[test]
fn a_vault_keeps_its_number_while_locked_and_each_vault_has_its_own() {
    let mut h = harness();
    let (personal, work) = (h.personal.as_u64(), h.work.as_u64());
    assert_ne!(personal, work);

    h.lock(h.personal);

    let locked = h
        .workspace
        .locked_vaults()
        .map(|locked| locked.id().as_u64());
    assert_eq!(locked.collect::<Vec<_>>(), [personal]);
    let unlocked = h.unlock(&personal_vault()).unwrap();
    assert_eq!(unlocked.as_u64(), personal);
}

#[test]
fn a_locked_vault_unlocks_with_its_key_under_the_same_id() {
    let mut h = harness();
    h.lock_all();

    let unlocked = h.unlock(&work_vault()).unwrap();

    assert_eq!(unlocked, h.work);
    assert!(h.workspace.vault(h.work).is_some());
    assert!(h.is_locked(h.personal));
    // Vaults keep the order they were opened in, however they are unlocked.
    h.unlock(&personal_vault()).unwrap();
    let order: Vec<VaultId> = h.workspace.vaults().map(|open| open.id()).collect();
    assert_eq!(order, [h.personal, h.work]);
}

#[test]
fn a_wrong_key_leaves_the_vault_locked() {
    let mut h = harness();
    h.lock(h.personal);

    let result = h
        .workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password("wrong"));

    assert!(matches!(
        result,
        Err(OpenVaultError::Vault(OpenError::WrongKey))
    ));
    assert!(h.is_locked(h.personal));
}

#[test]
fn locked_vaults_are_not_offered_as_closed_recent_vaults() {
    let mut h = harness();

    h.lock(h.work);

    let closed: Vec<&VaultLocation> = h.workspace.closed_recent_vaults().collect();
    assert!(closed.is_empty(), "offered {closed:?}");
    assert_eq!(h.workspace.recent_vaults().len(), 2);
}

#[test]
fn an_opened_vault_is_shown() {
    let h = harness();

    assert_eq!(h.workspace.view(), View::from(EntryScope::Vault(h.work)));
}

#[test]
fn unlocking_returns_to_the_selected_vault_group_and_entry() {
    let mut h = harness();
    let group = h.group(h.personal, "Work");
    let github = h.entry(h.personal, "GitHub");
    let shown = View {
        scope: EntryScope::Group(h.personal, group),
        entry: Some((h.personal, github)),
    };
    h.workspace.show(shown.clone());

    h.lock_all();
    h.unlock(&work_vault()).unwrap();
    h.unlock(&personal_vault()).unwrap();

    assert_eq!(h.workspace.view(), shown);
}

#[test]
fn locking_one_vault_leaves_its_view_without_its_contents() {
    let mut h = harness();
    let group = h.group(h.personal, "Work");
    let github = h.entry(h.personal, "GitHub");
    h.workspace.show(View {
        scope: EntryScope::Group(h.personal, group),
        entry: Some((h.personal, github)),
    });

    h.lock(h.personal);

    assert_eq!(
        h.workspace.view(),
        View::from(EntryScope::Vault(h.personal))
    );
    h.unlock(&personal_vault()).unwrap();
    assert_eq!(
        h.workspace.view(),
        View {
            scope: EntryScope::Group(h.personal, group),
            entry: Some((h.personal, github)),
        }
    );
}

#[test]
fn unlocking_after_the_group_shown_went_elsewhere_shows_its_nearest_surviving_parent() {
    let mut h = harness();
    let work = h.group(h.personal, "Work");
    let servers = h
        .workspace
        .create_group(h.personal, work, "Servers")
        .unwrap();
    let old = h
        .workspace
        .create_group(h.personal, servers, "Old")
        .unwrap();
    h.workspace.save_vault(h.personal).unwrap();
    h.workspace
        .show(View::from(EntryScope::Group(h.personal, old)));
    h.lock(h.personal);

    let key = keefast_core::CompositeKey::password(PASSWORD);
    let mut theirs = Vault::open(&h.storage.file(&personal_vault()).unwrap(), &key).unwrap();
    let later = keefast_core::Timestamp::from_unix_seconds(1_800_000_600);
    while theirs.group(servers).is_some() {
        theirs.delete_group(servers, later).unwrap();
    }
    h.storage
        .change_elsewhere(&personal_vault(), &theirs.save().unwrap());
    h.unlock(&personal_vault()).unwrap();

    assert_eq!(
        h.workspace.view(),
        View::from(EntryScope::Group(h.personal, work))
    );
}

#[test]
fn locking_a_vault_the_user_is_not_looking_at_leaves_the_view_alone() {
    let mut h = harness();
    let github = h.entry(h.personal, "GitHub");
    let shown = View {
        scope: EntryScope::Vault(h.personal),
        entry: Some((h.personal, github)),
    };
    h.workspace.show(shown.clone());

    h.lock(h.work);
    h.unlock(&work_vault()).unwrap();

    assert_eq!(h.workspace.view(), shown);
}

#[test]
fn an_entry_of_a_vault_still_locked_is_not_selected_on_unlocking_another() {
    let mut h = harness();
    let github = h.entry(h.personal, "GitHub");
    h.workspace.show(View {
        scope: EntryScope::AllVaults,
        entry: Some((h.personal, github)),
    });

    h.lock_all();
    h.unlock(&work_vault()).unwrap();

    assert_eq!(h.workspace.view(), View::from(EntryScope::AllVaults));
    h.unlock(&personal_vault()).unwrap();
    assert_eq!(h.workspace.view().entry, Some((h.personal, github)));
}

#[test]
fn unlocking_another_vault_than_the_one_shown_shows_it() {
    let mut h = harness();
    h.workspace.show(View::from(EntryScope::Vault(h.personal)));
    h.lock_all();
    assert_eq!(h.workspace.vault_to_unlock(), Some(h.personal));

    h.unlock(&work_vault()).unwrap();

    assert_eq!(h.workspace.view(), View::from(EntryScope::Vault(h.work)));
    assert_eq!(h.workspace.vault_to_unlock(), Some(h.personal));
}

#[test]
fn the_first_locked_vault_is_offered_when_none_is_shown() {
    let mut h = harness();
    h.workspace.show(View::from(EntryScope::AllVaults));

    h.lock_all();

    assert_eq!(h.workspace.vault_to_unlock(), Some(h.personal));
}

#[test]
fn locking_clears_what_keefast_copied_from_the_clipboard() {
    let mut h = harness();
    let github = h.entry(h.personal, "GitHub");
    h.workspace
        .copy_field(&mut h.clipboard, h.personal, github, CopyField::Password)
        .unwrap();

    h.lock_all();

    assert_eq!(h.clipboard.text(), Some(""));
    assert_eq!(h.workspace.clipboard_clears_in(), None);
}

#[test]
fn vaults_lock_after_15_minutes_of_inactivity() {
    let mut h = harness();

    h.idle(15 * 60 - 1);
    assert!(h.workspace.vault(h.personal).is_some());
    assert_eq!(h.workspace.idle_lock_in(), Some(Duration::from_secs(1)));

    h.idle(1);
    assert!(h.is_locked(h.personal));
    assert!(h.is_locked(h.work));
    assert_eq!(h.workspace.idle_lock_in(), None);
}

#[test]
fn activity_postpones_the_idle_lock() {
    let mut h = harness();

    h.idle(10 * 60);
    h.workspace.note_activity();
    h.idle(10 * 60);

    assert!(h.workspace.vault(h.personal).is_some());
    assert_eq!(
        h.workspace.idle_lock_in(),
        Some(Duration::from_secs(5 * 60))
    );
}

#[test]
fn the_idle_lock_is_configurable_and_can_be_turned_off() {
    let mut h = harness_with(AppConfig {
        idle_lock_after: Some(Duration::from_secs(60)),
        ..AppConfig::default()
    });
    h.idle(60);
    assert!(h.is_locked(h.personal));

    let mut h = harness_with(AppConfig {
        idle_lock_after: None,
        ..AppConfig::default()
    });
    h.idle(24 * 60 * 60);
    assert!(h.workspace.vault(h.personal).is_some());
    assert_eq!(h.workspace.idle_lock_in(), None);
}

#[test]
fn unlocking_starts_the_idle_wait_afresh() {
    let mut h = harness();
    h.lock(h.personal);
    h.idle(20 * 60);
    assert!(h.is_locked(h.work));

    h.unlock(&personal_vault()).unwrap();

    assert_eq!(
        h.workspace.idle_lock_in(),
        Some(Duration::from_secs(15 * 60))
    );
}

#[test]
fn the_os_locking_or_suspending_locks_every_vault() {
    let mut h = harness();

    let saves = h.workspace.os_locked(&mut h.clipboard);
    run(&mut h.workspace, saves);

    assert!(h.is_locked(h.personal));
    assert!(h.is_locked(h.work));
}

#[test]
fn locking_on_os_lock_can_be_turned_off() {
    let mut h = harness_with(AppConfig {
        lock_on_os_lock: false,
        ..AppConfig::default()
    });

    let saves = h.workspace.os_locked(&mut h.clipboard);
    run(&mut h.workspace, saves);

    assert!(h.workspace.vault(h.personal).is_some());
}

#[test]
fn lock_settings_are_kept_in_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let load = |text: &str| {
        std::fs::write(&path, text).unwrap();
        let config = FileConfigStore::new(path.clone()).load().unwrap();
        (config.idle_lock_after, config.lock_on_os_lock)
    };

    assert_eq!(load(""), (Some(Duration::from_secs(15 * 60)), true));
    assert_eq!(
        load("idle_lock_seconds = 120\nlock_on_os_lock = false\n"),
        (Some(Duration::from_secs(120)), false)
    );
    assert_eq!(load("idle_lock_seconds = 0\n"), (None, true));
}

#[test]
fn a_vault_with_unsaved_changes_asks_before_locking() {
    let mut h = harness();
    h.edit_personal();

    h.lock_all();

    assert!(h.is_locked(h.work));
    assert!(h.workspace.vault(h.personal).is_some());
    assert_eq!(h.workspace.pending_lock(), Some(h.personal));
}

#[test]
fn saving_before_locking_keeps_the_changes() {
    let mut h = harness();
    h.edit_personal();
    h.lock(h.personal);

    let save = h.workspace.answer_lock(h.personal, UnsavedLock::Save);
    run(&mut h.workspace, save.into_iter().collect());

    assert!(h.is_locked(h.personal));
    assert!(
        h.personal_file_titles()
            .contains(&"GitHub (edited)".to_owned())
    );
    h.unlock(&personal_vault()).unwrap();
    assert!(
        h.workspace
            .vault(h.personal)
            .unwrap()
            .vault()
            .entry(h.entry(h.personal, "GitHub (edited)"))
            .is_some()
    );
}

#[test]
fn discarding_locks_without_saving() {
    let mut h = harness();
    h.edit_personal();
    h.lock(h.personal);

    h.workspace.answer_lock(h.personal, UnsavedLock::Discard);

    assert!(h.is_locked(h.personal));
    assert_eq!(h.workspace.pending_lock(), None);
    assert_eq!(h.storage.writes(), 0);
    h.unlock(&personal_vault()).unwrap();
    h.entry(h.personal, "GitHub");
}

#[test]
fn cancelling_keeps_the_vault_unlocked_with_its_changes() {
    let mut h = harness();
    h.edit_personal();
    h.lock(h.personal);

    h.workspace.answer_lock(h.personal, UnsavedLock::Cancel);

    assert!(h.workspace.vault(h.personal).unwrap().has_unsaved_changes());
    assert_eq!(h.workspace.pending_lock(), None);
}

#[test]
fn with_autosave_on_a_vault_is_saved_then_locked() {
    let mut h = harness_with(AppConfig {
        autosave: true,
        ..AppConfig::default()
    });
    h.edit_personal();

    h.lock_all();

    assert!(h.is_locked(h.personal));
    assert_eq!(h.workspace.pending_lock(), None);
    assert!(
        h.personal_file_titles()
            .contains(&"GitHub (edited)".to_owned())
    );
}

#[test]
fn with_autosave_on_a_failed_save_asks_before_locking() {
    let mut h = harness_with(AppConfig {
        autosave: true,
        ..AppConfig::default()
    });
    h.edit_personal();
    h.storage.fail_writes(true);

    h.lock_all();

    assert!(h.workspace.vault(h.personal).is_some());
    assert_eq!(h.workspace.pending_lock(), Some(h.personal));
}

#[test]
fn a_failed_save_before_locking_asks_again() {
    let mut h = harness();
    h.edit_personal();
    h.lock(h.personal);
    h.storage.fail_writes(true);

    let save = h.workspace.answer_lock(h.personal, UnsavedLock::Save);
    run(&mut h.workspace, save.into_iter().collect());

    assert!(h.workspace.vault(h.personal).is_some());
    assert_eq!(h.workspace.pending_lock(), Some(h.personal));
}

#[test]
fn the_idle_lock_asks_about_unsaved_changes_too() {
    let mut h = harness();
    h.edit_personal();

    h.idle(15 * 60);

    assert!(h.is_locked(h.work));
    assert_eq!(h.workspace.pending_lock(), Some(h.personal));
    assert_eq!(h.workspace.idle_lock_in(), None, "no Vault left to lock");
}

#[test]
fn a_vault_that_is_not_open_has_nothing_to_lock() {
    let mut h = harness();
    h.lock(h.personal);

    let saves = h.workspace.lock_vault(h.personal, &mut h.clipboard);

    assert!(saves.is_empty());
    assert!(h.is_locked(h.personal));
}

#[test]
fn a_locked_vault_is_not_watched_or_merged_and_unlocking_watches_it_again() {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let clock = Arc::new(FakeClock::at_unix_seconds(1_800_000_000));
    let mut workspace = Workspace::with_clock(storage.clone(), clock);
    let changed = Arc::new(Mutex::new(Vec::new()));
    let sink = changed.clone();
    workspace.watch_vault_files(move |vault| sink.lock().unwrap().push(vault));
    let vault = workspace.open_vault(&personal_vault(), &key()).unwrap();
    assert_eq!(storage.watching(&personal_vault()), 1);

    let saves = workspace.lock_vault(vault, &mut FakeClipboard::default());
    assert!(saves.is_empty());
    assert_eq!(storage.watching(&personal_vault()), 0);

    // Another device changes the Vault file while it is locked.
    storage.change_elsewhere(&personal_vault(), KEEPASSXC_KDBX4);
    for vault in changed.lock().unwrap().drain(..) {
        workspace.vault_file_changed(vault);
    }
    assert!(workspace.begin_autosaves().is_empty());
    assert!(workspace.begin_saves().is_empty());
    assert!(workspace.take_merges().is_empty());

    let unlocked = workspace.open_vault(&personal_vault(), &key()).unwrap();
    assert_eq!(unlocked, vault);
    assert_eq!(storage.watching(&personal_vault()), 1);
}

#[test]
fn a_vault_waiting_to_lock_is_hidden_until_its_changes_are_dealt_with() {
    let mut h = harness();
    let github = h.entry(h.personal, "GitHub");
    h.edit_personal();

    h.lock(h.personal);

    let waiting = h.workspace.vault(h.personal).unwrap();
    assert!(waiting.is_locking());
    // Nothing of it is shown, or can be copied from.
    assert!(h.workspace.entry(h.personal, github).is_none());
    assert!(h.workspace.readable_vault(h.personal).is_none());
    assert!(
        h.workspace
            .visible_entries(EntryScope::Vault(h.personal))
            .is_empty()
    );
    assert!(
        h.workspace
            .visible_entries(EntryScope::AllVaults)
            .iter()
            .all(|listed| listed.vault != h.personal)
    );
    assert_eq!(
        h.workspace
            .copy_field(&mut h.clipboard, h.personal, github, CopyField::Password),
        Err(CopyError::NotFound)
    );
    assert_eq!(h.clipboard.text(), None);

    // Cancelling a lock the user asked for brings it back.
    h.workspace.answer_lock(h.personal, UnsavedLock::Cancel);
    assert!(!h.workspace.vault(h.personal).unwrap().is_locking());
    assert!(h.workspace.entry(h.personal, github).is_some());
}

#[test]
fn the_idle_lock_cannot_be_cancelled() {
    let mut h = harness();
    h.edit_personal();

    h.idle(15 * 60);

    assert_eq!(h.workspace.pending_lock(), Some(h.personal));
    assert!(!h.workspace.can_cancel_lock(h.personal));
    assert!(
        h.workspace
            .answer_lock(h.personal, UnsavedLock::Cancel)
            .is_none()
    );
    assert_eq!(h.workspace.pending_lock(), Some(h.personal));
    assert!(h.workspace.vault(h.personal).unwrap().is_locking());
    // Saving or discarding still locks it.
    h.workspace.answer_lock(h.personal, UnsavedLock::Discard);
    assert!(h.is_locked(h.personal));
}

#[test]
fn the_os_locking_or_suspending_cannot_be_cancelled_either() {
    let mut h = harness();
    h.edit_personal();

    let saves = h.workspace.os_locked(&mut h.clipboard);
    run(&mut h.workspace, saves);

    assert_eq!(h.workspace.pending_lock(), Some(h.personal));
    assert!(!h.workspace.can_cancel_lock(h.personal));
    h.workspace.answer_lock(h.personal, UnsavedLock::Cancel);
    assert!(h.workspace.vault(h.personal).unwrap().is_locking());
    let save = h.workspace.answer_lock(h.personal, UnsavedLock::Save);
    run(&mut h.workspace, save.into_iter().collect());
    assert!(h.is_locked(h.personal));
}

#[test]
fn a_lock_the_user_asked_for_can_be_cancelled_until_idle_time_makes_it_final() {
    let mut h = harness();
    h.edit_personal();
    h.lock(h.personal);
    assert!(h.workspace.can_cancel_lock(h.personal));

    h.idle(15 * 60);

    assert!(!h.workspace.can_cancel_lock(h.personal));
    h.workspace.answer_lock(h.personal, UnsavedLock::Cancel);
    assert!(h.workspace.vault(h.personal).unwrap().is_locking());
}
