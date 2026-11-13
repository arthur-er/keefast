//! Syncing Dropbox Vaults: a Vault opens and saves offline through the encrypted local cache,
//! saves Merge in changes made on other devices before uploading, the Vault file is checked for
//! such changes in the background, and each Vault shows where it stands with Dropbox. Dropbox
//! is an in-memory Storage provider here, with Dropbox-style revisions and injected conflicts.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{CompositeKey, Vault};
use keefast_workspace::{
    OpenVaultError, SaveStatus, StorageError, SyncStatus, UnlockSecrets, VaultId, VaultLocation,
    Workspace,
};
use support::{
    FakeClipboard, FakeClock, FakeStorage, FakeVaultCache, KEEPASSXC_KDBX4, PASSWORD,
    personal_vault,
};

const START: u64 = 1_800_000_000;

fn in_dropbox() -> VaultLocation {
    VaultLocation::Dropbox("/Vaults/Personal.kdbx".into())
}

/// One device: a Workspace reaching Dropbox and its cache, at a clock the test moves.
struct Device {
    workspace: Workspace,
    clock: Arc<FakeClock>,
}

struct Harness {
    dropbox: Arc<FakeStorage>,
    cache: Arc<FakeVaultCache>,
}

fn harness() -> Harness {
    Harness {
        dropbox: Arc::new(FakeStorage::default().with_file(&in_dropbox(), KEEPASSXC_KDBX4)),
        cache: Arc::new(FakeVaultCache::default()),
    }
}

impl Harness {
    /// keefast starting on this machine: its cache is the one left by earlier runs.
    fn start(&self) -> Device {
        let clock = Arc::new(FakeClock::at_unix_seconds(START));
        let workspace = Workspace::with_clock(Arc::new(FakeStorage::default()), clock.clone())
            .with_dropbox_storage(self.dropbox.clone())
            .with_vault_cache(self.cache.clone());
        Device { workspace, clock }
    }

    fn open(&self) -> (Device, VaultId) {
        let mut device = self.start();
        let vault = device
            .workspace
            .open_vault(&in_dropbox(), &secrets())
            .expect("the Vault opens");
        (device, vault)
    }

    /// The Vault as another device would open it from Dropbox now.
    fn in_dropbox_now(&self) -> Vault {
        Vault::open(&self.dropbox.file(&in_dropbox()).unwrap(), &key()).unwrap()
    }

    /// Another device adds a Group and uploads the Vault file.
    fn add_group_elsewhere(&self, name: &str) {
        self.dropbox
            .change_elsewhere(&in_dropbox(), &with_group(self.in_dropbox_now(), name));
    }
}

fn with_group(mut vault: Vault, name: &str) -> Vec<u8> {
    let root = vault.root().id();
    vault
        .create_group(
            root,
            name,
            keefast_core::Timestamp::from_unix_seconds(START as i64 + 600),
        )
        .unwrap();
    vault.save().unwrap()
}

fn key() -> CompositeKey {
    CompositeKey::password(PASSWORD)
}

fn secrets() -> UnlockSecrets {
    UnlockSecrets::password(PASSWORD)
}

fn has_group(vault: &Vault, name: &str) -> bool {
    vault
        .root()
        .groups()
        .iter()
        .any(|group| group.name() == name)
}

impl Device {
    fn add_group(&mut self, vault: VaultId, name: &str) {
        let root = self.workspace.vault(vault).unwrap().vault().root().id();
        self.workspace.create_group(vault, root, name).unwrap();
    }

    fn has_group(&self, vault: VaultId, name: &str) -> bool {
        has_group(self.workspace.vault(vault).unwrap().vault(), name)
    }

    fn status(&self, vault: VaultId) -> Option<SyncStatus> {
        self.workspace.vault(vault).unwrap().sync_status()
    }

    /// Lets time pass until the next background sync is due, and runs it.
    fn sync_when_due(&mut self) -> usize {
        let wait = self.workspace.sync_in().expect("a sync is scheduled");
        self.clock.advance(wait);
        let syncs = self.workspace.begin_syncs();
        let count = syncs.len();
        for sync in syncs {
            let _ = self.workspace.finish_save(sync.run());
        }
        count
    }
}

#[test]
fn opening_a_dropbox_vault_caches_its_vault_file() {
    let h = harness();

    let (device, vault) = h.open();

    let cached = h
        .cache
        .cached(&in_dropbox())
        .expect("the Vault file is cached");
    assert_eq!(cached.bytes, KEEPASSXC_KDBX4);
    assert!(cached.uploaded);
    assert_eq!(device.status(vault), Some(SyncStatus::Synced));
}

#[test]
fn a_dropbox_vault_opens_offline_from_the_cache() {
    let h = harness();
    drop(h.open());
    h.dropbox.go_offline(true);

    let (device, vault) = h.open();

    assert_eq!(
        device
            .workspace
            .visible_entries(keefast_workspace::EntryScope::Vault(vault))
            .len(),
        4
    );
    assert_eq!(device.status(vault), Some(SyncStatus::Offline));
}

#[test]
fn offline_without_a_cached_copy_a_dropbox_vault_cant_open() {
    let h = harness();
    h.dropbox.go_offline(true);
    let mut device = h.start();

    let result = device.workspace.open_vault(&in_dropbox(), &secrets());

    assert!(
        matches!(
            result,
            Err(OpenVaultError::Storage(StorageError::Offline(_)))
        ),
        "{result:?}"
    );
}

#[test]
fn local_vaults_are_not_cached_and_have_no_sync_status() {
    let h = harness();
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let mut workspace = Workspace::new(storage).with_vault_cache(h.cache.clone());

    let vault = workspace.open_vault(&personal_vault(), &secrets()).unwrap();

    assert_eq!(workspace.vault(vault).unwrap().sync_status(), None);
    assert!(h.cache.cached(&personal_vault()).is_none());
    assert_eq!(workspace.sync_in(), None);
}

#[test]
fn saving_fetches_the_revision_and_merges_a_remote_change_before_uploading() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.add_group_elsewhere("From the phone");
    laptop.add_group(vault, "From the laptop");

    laptop.workspace.save_vault(vault).unwrap();

    assert_eq!(
        h.dropbox.conflicts(),
        0,
        "the upload was made against the Merged revision"
    );
    let uploaded = h.in_dropbox_now();
    assert!(has_group(&uploaded, "From the phone"));
    assert!(has_group(&uploaded, "From the laptop"));
    assert!(laptop.has_group(vault, "From the phone"));
    assert_eq!(laptop.status(vault), Some(SyncStatus::Synced));
}

#[test]
fn an_upload_that_conflicts_is_merged_and_retried() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    laptop.add_group(vault, "From the laptop");
    // Another device uploads between keefast fetching the revision and uploading.
    h.dropbox.change_before_next_write(
        &in_dropbox(),
        &with_group(h.in_dropbox_now(), "From the phone"),
    );

    laptop.workspace.save_vault(vault).unwrap();

    assert_eq!(h.dropbox.conflicts(), 1);
    let uploaded = h.in_dropbox_now();
    assert!(has_group(&uploaded, "From the phone"));
    assert!(has_group(&uploaded, "From the laptop"));
    assert_eq!(laptop.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn the_vault_shows_syncing_while_a_save_runs() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    laptop.add_group(vault, "From the laptop");

    let save = laptop.workspace.begin_save(vault).unwrap();
    assert_eq!(laptop.status(vault), Some(SyncStatus::Syncing));
    laptop.workspace.finish_save(save.run()).unwrap();

    assert_eq!(laptop.status(vault), Some(SyncStatus::Synced));
}

#[test]
fn a_sync_that_fails_shows_an_error() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    laptop.add_group(vault, "From the laptop");
    h.dropbox.fail_writes(true);

    assert!(laptop.workspace.save_vault(vault).is_err());

    assert_eq!(laptop.status(vault), Some(SyncStatus::Error));
}

#[test]
fn dropbox_is_checked_in_the_background_and_remote_changes_merged() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    assert_eq!(laptop.workspace.sync_in(), Some(Workspace::SYNC_INTERVAL));
    assert!(
        laptop.workspace.begin_syncs().is_empty(),
        "nothing is due yet"
    );
    h.add_group_elsewhere("From the phone");

    assert_eq!(laptop.sync_when_due(), 1);

    assert!(laptop.has_group(vault, "From the phone"));
    let merges = laptop.workspace.take_merges();
    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0].vault, vault);
    assert_eq!(laptop.status(vault), Some(SyncStatus::Synced));
    assert_eq!(
        laptop.workspace.sync_in(),
        Some(Workspace::SYNC_INTERVAL),
        "the next check is scheduled"
    );
}

#[test]
fn checking_an_unchanged_vault_file_does_not_download_it() {
    let h = harness();
    let (mut laptop, _) = h.open();
    let reads = h.dropbox.reads();

    laptop.sync_when_due();
    laptop.sync_when_due();

    assert_eq!(h.dropbox.reads(), reads);
    assert_eq!(h.dropbox.writes(), 0);
}

#[test]
fn a_save_made_offline_is_kept_in_the_cache_and_uploaded_when_back_online() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.dropbox.go_offline(true);
    laptop.add_group(vault, "Made offline");

    laptop.workspace.save_vault(vault).unwrap();

    assert_eq!(laptop.workspace.save_status(), SaveStatus::Saved);
    assert_eq!(laptop.status(vault), Some(SyncStatus::Offline));
    let cached = h.cache.cached(&in_dropbox()).unwrap();
    assert!(!cached.uploaded);
    assert!(has_group(
        &Vault::open(&cached.bytes, &key()).unwrap(),
        "Made offline"
    ));
    assert!(!has_group(&h.in_dropbox_now(), "Made offline"));

    // Still offline: the next check leaves it waiting.
    laptop.sync_when_due();
    assert_eq!(laptop.status(vault), Some(SyncStatus::Offline));

    h.dropbox.go_offline(false);
    h.add_group_elsewhere("From the phone");
    laptop.sync_when_due();

    let uploaded = h.in_dropbox_now();
    assert!(has_group(&uploaded, "Made offline"));
    assert!(has_group(&uploaded, "From the phone"));
    assert!(laptop.has_group(vault, "From the phone"));
    assert_eq!(laptop.status(vault), Some(SyncStatus::Synced));
    assert!(h.cache.cached(&in_dropbox()).unwrap().uploaded);
}

#[test]
fn a_key_change_saved_offline_keeps_the_old_key_until_it_is_uploaded() {
    const NEW_PASSWORD: &str = "a brand new master password";
    let h = harness();
    let (mut laptop, vault) = h.open();
    laptop
        .workspace
        .change_master_key(vault, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();
    h.dropbox.go_offline(true);
    laptop.workspace.save_vault(vault).unwrap();
    assert_eq!(laptop.status(vault), Some(SyncStatus::Offline));

    // The Vault file in Dropbox still has the old key, and another device adds to it.
    h.dropbox.go_offline(false);
    h.add_group_elsewhere("From the phone");
    laptop.sync_when_due();

    let uploaded = Vault::open(
        &h.dropbox.file(&in_dropbox()).unwrap(),
        &CompositeKey::password(NEW_PASSWORD),
    )
    .expect("the Vault file in Dropbox has the new key now");
    assert!(has_group(&uploaded, "From the phone"));
    assert_eq!(laptop.status(vault), Some(SyncStatus::Synced));
}

#[test]
fn checks_while_offline_wait_longer_each_time() {
    let h = harness();
    let (mut laptop, _) = h.open();
    h.dropbox.go_offline(true);

    let mut waits = Vec::new();
    for _ in 0..8 {
        waits.push(laptop.workspace.sync_in().unwrap());
        laptop.sync_when_due();
    }

    assert_eq!(waits[0], Workspace::SYNC_INTERVAL);
    assert!(waits.windows(2).all(|pair| pair[0] <= pair[1]), "{waits:?}");
    assert!(waits[2] > waits[1], "{waits:?}");
    assert!(
        *waits.last().unwrap() <= Duration::from_secs(5 * 60),
        "{waits:?}"
    );

    h.dropbox.go_offline(false);
    laptop.sync_when_due();
    assert_eq!(laptop.workspace.sync_in(), Some(Workspace::SYNC_INTERVAL));
}

#[test]
fn being_offline_is_not_a_failed_save() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.dropbox.go_offline(true);

    laptop.sync_when_due();

    assert_eq!(laptop.status(vault), Some(SyncStatus::Offline));
    assert_eq!(laptop.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn a_check_that_fails_with_nothing_to_save_is_no_failed_save() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.dropbox.fail_reads(true);

    laptop.sync_when_due();

    assert_eq!(laptop.status(vault), Some(SyncStatus::Error));
    assert!(!laptop.workspace.vault(vault).unwrap().save_failed());
    assert_eq!(laptop.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn edits_saved_offline_survive_a_restart_and_are_merged_on_upload() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.dropbox.go_offline(true);
    laptop.add_group(vault, "Made offline");
    laptop.workspace.save_vault(vault).unwrap();
    drop(laptop);
    h.dropbox.go_offline(false);
    h.add_group_elsewhere("From the phone");

    // keefast starts again, online: the Vault opens with the edits made offline, and uploads them.
    let (mut laptop, vault) = h.open();
    assert!(laptop.has_group(vault, "Made offline"));
    let syncs = laptop.workspace.begin_syncs();
    assert_eq!(syncs.len(), 1, "the upload starts at once");
    for sync in syncs {
        laptop.workspace.finish_save(sync.run()).unwrap();
    }

    let uploaded = h.in_dropbox_now();
    assert!(has_group(&uploaded, "Made offline"));
    assert!(has_group(&uploaded, "From the phone"));
    assert!(laptop.has_group(vault, "From the phone"));
    assert_eq!(laptop.status(vault), Some(SyncStatus::Synced));
}

#[test]
fn a_save_offline_that_cant_be_cached_fails() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.dropbox.go_offline(true);
    h.cache.fail(true);
    laptop.add_group(vault, "Made offline");

    assert!(laptop.workspace.save_vault(vault).is_err());

    assert_eq!(laptop.workspace.save_status(), SaveStatus::Failed);
}

#[test]
fn a_locked_vault_is_not_checked() {
    let h = harness();
    let (mut laptop, vault) = h.open();

    let saves = laptop
        .workspace
        .lock_vault(vault, &mut FakeClipboard::default());

    assert!(saves.is_empty());
    assert_eq!(laptop.workspace.sync_in(), None);
    laptop.clock.advance(Duration::from_secs(3600));
    assert!(laptop.workspace.begin_syncs().is_empty());
}

#[test]
fn a_vault_locked_with_an_upload_waiting_uploads_once_unlocked() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.dropbox.go_offline(true);
    laptop.add_group(vault, "Made offline");
    laptop.workspace.save_vault(vault).unwrap();

    let saves = laptop
        .workspace
        .lock_vault(vault, &mut FakeClipboard::default());
    assert!(
        saves.is_empty(),
        "saved on this machine, so it locks at once"
    );
    h.dropbox.go_offline(false);
    let unlocked = laptop
        .workspace
        .open_vault(&in_dropbox(), &secrets())
        .unwrap();
    for sync in laptop.workspace.begin_syncs() {
        laptop.workspace.finish_save(sync.run()).unwrap();
    }

    assert_eq!(unlocked, vault);
    assert!(has_group(&h.in_dropbox_now(), "Made offline"));
}

#[test]
fn syncing_now_checks_every_dropbox_vault_without_waiting() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    h.add_group_elsewhere("From the phone");

    let syncs = laptop.workspace.sync_now();
    assert_eq!(syncs.len(), 1);
    for sync in syncs {
        laptop.workspace.finish_save(sync.run()).unwrap();
    }

    assert!(laptop.has_group(vault, "From the phone"));
}

#[test]
fn the_cache_on_disk_holds_the_encrypted_vault_file_and_its_revision() {
    let dir = tempfile::tempdir().unwrap();
    let cache = keefast_workspace::FileVaultCache::new(dir.path().join("vault-cache"));
    let file = keefast_workspace::CachedFile {
        bytes: KEEPASSXC_KDBX4.to_vec(),
        revision: Some(keefast_workspace::Revision::new("rev7")),
        uploaded: false,
    };
    use keefast_workspace::VaultCache;

    assert_eq!(cache.load(&in_dropbox()), None);
    cache.store(&in_dropbox(), &file).unwrap();

    assert_eq!(cache.load(&in_dropbox()), Some(file));
    // Dropbox paths are case-insensitive.
    let other_case = VaultLocation::Dropbox("/vaults/PERSONAL.kdbx".into());
    assert!(cache.load(&other_case).is_some());
    let on_disk: Vec<_> = std::fs::read_dir(dir.path().join("vault-cache"))
        .unwrap()
        .map(|entry| std::fs::read(entry.unwrap().path()).unwrap())
        .collect();
    assert_eq!(on_disk.len(), 1);
    let contents = &on_disk[0];
    assert!(
        contents.ends_with(KEEPASSXC_KDBX4),
        "the Vault file, still encrypted"
    );
    let text = String::from_utf8_lossy(contents);
    assert!(!text.contains("Personal"), "the path isn't in it");
    assert!(!text.contains(PASSWORD));
}

#[test]
fn a_vault_file_gone_from_dropbox_shows_a_sync_error() {
    let h = harness();
    let (mut laptop, vault) = h.open();

    h.dropbox.delete_elsewhere(&in_dropbox());
    laptop.sync_when_due();

    assert_eq!(laptop.status(vault), Some(SyncStatus::Error));
    assert_eq!(laptop.workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn a_check_from_before_a_lock_does_not_land_on_the_vault_unlocked_again() {
    let h = harness();
    let (mut laptop, vault) = h.open();
    laptop.clock.advance(Workspace::SYNC_INTERVAL);
    let before_lock = laptop.workspace.begin_syncs().pop().unwrap();
    laptop
        .workspace
        .lock_vault(vault, &mut FakeClipboard::default());
    laptop
        .workspace
        .open_vault(&in_dropbox(), &secrets())
        .unwrap();
    laptop.add_group(vault, "After unlocking");
    let save = laptop.workspace.begin_save(vault).unwrap();

    let _ = laptop.workspace.finish_save(before_lock.run());

    assert_eq!(
        laptop.status(vault),
        Some(SyncStatus::Syncing),
        "the new save still runs"
    );
    assert!(laptop.workspace.vault(vault).unwrap().is_saving());
    laptop.workspace.finish_save(save.run()).unwrap();
    assert!(has_group(&h.in_dropbox_now(), "After unlocking"));
}

#[test]
fn an_unreadable_cache_file_is_set_aside_not_overwritten() {
    use keefast_workspace::VaultCache;
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("vault-cache");
    let cache = keefast_workspace::FileVaultCache::new(&cache_dir);
    let file = keefast_workspace::CachedFile {
        bytes: KEEPASSXC_KDBX4.to_vec(),
        revision: None,
        uploaded: false,
    };
    cache.store(&in_dropbox(), &file).unwrap();
    let path = std::fs::read_dir(&cache_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&path, b"damaged").unwrap();

    assert_eq!(cache.load(&in_dropbox()), None);
    cache.store(&in_dropbox(), &file).unwrap();

    let names: Vec<String> = std::fs::read_dir(&cache_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let aside = names
        .iter()
        .find(|name| name.ends_with(".unreadable"))
        .unwrap();
    assert_eq!(std::fs::read(cache_dir.join(aside)).unwrap(), b"damaged");
}
