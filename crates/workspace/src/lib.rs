//! keefast's headless app model: the set of open Vaults, with its environment injected.
//! No GPUI here; the app renders this state and sends it commands.

mod atomic_file;
mod attachments;
mod browser;
mod browser_hosts;
mod cache;
mod clipboard;
mod clock;
mod config;
mod dropbox;
mod favicons;
mod generator;
mod locking;
mod master_key;
mod os_lock;
mod secret_store;
mod storage;
mod unlock_secrets;

use std::collections::BTreeMap;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use secrecy::ExposeSecret;

use keefast_core::{
    CustomIconId, EditError, Entry, EntryColor, EntryFields, EntryId, EntrySearch, EntrySort,
    GeneratorError, Group, GroupId, KeyFileError, MergeSummary, OpenError, SaveError, Timestamp,
    Totp, TotpCode, TotpError, Vault,
};
use secrecy::SecretString;

pub use attachments::{read_attachment, write_attachment};
pub use browser::{
    AccessAnswer, AccessId, AccessRequest, BrowserReply, BrowserSession, ConnectionId, FormFill,
    FormPick, KEEPASSXC_VERSION, LoginId, LoginRequest, LoginTarget, Outgoing, PairingId,
    PairingRequest, PendingAccess, PendingLogin, PendingPairing, ProtocolError,
};
pub use browser_hosts::{
    ALLOWED_ORIGINS, Browser, BrowserHosts, HOST_BINARY, HOST_NAME, NativeMessagingHosts,
};
pub use cache::{CachedFile, FileVaultCache, VaultCache};
pub use clipboard::{Clipboard, CopyError, CopyField};
pub use clock::{Clock, SystemClock};
pub use config::{AppConfig, ConfigError, ConfigStore, FileConfigStore, PaneWidths, ThemeSetting};
pub use dropbox::{
    Authorization, ConnectCanceller, Dropbox, DropboxAccount, DropboxConfig, DropboxError,
    DropboxItem,
};
pub use favicons::{DownloadedFavicon, Favicon, FaviconSource, HttpFavicons, PendingFavicon};
pub use generator::{GeneratorMode, GeneratorSettings};
pub use locking::{LockedVault, UnsavedLock};
pub use master_key::{ChangeKeyError, CreateVaultError, NewVault, PendingCreate};
pub use os_lock::watch_os_lock;
#[cfg(target_os = "linux")]
pub use os_lock::watch_os_lock_on;
pub use secret_store::{OsSecretStore, SecretStore, SecretStoreError};
pub use storage::{
    LocalStorage, Revision, StorageError, StorageProvider, StoredFile, VaultLocation, Watch,
};
pub use unlock_secrets::{NewKeyError, UnlockSecrets, write_new_key_file};

/// A Vault the Workspace has open, unlocked or locked. Vaults are numbered in the order they
/// were opened, and keep their number while locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultId(u64);

impl VaultId {
    /// The Vault's number, for a UI that names Vaults outside this process. Find the Vault again
    /// by comparing numbers with those of [`Workspace::vaults`] and [`Workspace::locked_vaults`]:
    /// there is no way back from a number, so none can be made up.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// What the Entry list shows: every open Vault, one Vault, one Group of a Vault, one of a
/// Vault's Smart lists, or the Entries of every open Vault with a tag or colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryScope {
    AllVaults,
    Vault(VaultId),
    Group(VaultId, GroupId),
    /// The Vault's Entries outside the Recycle Bin that the Smart list picks.
    Smart(VaultId, SmartList),
    /// Entries outside the Recycle Bin with this tag.
    Tag(String),
    /// Entries outside the Recycle Bin marked with this colour.
    Color(EntryColor),
}

impl EntryScope {
    /// The Vault the scope is in, if it is in one.
    pub fn vault(&self) -> Option<VaultId> {
        match self {
            EntryScope::Vault(vault)
            | EntryScope::Group(vault, _)
            | EntryScope::Smart(vault, _) => Some(*vault),
            EntryScope::AllVaults | EntryScope::Tag(_) | EntryScope::Color(_) => None,
        }
    }
}

/// A list of a Vault's Entries picked by what they have rather than where they are, under All
/// Entries in the sidebar. Entries in the Recycle Bin are never on one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartList {
    /// Entries with TOTP set up, even settings keefast can't read.
    WithTotp,
    /// Entries changed in the last [`RECENTLY_MODIFIED`], newest first.
    RecentlyModified,
    /// Entries whose expiry time has come.
    Expired,
}

/// How long ago an Entry may have changed to be on Recently modified.
pub const RECENTLY_MODIFIED: Duration = Duration::from_secs(30 * 24 * 60 * 60);

impl SmartList {
    pub const ALL: [SmartList; 3] = [
        SmartList::WithTotp,
        SmartList::RecentlyModified,
        SmartList::Expired,
    ];

    /// Whether the list has the Entry, by `now`.
    pub fn includes(self, entry: &Entry, now: Timestamp) -> bool {
        match self {
            SmartList::WithTotp => entry.totp().is_some(),
            SmartList::RecentlyModified => {
                let since = now.unix_seconds() - RECENTLY_MODIFIED.as_secs() as i64;
                entry
                    .last_modified()
                    .is_some_and(|modified| modified.unix_seconds() >= since)
            }
            SmartList::Expired => entry.is_expired(now),
        }
    }
}

/// Where the user is: what the Entry list shows, and the Entry selected in it, if one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub scope: EntryScope,
    pub entry: Option<(VaultId, EntryId)>,
}

impl From<EntryScope> for View {
    fn from(scope: EntryScope) -> Self {
        Self { scope, entry: None }
    }
}

impl View {
    /// The Vault the Entry list is narrowed to, if it is.
    pub fn vault(&self) -> Option<VaultId> {
        self.scope.vault()
    }
}

/// An Entry in the Entry list, with the Vault it belongs to.
#[derive(Clone, Copy)]
pub struct ListedEntry<'a> {
    pub vault: VaultId,
    pub entry: &'a Entry,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenVaultError {
    #[error("a Vault from this Vault file is already open")]
    AlreadyOpen(VaultId),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("key file not found: {0}")]
    KeyFileNotFound(PathBuf),
    #[error("could not read the key file: {0}")]
    KeyFileUnreadable(#[source] io::Error),
    #[error(transparent)]
    KeyFile(#[from] KeyFileError),
    #[error(transparent)]
    Vault(#[from] OpenError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditVaultError {
    #[error("the Vault is not open")]
    VaultNotOpen,
    #[error("the browser is not paired with the Vault")]
    NotPaired,
    #[error(transparent)]
    Edit(#[from] EditError),
}

#[derive(Debug, thiserror::Error)]
pub enum ImportIconError {
    #[error("could not read the image: {0}")]
    Unreadable(#[source] io::Error),
    #[error(transparent)]
    Edit(#[from] EditVaultError),
}

#[derive(Debug, thiserror::Error)]
pub enum SaveVaultError {
    #[error("the Vault is not open")]
    VaultNotOpen,
    #[error("the Vault is already being saved")]
    InProgress,
    #[error(transparent)]
    Vault(#[from] SaveError),
    #[error("the Vault file was changed elsewhere and can't be opened to Merge it: {0}")]
    Merge(#[from] OpenError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// A Merge of changes made to a Vault file elsewhere into its open Vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeReport {
    pub vault: VaultId,
    pub summary: MergeSummary,
}

/// Where the open Vaults stand with their Vault files, most pressing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    Saving,
    Failed,
    Unsaved,
    Saved,
}

/// Where a Vault whose Vault file is with an online Storage provider, such as Dropbox, stands
/// with its Vault file there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    /// The Vault file there has everything saved here.
    Synced,
    /// Checking the Vault file there for changes, or uploading to it.
    Syncing,
    /// The service can't be reached, as without a network or with its account disconnected.
    /// Saves are kept on this machine meanwhile.
    Offline,
    /// The last sync failed for another reason.
    Error,
}

/// Why the last sync with an online Vault file didn't go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncProblem {
    Offline,
    Failed,
}

/// How an open Vault whose Vault file is with an online Storage provider stands with it.
struct OnlineSync {
    /// Whether the Vault was saved to the local cache only, and the Vault file still lacks
    /// that save.
    upload_pending: bool,
    /// Why the last sync didn't go through, if it didn't.
    problem: Option<SyncProblem>,
    /// How many syncs in a row didn't go through, to wait longer before the next.
    failures: u32,
    /// When the Vault file is next checked for changes made elsewhere.
    next_check: SystemTime,
}

impl OnlineSync {
    fn new(fetched: Fetched, now: SystemTime) -> Self {
        let mut sync = Self {
            upload_pending: fetched == Fetched::CachedUnuploaded,
            problem: None,
            failures: 0,
            next_check: now,
        };
        match fetched {
            Fetched::FromProvider => sync.record(None, now),
            Fetched::CachedOffline => sync.record(Some(SyncProblem::Offline), now),
            // A save not uploaded yet is uploaded at once.
            Fetched::CachedUnuploaded => {}
        }
        sync
    }

    /// Records how a sync went, and schedules the next check: later each time in a row a
    /// sync doesn't go through, up to [`Workspace::MAX_SYNC_WAIT`].
    fn record(&mut self, problem: Option<SyncProblem>, now: SystemTime) {
        self.problem = problem;
        self.failures = match problem {
            Some(_) => self.failures.saturating_add(1),
            None => 0,
        };
        let doublings = self.failures.saturating_sub(1).min(8);
        let wait = Workspace::SYNC_INTERVAL
            .saturating_mul(1 << doublings)
            .min(Workspace::MAX_SYNC_WAIT);
        self.next_check = now + wait;
    }

    fn status(&self, saving: bool) -> SyncStatus {
        match (saving, self.problem) {
            (true, _) => SyncStatus::Syncing,
            (false, Some(SyncProblem::Offline)) => SyncStatus::Offline,
            (false, Some(SyncProblem::Failed)) => SyncStatus::Error,
            // Saved here only, with the upload about to start.
            (false, None) if self.upload_pending => SyncStatus::Syncing,
            (false, None) => SyncStatus::Synced,
        }
    }
}

/// A Vault the Workspace has open, together with where its Vault file lives.
pub struct OpenVault {
    id: VaultId,
    /// Tells this unlocking of the Vault from earlier ones, which had the same id, so a save
    /// begun before the Vault locked doesn't land on it.
    unlocking: u64,
    location: VaultLocation,
    vault: Vault,
    /// Counts edits; `saved_edits` is the count the Vault file last caught up with.
    edits: u64,
    saved_edits: u64,
    saving: bool,
    /// The edit count at which the last save failed, if it did.
    failed_save_at: Option<u64>,
    /// The revision of the Vault file the Vault last caught up with.
    revision: Option<Revision>,
    /// Whether the Vault file may have changed since it was last checked.
    file_changed: bool,
    watch: Option<Watch>,
    /// Whether the Vault locks once its unsaved changes are saved, or the user discards them.
    /// Until then it is hidden: the browser, the Entry list and the copy commands behave as if
    /// it were locked, so a Vault that is about to lock can't be read from.
    lock_requested: bool,
    /// Whether that lock was not asked for by the user (idle, OS lock or suspend), and so
    /// can't be cancelled by whoever is at the keyboard.
    auto_lock: bool,
    /// `None` for a local Vault file.
    online: Option<OnlineSync>,
    /// The edit that changed the master key, while the Vault file doesn't have the new key.
    key_changed_at: Option<u64>,
    /// Whether a save was asked for while another was running.
    save_requested: bool,
}

impl OpenVault {
    pub fn id(&self) -> VaultId {
        self.id
    }

    pub fn location(&self) -> &VaultLocation {
        &self.location
    }

    /// The Vault's name, or its Vault file's when it has none.
    pub fn display_name(&self) -> String {
        let name = self.vault.name();
        match name.trim() {
            "" => self.location.display_name(),
            name => name.to_owned(),
        }
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// Whether the Vault has edits its Vault file doesn't have yet.
    pub fn has_unsaved_changes(&self) -> bool {
        self.edits > self.saved_edits
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    /// Whether the Vault is waiting to lock until its unsaved changes are saved or discarded.
    /// It is hidden from the Entry list, the browser and the copy commands meanwhile.
    pub fn is_locking(&self) -> bool {
        self.lock_requested
    }

    /// Whether the last save failed and the Vault hasn't been saved since.
    pub fn save_failed(&self) -> bool {
        self.failed_save_at.is_some()
    }

    /// Where the Vault stands with its Vault file at an online Storage provider. `None` for
    /// local Vault files.
    pub fn sync_status(&self) -> Option<SyncStatus> {
        Some(self.online.as_ref()?.status(self.saving))
    }
}

/// An open-Vault command that has not run yet. It owns everything it needs, so the slow part
/// (reading the Vault file and key file, and key derivation) can run off the UI thread.
pub struct PendingOpen {
    storage: Arc<dyn StorageProvider>,
    cache: Option<Arc<dyn VaultCache>>,
    location: VaultLocation,
    secrets: UnlockSecrets,
    /// How many OS locks and suspends there had been when the open began.
    os_locks: u64,
}

/// Where an opened Vault file was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fetched {
    /// Its Storage provider.
    FromProvider,
    /// The cache, as its Storage provider couldn't be reached.
    CachedOffline,
    /// The cache, which holds a save not uploaded yet.
    CachedUnuploaded,
}

impl PendingOpen {
    pub fn unlock(self) -> Result<UnlockedVault, OpenVaultError> {
        let (bytes, revision, fetched) = self.fetch()?;
        let mut opened = Err(OpenError::WrongKey);
        for key in self.secrets.composite_keys()? {
            opened = Vault::open(&bytes, &key);
            if !matches!(opened, Err(OpenError::WrongKey)) {
                break;
            }
        }
        let vault = opened?;
        Ok(UnlockedVault {
            location: self.location,
            vault,
            revision,
            fetched,
            os_locks: Some(self.os_locks),
        })
    }

    /// Reads the Vault file, caching it if it is online. A cached save not uploaded yet is
    /// newer than the Vault file, so it is opened instead, and uploaded once open; a Vault
    /// file out of reach is opened from the cache.
    fn fetch(&self) -> Result<(Vec<u8>, Option<Revision>, Fetched), StorageError> {
        let cached = || self.cache.as_ref()?.load(&self.location);
        if let Some(cached) = cached().filter(|cached| !cached.uploaded) {
            return Ok((cached.bytes, cached.revision, Fetched::CachedUnuploaded));
        }
        match self.storage.read(&self.location) {
            Ok(stored) => {
                if let Some(cache) = &self.cache {
                    // Best effort: without it, the Vault just won't open offline.
                    let _ = cache.store(
                        &self.location,
                        &CachedFile {
                            bytes: stored.bytes.clone(),
                            revision: Some(stored.revision.clone()),
                            uploaded: true,
                        },
                    );
                }
                Ok((stored.bytes, Some(stored.revision), Fetched::FromProvider))
            }
            Err(StorageError::Offline(reason)) => match cached() {
                Some(cached) => Ok((cached.bytes, cached.revision, Fetched::CachedOffline)),
                None => Err(StorageError::Offline(reason)),
            },
            Err(error) => Err(error),
        }
    }
}

/// A decrypted Vault waiting to be added to the Workspace with [`Workspace::add_unlocked`].
pub struct UnlockedVault {
    location: VaultLocation,
    vault: Vault,
    revision: Option<Revision>,
    fetched: Fetched,
    /// For a Vault unlocked with its key, how many OS locks and suspends there had been when
    /// the unlock began; `None` for a Vault just created.
    os_locks: Option<u64>,
}

/// A save that has not run yet: a snapshot of the Vault, so the slow part (key derivation,
/// encryption and writing) can run off the UI thread while editing goes on.
///
/// A save never overwrites changes made to the Vault file elsewhere: it fetches the Vault
/// file's revision, Merges in what changed since it was last read, then writes, conditional on
/// that revision, and tries again if it changed meanwhile. A save that is only a check for such
/// changes writes only after a Merge, and only if the Vault had no unsaved edits and the Vault
/// file lacks something: unsaved edits wait for the user, or autosave, to save them.
///
/// An online Vault file out of reach, as without a network or with its account disconnected, is
/// saved to the local cache instead, and uploaded later.
pub struct PendingSave {
    id: VaultId,
    unlocking: u64,
    edits: u64,
    storage: Arc<dyn StorageProvider>,
    /// The local cache, for an online Vault file.
    cache: Option<Arc<dyn VaultCache>>,
    location: VaultLocation,
    vault: Vault,
    revision: Option<Revision>,
    /// Whether the Vault has changes to write, or this only checks the Vault file.
    write: bool,
    /// Whether the Vault had edits its Vault file didn't have.
    unsaved: bool,
}

/// How often a save Merges and tries again when the Vault file keeps changing under it.
const SAVE_ATTEMPTS: usize = 3;

impl PendingSave {
    pub fn vault_id(&self) -> VaultId {
        self.id
    }

    pub fn run(mut self) -> FinishedSave {
        let mut merged = Vec::new();
        let result = self.sync(&mut merged);
        FinishedSave {
            id: self.id,
            unlocking: self.unlocking,
            edits: self.edits,
            write: self.write,
            merged,
            result,
        }
    }

    fn sync(&mut self, merged: &mut Vec<Vault>) -> Result<Synced, SaveVaultError> {
        let mut write = self.write;
        for _ in 0..SAVE_ATTEMPTS {
            // Gone, perhaps for a moment while a sync tool replaces it: nothing to Merge.
            let stored = match self.storage.revision(&self.location) {
                Ok(current) if self.revision.as_ref() != Some(&current) => {
                    match self.storage.read(&self.location) {
                        Ok(stored) => Some(stored),
                        Err(StorageError::NotFound(_)) => None,
                        Err(error) if write && error.is_out_of_reach() => {
                            return self.keep_offline();
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                Ok(_) if !write => return Ok(Synced::Behind(None)),
                Ok(_) | Err(StorageError::NotFound(_)) => None,
                Err(error) if write && error.is_out_of_reach() => return self.keep_offline(),
                Err(error) => return Err(error.into()),
            };
            match stored {
                Some(stored) => {
                    let theirs = self.vault.open_version(&stored.bytes)?;
                    self.vault.merge(&theirs);
                    self.revision = Some(stored.revision.clone());
                    // A check writes only for a Vault without unsaved edits, and only if the
                    // Vault file lacks something the Vault has.
                    write = self.write
                        || (!self.unsaved && theirs.clone().merge(&self.vault).changed());
                    merged.push(theirs);
                    if !write {
                        self.cache_synced(stored.bytes);
                        return Ok(match self.unsaved {
                            true => Synced::Behind(Some(stored.revision)),
                            false => Synced::Saved(stored.revision),
                        });
                    }
                }
                None if !write => return Ok(Synced::Gone),
                None => {}
            }
            let bytes = self.vault.save()?;
            match self
                .storage
                .write(&self.location, &bytes, self.revision.as_ref())
            {
                Ok(revision) => {
                    self.revision = Some(revision.clone());
                    self.cache_synced(bytes);
                    return Ok(Synced::Saved(revision));
                }
                // Changed meanwhile: Merge it in, and try again.
                Err(StorageError::Conflict) => {}
                Err(error) if error.is_out_of_reach() => return self.keep_offline(),
                Err(error) => return Err(error.into()),
            }
        }
        Err(StorageError::Conflict.into())
    }

    /// Saves the Vault to the local cache, as its Vault file can't be reached, to be
    /// uploaded once it can. Without a cache, the save fails.
    fn keep_offline(&self) -> Result<Synced, SaveVaultError> {
        let offline = || StorageError::Offline("no local cache to keep the save in".into());
        let cache = self.cache.as_ref().ok_or_else(offline)?;
        let file = CachedFile {
            bytes: self.vault.save()?,
            revision: self.revision.clone(),
            uploaded: false,
        };
        cache
            .store(&self.location, &file)
            .map_err(StorageError::Io)?;
        Ok(Synced::KeptOffline(self.revision.clone()))
    }

    /// Caches the online Vault file as it now is, at the revision last read or written. Best
    /// effort: without it, the Vault opens offline as it was last cached.
    fn cache_synced(&self, bytes: Vec<u8>) {
        if let Some(cache) = &self.cache {
            let file = CachedFile {
                bytes,
                revision: self.revision.clone(),
                uploaded: true,
            };
            let _ = cache.store(&self.location, &file);
        }
    }
}

/// Where a save left the Vault file.
enum Synced {
    /// The Vault file holds everything the saved snapshot did, at this revision.
    Saved(Revision),
    /// The local cache holds everything the saved snapshot did, to upload once the Vault file
    /// can be reached; the Vault file was last seen at this revision.
    KeptOffline(Option<Revision>),
    /// The Vault file doesn't hold everything the snapshot did: it is at this revision, if read.
    Behind(Option<Revision>),
    /// A check found the Vault file gone from its Storage provider.
    Gone,
}

/// The outcome of a [`PendingSave`], to hand back with [`Workspace::finish_save`].
pub struct FinishedSave {
    id: VaultId,
    unlocking: u64,
    edits: u64,
    /// Whether it was a save, or only a check of the Vault file.
    write: bool,
    /// The versions of the Vault file Merged, to Merge into the open Vault too.
    merged: Vec<Vault>,
    result: Result<Synced, SaveVaultError>,
}

pub struct Workspace {
    /// The Storage provider for local Vault files.
    storage: Arc<dyn StorageProvider>,
    /// The Dropbox account and its files, when keefast has a Dropbox app key.
    dropbox: Option<Arc<Dropbox>>,
    /// The Storage provider for Vault files in Dropbox: `dropbox`, or a stand-in for it.
    dropbox_storage: Option<Arc<dyn StorageProvider>>,
    /// The local cache of online Vault files.
    cache: Option<Arc<dyn VaultCache>>,
    clock: Arc<dyn Clock>,
    /// The unlocked Vaults, in the order they were opened.
    vaults: Vec<OpenVault>,
    /// The locked Vaults, in the order they were opened.
    locked: Vec<LockedVault>,
    next_id: u64,
    /// Counts unlockings, to tell each from the ones before.
    unlockings: u64,
    /// Counts the OS locks and suspends that locked the Vaults, so an unlock begun before one
    /// is discarded.
    os_locks: u64,
    config: AppConfig,
    /// Where the config is kept between runs; `None` keeps it in memory only.
    config_store: Option<Arc<dyn ConfigStore>>,
    config_error: Option<ConfigError>,
    /// Where the user is. The browser pairs with the Vault shown.
    view: View,
    /// When the user last gave keefast input, for the idle lock.
    last_activity: SystemTime,
    browser: browser::BrowserConnections,
    browser_hosts: Option<Arc<dyn BrowserHosts>>,
    /// Where the native host was last registered, or why it couldn't be.
    browser_hosts_status: Option<io::Result<Vec<Browser>>>,
    pending_clear: clipboard::PendingClear,
    /// Told which Vault's Vault file may have changed, from the Storage provider's thread.
    on_file_change: Option<Arc<dyn Fn(VaultId) + Send + Sync>>,
    merges: Vec<MergeReport>,
    favicons: favicons::Favicons,
}

impl Workspace {
    /// How many recent Vault files are remembered.
    pub const MAX_RECENT_VAULTS: usize = 10;
    /// How often online Vault files are checked for changes made elsewhere. Dropbox doesn't
    /// tell desktop apps of changes, so keefast asks.
    pub const SYNC_INTERVAL: Duration = Duration::from_secs(30);
    /// The longest wait between checks while an online Vault file can't be reached.
    pub const MAX_SYNC_WAIT: Duration = Duration::from_secs(5 * 60);

    pub fn new(storage: Arc<dyn StorageProvider>) -> Self {
        Self::with_clock(storage, Arc::new(SystemClock))
    }

    pub fn with_clock(storage: Arc<dyn StorageProvider>, clock: Arc<dyn Clock>) -> Self {
        let last_activity = clock.now();
        Self {
            storage,
            dropbox: None,
            dropbox_storage: None,
            cache: None,
            clock,
            last_activity,
            vaults: Vec::new(),
            locked: Vec::new(),
            next_id: 0,
            unlockings: 0,
            os_locks: 0,
            config: AppConfig::default(),
            config_store: None,
            config_error: None,
            view: View::from(EntryScope::AllVaults),
            browser: Default::default(),
            browser_hosts: None,
            browser_hosts_status: None,
            pending_clear: Default::default(),
            on_file_change: None,
            merges: Vec::new(),
            favicons: Default::default(),
        }
    }

    /// Loads the app config from `store` and keeps it there from now on. A config that can't
    /// be loaded is reported by [`Self::config_error`], and is left alone rather than
    /// overwritten with the defaults the Workspace falls back to.
    pub fn with_config(mut self, store: Arc<dyn ConfigStore>) -> Self {
        match store.load() {
            Ok(config) => {
                self.config = config;
                self.config_store = Some(store);
            }
            Err(error) => self.config_error = Some(error),
        }
        self
    }

    /// Registers the native host with browsers through `hosts` from now on. With browser
    /// integration on, it is registered again now, so the registration follows the app when
    /// it moves.
    pub fn with_browser_hosts(mut self, hosts: Arc<dyn BrowserHosts>) -> Self {
        self.browser_hosts = Some(hosts);
        if self.config.browser_integration {
            self.register_browser_hosts();
        }
        self
    }

    /// Reads and writes Vault files in Dropbox through `dropbox` from now on.
    pub fn with_dropbox(mut self, dropbox: Arc<Dropbox>) -> Self {
        self.dropbox_storage = Some(dropbox.clone());
        self.dropbox = Some(dropbox);
        self
    }

    /// Reads and writes Vault files in Dropbox through `storage`, a stand-in for Dropbox, from
    /// now on.
    pub fn with_dropbox_storage(mut self, storage: Arc<dyn StorageProvider>) -> Self {
        self.dropbox_storage = Some(storage);
        self
    }

    /// Keeps online Vault files in `cache` from now on, so they open and save offline.
    pub fn with_vault_cache(mut self, cache: Arc<dyn VaultCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// The Dropbox Storage provider, to connect an account and browse it. `None` when keefast
    /// has no Dropbox app key.
    pub fn dropbox(&self) -> Option<&Arc<Dropbox>> {
        self.dropbox.as_ref()
    }

    /// The Dropbox app key the app config names, if it names one.
    pub fn dropbox_app_key(&self) -> Option<&str> {
        self.config.dropbox_app_key.as_deref()
    }

    /// The Storage provider that reaches `location`.
    fn storage_for(
        &self,
        location: &VaultLocation,
    ) -> Result<Arc<dyn StorageProvider>, StorageError> {
        match location {
            VaultLocation::Local(_) => Ok(Arc::clone(&self.storage)),
            VaultLocation::Dropbox(_) => self
                .dropbox_storage
                .clone()
                .ok_or_else(|| StorageError::Unavailable(location.clone())),
        }
    }

    /// The local cache for the Vault file at `location`, if it is online.
    fn cache_for(&self, location: &VaultLocation) -> Option<Arc<dyn VaultCache>> {
        location.is_online().then(|| self.cache.clone()).flatten()
    }

    pub fn browser_integration(&self) -> bool {
        self.config.browser_integration
    }

    /// Turns browser integration on or off: registers or unregisters the native host with the
    /// browsers, and remembers the choice in the app config. Turned off, every extension's
    /// session ends; the replies returned turn their pending Pairing requests down.
    pub fn set_browser_integration(&mut self, enabled: bool) -> Vec<Outgoing> {
        self.config.browser_integration = enabled;
        self.save_config();
        if enabled {
            self.register_browser_hosts();
            return Vec::new();
        }
        if let Some(hosts) = &self.browser_hosts {
            self.browser_hosts_status = Some(hosts.uninstall().map(|()| Vec::new()));
        }
        self.end_browser_sessions()
    }

    /// The browsers the native host was last registered with, or why it couldn't be. `None`
    /// until browser integration is turned on.
    pub fn browser_hosts_status(&self) -> Option<&io::Result<Vec<Browser>>> {
        self.browser_hosts_status.as_ref()
    }

    fn register_browser_hosts(&mut self) {
        if let Some(hosts) = &self.browser_hosts {
            self.browser_hosts_status = Some(hosts.install());
        }
    }

    /// Why the app config could not be loaded or last saved, if it couldn't.
    pub fn config_error(&self) -> Option<&ConfigError> {
        self.config_error.as_ref()
    }

    fn save_config(&mut self) {
        if let Some(store) = &self.config_store {
            self.config_error = store.save(&self.config).err();
        }
    }

    pub fn generator_settings(&self) -> &GeneratorSettings {
        &self.config.generator
    }

    pub fn theme(&self) -> ThemeSetting {
        self.config.theme
    }

    /// Changes which colours the app is drawn in, and remembers it in the app config.
    pub fn set_theme(&mut self, theme: ThemeSetting) {
        self.config.theme = theme;
        self.save_config();
    }

    pub fn pane_widths(&self) -> PaneWidths {
        self.config.pane_widths
    }

    /// The keys the user gave keefast's shortcuts, by name, in place of their defaults.
    pub fn shortcuts(&self) -> &BTreeMap<String, Vec<String>> {
        &self.config.shortcuts
    }

    /// Gives a shortcut these keys, or its default keys back with `None`, and remembers it in
    /// the app config. The app checks the keys are free first.
    pub fn set_shortcut(&mut self, name: &str, keys: Option<Vec<String>>) {
        let changed = match keys {
            Some(keys) => self.config.shortcuts.insert(name.to_owned(), keys.clone()) != Some(keys),
            None => self.config.shortcuts.remove(name).is_some(),
        };
        if changed {
            self.save_config();
        }
    }

    /// Changes how wide the Vault browser's panes are, and remembers it in the app config.
    pub fn set_pane_widths(&mut self, widths: PaneWidths) {
        if self.config.pane_widths != widths {
            self.config.pane_widths = widths;
            self.save_config();
        }
    }

    /// Changes how passwords are generated, and remembers it in the app config. Returns
    /// whether anything changed.
    pub fn set_generator_settings(&mut self, settings: GeneratorSettings) -> bool {
        if self.config.generator == settings {
            return false;
        }
        self.config.generator = settings;
        self.save_config();
        true
    }

    /// A new password or passphrase made with the user's generator settings.
    pub fn generate_password(&self) -> Result<SecretString, GeneratorError> {
        self.config.generator.recipe().generate()
    }

    /// Tells the Workspace where the user is, to return them there when a Vault they were in
    /// is unlocked again. The browser pairs with the Vault shown.
    pub fn show(&mut self, view: View) {
        self.view = view;
    }

    /// Where the user is, as last shown, without what locking took away.
    pub fn view(&self) -> View {
        self.view.clone()
    }

    /// The Groups that the Group shown, if it is in `vault`, is in: its parent first, up to but
    /// not including the Vault's root Group. Where [`Self::keep_view_to_what_exists`] moves the
    /// user if the Group goes.
    fn groups_shown_in(&self, vault: VaultId) -> Vec<GroupId> {
        let EntryScope::Group(shown, group) = self.view.scope else {
            return Vec::new();
        };
        let Some(open) = self.vault(vault).filter(|_| shown == vault) else {
            return Vec::new();
        };
        let mut parents = Vec::new();
        for child in open.vault.root().groups() {
            if parents_of(child, group, &mut parents) {
                break;
            }
        }
        parents
    }

    /// Moves the view off what isn't there any more, or is in a Vault that is locked: a Group
    /// gone to the nearest of `shown_in` (see [`Self::groups_shown_in`]) still there, or else
    /// to the Vault root, and an Entry gone to no Entry.
    fn keep_view_to_what_exists(&mut self, shown_in: &[GroupId]) {
        let has_group = |workspace: &Self, vault, group| {
            workspace
                .vault(vault)
                .is_some_and(|open| open.vault.group(group).is_some())
        };
        if let EntryScope::Group(vault, group) = self.view.scope
            && !has_group(self, vault, group)
        {
            self.view.scope = shown_in
                .iter()
                .copied()
                .find(|&parent| has_group(self, vault, parent))
                .map_or(EntryScope::Vault(vault), |parent| {
                    EntryScope::Group(vault, parent)
                });
        }
        if let Some((vault, entry)) = self.view.entry
            && self
                .vault(vault)
                .is_none_or(|open| open.vault.entry(entry).is_none())
        {
            self.view.entry = None;
        }
    }

    /// Vault files opened lately, most recent first, whether open now or not.
    pub fn recent_vaults(&self) -> &[VaultLocation] {
        &self.config.recent_vaults
    }

    /// The recent Vault files that aren't open now, unlocked or locked: those offered for
    /// reopening.
    pub fn closed_recent_vaults(&self) -> impl Iterator<Item = &VaultLocation> {
        self.config.recent_vaults.iter().filter(|location| {
            self.open_vault_at(location).is_none() && self.locked_vault_at(location).is_none()
        })
    }

    pub fn forget_recent(&mut self, location: &VaultLocation) {
        self.config
            .recent_vaults
            .retain(|recent| recent != location);
        self.save_config();
    }

    fn remember_recent(&mut self, location: &VaultLocation) {
        let recent = &mut self.config.recent_vaults;
        recent.retain(|recent| recent != location);
        recent.insert(0, location.clone());
        recent.truncate(Self::MAX_RECENT_VAULTS);
        self.save_config();
    }

    /// Copies an Entry's username or password to the clipboard, to be cleared after the
    /// configured wait (see [`Self::clear_clipboard_if_due`]). An empty field is not copied, so
    /// the clipboard keeps what it holds.
    pub fn copy_field(
        &mut self,
        clipboard: &mut dyn Clipboard,
        vault: VaultId,
        entry_id: EntryId,
        field: CopyField,
    ) -> Result<(), CopyError> {
        let clear_at = self
            .config
            .clipboard_clear_after
            .map(|after| self.clock.now() + after);
        // A Vault waiting to lock can't be copied from. (Not through `readable_vault`: that
        // would borrow all of the Workspace, and the clipboard record is written below.)
        let entry = self
            .vaults
            .iter()
            .find(|open| open.id == vault && !open.lock_requested)
            .and_then(|open| open.vault.entry(entry_id))
            .ok_or(CopyError::NotFound)?;
        let totp_code;
        let text = match field {
            CopyField::Username => entry.username(),
            CopyField::Password => entry.password().expose_secret(),
            CopyField::Url => entry.url(),
            CopyField::CustomField(index) => entry
                .custom_fields()
                .get(index)
                .map_or("", |field| field.value.expose_secret()),
            CopyField::TotpCode => {
                totp_code = self
                    .totp_code(vault, entry_id)
                    .and_then(Result::ok)
                    .map(|code| code.code().to_owned())
                    .unwrap_or_default();
                &totp_code
            }
        };
        if text.is_empty() {
            return Err(CopyError::Empty);
        }
        self.pending_clear.copy(clipboard, text, clear_at);
        Ok(())
    }

    /// How long copied secrets stay on the clipboard; `None` keeps them there.
    pub fn clipboard_clear_after(&self) -> Option<Duration> {
        self.config.clipboard_clear_after
    }

    /// Changes how long secrets copied from now on stay on the clipboard, and remembers it in
    /// the app config.
    pub fn set_clipboard_clear_after(&mut self, after: Option<Duration>) {
        self.config.clipboard_clear_after = after;
        self.save_config();
    }

    /// How long until what keefast last copied is cleared from the clipboard, if it will be.
    pub fn clipboard_clears_in(&self) -> Option<Duration> {
        self.pending_clear.clears_in(self.clock.now())
    }

    /// Clears what keefast last copied from the clipboard once its wait is over, unless
    /// something else has been copied since. Call it when [`Self::clipboard_clears_in`] says.
    pub fn clear_clipboard_if_due(&mut self, clipboard: &mut dyn Clipboard) {
        self.pending_clear.clear_if_due(clipboard, self.clock.now());
    }

    /// Clears what keefast last copied from the clipboard without waiting, unless something
    /// else has been copied since. Call it when quitting, so nothing copied outlives the app.
    pub fn clear_clipboard_now(&mut self, clipboard: &mut dyn Clipboard) {
        self.pending_clear.clear_now(clipboard);
    }

    /// Reads, decrypts and adds a Vault in one blocking call.
    pub fn open_vault(
        &mut self,
        location: &VaultLocation,
        secrets: &UnlockSecrets,
    ) -> Result<VaultId, OpenVaultError> {
        let pending = self.begin_open(location.clone(), secrets.clone())?;
        // Nothing can lock the Vaults while this call holds the Workspace.
        Ok(self.add(pending.unlock()?))
    }

    /// Starts opening a Vault file, or unlocking it if its Vault is locked. Run the returned
    /// open off the UI thread, then hand the unlocked Vault to [`Self::add_unlocked`]. A Vault
    /// file that is already open and unlocked, under whatever path, is refused.
    pub fn begin_open(
        &self,
        location: VaultLocation,
        secrets: UnlockSecrets,
    ) -> Result<PendingOpen, OpenVaultError> {
        let storage = self.storage_for(&location)?;
        let location = storage.canonical(&location);
        if let Some(open) = self.open_vault_at(&location) {
            return Err(OpenVaultError::AlreadyOpen(open));
        }
        Ok(PendingOpen {
            storage,
            cache: self.cache_for(&location),
            location,
            secrets,
            os_locks: self.os_locks,
        })
    }

    /// Adds an unlocked Vault, shows it, and remembers its Vault file as the most recent. If
    /// the same Vault file was opened meanwhile, that Vault is kept and this one dropped. A
    /// locked Vault unlocked this way keeps its id, and returns the user to where they were in
    /// it, if they were in it when it locked.
    ///
    /// An unlock begun before the OS locked or suspended is discarded, returning `None`: the
    /// Vault stays as it was, locked or not open, for the user to unlock again once back. A
    /// Vault just created is always added.
    pub fn add_unlocked(&mut self, unlocked: UnlockedVault) -> Option<VaultId> {
        if unlocked
            .os_locks
            .is_some_and(|began| began != self.os_locks)
        {
            return None;
        }
        Some(self.add(unlocked))
    }

    fn add(&mut self, unlocked: UnlockedVault) -> VaultId {
        self.remember_recent(&unlocked.location);
        if let Some(open) = self.open_vault_at(&unlocked.location) {
            return open;
        }
        self.last_activity = self.clock.now();
        let (id, view) = match self.locked_vault_at(&unlocked.location) {
            Some(id) => (id, self.take_locked(id)),
            None => {
                self.next_id += 1;
                let id = VaultId(self.next_id - 1);
                (id, Some((View::from(EntryScope::Vault(id)), Vec::new())))
            }
        };
        let watch = self.watch(id, &unlocked.location);
        let now = self.clock.now();
        let online = unlocked
            .location
            .is_online()
            .then(|| OnlineSync::new(unlocked.fetched, now));
        self.unlockings += 1;
        let open = OpenVault {
            id,
            unlocking: self.unlockings,
            location: unlocked.location,
            vault: unlocked.vault,
            edits: 0,
            saved_edits: 0,
            saving: false,
            failed_save_at: None,
            revision: unlocked.revision,
            file_changed: false,
            watch,
            lock_requested: false,
            auto_lock: false,
            online,
            key_changed_at: None,
            save_requested: false,
        };
        let at = self.vaults.partition_point(|open| open.id < id);
        self.vaults.insert(at, open);
        match view {
            Some((view, shown_in)) => self.restore_view(view, &shown_in),
            // Showing a Vault still locked, the user is shown the one they unlocked instead.
            None if self
                .view
                .vault()
                .is_some_and(|shown| self.vault(shown).is_none()) =>
            {
                self.view = View::from(EntryScope::Vault(id));
            }
            None => {}
        }
        self.announce_unlocked();
        id
    }

    /// Watches the Vault files of open Vaults, and of those opened later, calling `on_change`
    /// with the Vault whenever its Vault file may have changed. It is called from another
    /// thread; pass the Vault on to [`Self::vault_file_changed`].
    pub fn watch_vault_files(&mut self, on_change: impl Fn(VaultId) + Send + Sync + 'static) {
        self.on_file_change = Some(Arc::new(on_change));
        for index in 0..self.vaults.len() {
            let (id, location) = (self.vaults[index].id, self.vaults[index].location.clone());
            self.vaults[index].watch = self.watch(id, &location);
        }
    }

    fn watch(&self, id: VaultId, location: &VaultLocation) -> Option<Watch> {
        let on_change = Arc::clone(self.on_file_change.as_ref()?);
        self.storage_for(location)
            .ok()?
            .watch(location, Arc::new(move || on_change(id)))
    }

    /// Notes that the Vault's Vault file may have changed elsewhere. The next autosave or
    /// save checks it, and Merges the changes in.
    pub fn vault_file_changed(&mut self, id: VaultId) {
        if let Some(open) = self.vault_mut(id) {
            open.file_changed = true;
        }
    }

    /// The Merges of changes made elsewhere since last asked, oldest first.
    pub fn take_merges(&mut self) -> Vec<MergeReport> {
        std::mem::take(&mut self.merges)
    }

    pub fn vaults(&self) -> impl Iterator<Item = &OpenVault> {
        self.vaults.iter()
    }

    /// The open Vault read from the Vault file at `location`, if there is one. Open Vaults are
    /// known by their Storage provider's canonical location.
    pub fn open_vault_at(&self, location: &VaultLocation) -> Option<VaultId> {
        self.vaults
            .iter()
            .find(|open| &open.location == location)
            .map(|open| open.id)
    }

    /// Where new Entries and Groups go for what the Entry list shows: the shown Group, or the
    /// top Group of the shown Vault. With all Vaults shown there is none, unless only one
    /// Vault is open.
    pub fn group_for_new_items(&self, scope: EntryScope) -> Option<(VaultId, GroupId)> {
        let open = match scope {
            EntryScope::Group(vault, group) => {
                return self.group(vault, group).map(|_| (vault, group));
            }
            EntryScope::Vault(vault) | EntryScope::Smart(vault, _) => self.readable_vault(vault),
            EntryScope::AllVaults | EntryScope::Tag(_) | EntryScope::Color(_) => {
                let readable: Vec<&OpenVault> = self.readable_vaults().collect();
                match readable.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                }
            }
        }?;
        Some((open.id, open.vault.root().id()))
    }

    pub fn vault(&self, id: VaultId) -> Option<&OpenVault> {
        self.vaults.iter().find(|vault| vault.id == id)
    }

    fn vault_mut(&mut self, id: VaultId) -> Option<&mut OpenVault> {
        self.vaults.iter_mut().find(|vault| vault.id == id)
    }

    /// An open Vault that is not waiting to lock: the ones the user and the browser can read
    /// from.
    pub fn readable_vault(&self, id: VaultId) -> Option<&OpenVault> {
        self.vault(id).filter(|open| !open.lock_requested)
    }

    /// The open Vaults that are not waiting to lock, in the order they were opened.
    pub fn readable_vaults(&self) -> impl Iterator<Item = &OpenVault> {
        self.vaults.iter().filter(|open| !open.lock_requested)
    }

    /// A Group of a Vault that is not waiting to lock.
    pub fn group(&self, vault: VaultId, group: GroupId) -> Option<&Group> {
        self.readable_vault(vault)?.vault.group(group)
    }

    /// An Entry of a Vault that is not waiting to lock.
    pub fn entry(&self, vault: VaultId, entry: EntryId) -> Option<&Entry> {
        self.readable_vault(vault)?.vault.entry(entry)
    }

    /// The Entry list: every Entry in scope, including those in subgroups, sorted by title.
    /// Entries with the same title stay in the order their Vaults were opened.
    pub fn visible_entries(&self, scope: EntryScope) -> Vec<ListedEntry<'_>> {
        self.find_entries(scope, &EntrySearch::default(), EntrySort::Title)
    }

    /// The Entry list narrowed by a search: the Entries in scope that match, best match first
    /// and equally good ones in `sort` order. Entries that tie on both stay in the order their
    /// Vaults were opened. Recently modified is always newest first.
    pub fn find_entries(
        &self,
        scope: EntryScope,
        search: &EntrySearch,
        sort: EntrySort,
    ) -> Vec<ListedEntry<'_>> {
        let sort = match scope {
            EntryScope::Smart(_, SmartList::RecentlyModified) => EntrySort::Modified,
            _ => sort,
        };
        let entries = match scope {
            EntryScope::Smart(vault, list) => self.smart_list(vault, list),
            EntryScope::Tag(tag) => self.marked_entries(|entry| entry.tags().contains(&tag)),
            EntryScope::Color(color) => self.marked_entries(|entry| entry.color() == Some(color)),
            EntryScope::AllVaults => self
                .readable_vaults()
                .flat_map(|open| listed(open.id, open.vault.root()))
                .collect(),
            EntryScope::Vault(vault) => self
                .readable_vault(vault)
                .into_iter()
                .flat_map(|open| listed(vault, open.vault.root()))
                .collect(),
            EntryScope::Group(vault, group) => self
                .group(vault, group)
                .into_iter()
                .flat_map(|group| listed(vault, group))
                .collect(),
        };
        search.rank(entries, |listed| listed.entry, sort)
    }

    /// The Entries the Vault's Smart list has, in Vault file order: count them for the sidebar.
    pub fn smart_list(&self, vault: VaultId, list: SmartList) -> Vec<ListedEntry<'_>> {
        let now = self.now();
        self.readable_vault(vault)
            .into_iter()
            .flat_map(|open| entries_outside_recycle_bin(&open.vault))
            .filter(|entry| list.includes(entry, now))
            .map(|entry| ListedEntry { vault, entry })
            .collect()
    }

    /// The Entries outside the Recycle Bin of every open Vault that `marked` picks.
    fn marked_entries(&self, marked: impl Fn(&Entry) -> bool) -> Vec<ListedEntry<'_>> {
        self.readable_vaults()
            .flat_map(|open| {
                entries_outside_recycle_bin(&open.vault)
                    .filter(|entry| marked(entry))
                    .map(|entry| ListedEntry {
                        vault: open.id,
                        entry,
                    })
            })
            .collect()
    }

    pub fn create_entry(
        &mut self,
        vault: VaultId,
        group: GroupId,
        fields: &EntryFields,
    ) -> Result<EntryId, EditVaultError> {
        self.edit(vault, |vault, now| vault.create_entry(group, fields, now))
    }

    /// Sets an Entry's fields, keeping the version it replaces in the Entry's history.
    pub fn update_entry(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        fields: &EntryFields,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.update_entry(entry, fields, now))
    }

    pub fn move_entry(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        to: GroupId,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.move_entry(entry, to, now))
    }

    /// Moves the Entry to the Recycle Bin, or deletes it for good when the Vault has the
    /// Recycle Bin disabled or the Entry is already in it.
    pub fn delete_entry(&mut self, vault: VaultId, entry: EntryId) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.delete_entry(entry, now))
    }

    pub fn create_group(
        &mut self,
        vault: VaultId,
        parent: GroupId,
        name: &str,
    ) -> Result<GroupId, EditVaultError> {
        self.edit(vault, |vault, now| vault.create_group(parent, name, now))
    }

    /// Renames the Vault itself. An empty name names it after its Vault file again.
    pub fn rename_vault(&mut self, vault: VaultId, name: &str) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| {
            vault.rename(name, now);
            Ok(())
        })
    }

    /// Turns the Vault's Recycle Bin on or off: off, deleting deletes for good.
    pub fn set_recycle_bin_enabled(
        &mut self,
        vault: VaultId,
        enabled: bool,
    ) -> Result<(), EditVaultError> {
        if self
            .vault(vault)
            .is_some_and(|open| open.vault.recycle_bin_enabled() == enabled)
        {
            return Ok(());
        }
        self.edit(vault, |vault, now| {
            vault.set_recycle_bin_enabled(enabled, now);
            Ok(())
        })
    }

    pub fn rename_group(
        &mut self,
        vault: VaultId,
        group: GroupId,
        name: &str,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.rename_group(group, name, now))
    }

    pub fn move_group(
        &mut self,
        vault: VaultId,
        group: GroupId,
        to: GroupId,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.move_group(group, to, now))
    }

    /// Moves the Group to the Recycle Bin, or deletes it for good, like [`Self::delete_entry`].
    pub fn delete_group(&mut self, vault: VaultId, group: GroupId) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.delete_group(group, now))
    }

    /// Sets up TOTP on an Entry, or removes it with `None`, keeping the version it replaces in
    /// the Entry's history.
    pub fn set_totp(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        totp: Option<&Totp>,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| vault.set_totp(entry, totp, now))
    }

    /// Makes an earlier version of an Entry current again, by its index in the Entry's history,
    /// oldest first. The version it replaces goes to the history.
    pub fn restore_entry_version(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        version: usize,
    ) -> Result<(), EditVaultError> {
        self.edit(vault, |vault, now| {
            vault.restore_entry_version(entry, version, now)
        })
    }

    /// The Entry's TOTP code now, with how long it lasts, or why its TOTP settings can't be
    /// read. `None` when the Entry has no TOTP, or isn't open.
    pub fn totp_code(&self, vault: VaultId, entry: EntryId) -> Option<Result<TotpCode, TotpError>> {
        let totp = self.entry(vault, entry)?.totp()?;
        let now = self.now();
        Some(totp.map(|totp| totp.code(now)))
    }

    /// Every tag of the Entries outside the Recycle Bin in the open Vaults, sorted ignoring
    /// case, each once.
    pub fn tags(&self) -> Vec<&str> {
        let mut tags: Vec<&str> = self
            .readable_vaults()
            .flat_map(|open| entries_outside_recycle_bin(&open.vault))
            .flat_map(|entry| entry.tags().iter().map(String::as_str))
            .collect();
        tags.sort_by_cached_key(|tag| (tag.to_lowercase(), tag.to_string()));
        tags.dedup();
        tags
    }

    /// Chooses the colour a tag is drawn in, in this Vault, or with `None` undoes the choice.
    pub fn set_tag_color(
        &mut self,
        vault: VaultId,
        tag: &str,
        color: Option<EntryColor>,
    ) -> Result<(), EditVaultError> {
        let open = self.vault(vault).ok_or(EditVaultError::VaultNotOpen)?;
        if open.vault.tag_colors().get(tag).copied() == color {
            return Ok(());
        }
        self.edit(vault, |vault, now| {
            vault.set_tag_color(tag, color, now);
            Ok(())
        })
    }

    /// Whether the Entry's expiry time has come.
    pub fn is_expired(&self, entry: &Entry) -> bool {
        entry.is_expired(self.now())
    }

    /// Adds the PNG image at `path` to the Vault's custom icons, for its Entries to use. An
    /// image the Vault already has is not added again: its icon is returned.
    pub fn import_custom_icon(
        &mut self,
        vault: VaultId,
        path: &Path,
    ) -> Result<CustomIconId, ImportIconError> {
        if self.vault(vault).is_none() {
            return Err(EditVaultError::VaultNotOpen.into());
        }
        let png = read_icon(path).map_err(ImportIconError::Unreadable)?;
        // Not an edit when the Vault already has the image, so it isn't left with unsaved changes.
        let existing = self
            .vault(vault)
            .and_then(|open| open.vault.custom_icon_with(&png));
        if let Some(icon) = existing {
            return Ok(icon);
        }
        Ok(self.edit(vault, |vault, _| vault.add_custom_icon(&png))?)
    }

    /// The time at the Workspace's clock.
    pub fn now(&self) -> Timestamp {
        Timestamp::from_system_time(self.clock.now())
    }

    fn edit<T>(
        &mut self,
        id: VaultId,
        edit: impl FnOnce(&mut Vault, Timestamp) -> Result<T, EditError>,
    ) -> Result<T, EditVaultError> {
        let now = self.now();
        let open = self.vault_mut(id).ok_or(EditVaultError::VaultNotOpen)?;
        let result = edit(&mut open.vault, now)?;
        open.edits += 1;
        Ok(result)
    }

    /// Saves a Vault to its Vault file in one blocking call.
    pub fn save_vault(&mut self, id: VaultId) -> Result<(), SaveVaultError> {
        let pending = self.begin_save(id)?;
        self.finish_save(pending.run())
    }

    /// Starts saving a Vault. Run the returned save off the UI thread, then hand its outcome
    /// to [`Self::finish_save`]. Only one save per Vault runs at a time: asked for while one
    /// runs, the save is refused now and started by the next [`Self::begin_autosaves`].
    pub fn begin_save(&mut self, id: VaultId) -> Result<PendingSave, SaveVaultError> {
        self.begin(id, true)
    }

    /// Starts a save, or with `write` off only a check of the Vault file for changes made
    /// elsewhere.
    fn begin(&mut self, id: VaultId, write: bool) -> Result<PendingSave, SaveVaultError> {
        let location = &self.vault(id).ok_or(SaveVaultError::VaultNotOpen)?.location;
        let storage = self.storage_for(location)?;
        let cache = self.cache_for(location);
        let open = self.vault_mut(id).ok_or(SaveVaultError::VaultNotOpen)?;
        if open.saving {
            // Saved as soon as the running save is done, which may not have every edit.
            open.save_requested |= write;
            return Err(SaveVaultError::InProgress);
        }
        open.saving = true;
        // A save kept on this machine is uploaded by whatever reaches the Vault file next.
        let write = write || open.online.as_ref().is_some_and(|sync| sync.upload_pending);
        open.save_requested &= !write;
        // This save reads the Vault file if it changed, so the change is taken care of.
        open.file_changed = false;
        Ok(PendingSave {
            id,
            unlocking: open.unlocking,
            edits: open.edits,
            storage,
            cache,
            location: open.location.clone(),
            vault: open.vault.clone(),
            revision: open.revision.clone(),
            write,
            unsaved: open.has_unsaved_changes() || open.save_failed(),
        })
    }

    /// Records a save's outcome, Merging into the Vault any changes made to its Vault file
    /// elsewhere that the save found. Edits made while it ran stay unsaved. A Vault waiting to
    /// lock until it is saved locks now if nothing is left unsaved. A save begun before the
    /// Vault locked is not recorded.
    pub fn finish_save(&mut self, finished: FinishedSave) -> Result<(), SaveVaultError> {
        // Read before the Merge, which may take the Group shown away.
        let shown_in = self.groups_shown_in(finished.id);
        let Some(open) = self
            .vaults
            .iter_mut()
            .find(|open| (open.id, open.unlocking) == (finished.id, finished.unlocking))
        else {
            return finished.result.map(|_| ());
        };
        open.saving = false;
        let mut summary = MergeSummary::default();
        for theirs in &finished.merged {
            summary += open.vault.merge(theirs);
        }
        let merged = summary.changed();
        if merged {
            self.merges.push(MergeReport {
                vault: open.id,
                summary,
            });
        }
        let (outcome, problem) = match finished.result {
            Ok(synced) => {
                let (revision, caught_up, problem) = match synced {
                    Synced::Saved(revision) => (Some(revision), true, None),
                    Synced::KeptOffline(revision) => (revision, true, Some(SyncProblem::Offline)),
                    Synced::Behind(revision) => (revision, false, None),
                    Synced::Gone => (None, false, Some(SyncProblem::Failed)),
                };
                if let Some(revision) = revision {
                    open.revision = Some(revision);
                }
                if caught_up {
                    open.saved_edits = open.saved_edits.max(finished.edits);
                    open.failed_save_at = None;
                    if let Some(sync) = &mut open.online {
                        sync.upload_pending = problem == Some(SyncProblem::Offline);
                    }
                    // Only a Vault file with the new key lets the old one go: a save kept on this
                    // machine still has to Merge what Dropbox holds, which has the old key.
                    if problem != Some(SyncProblem::Offline) {
                        open.key_change_saved(finished.edits);
                    }
                }
                (Ok(()), problem)
            }
            Err(error) => {
                let offline =
                    matches!(&error, SaveVaultError::Storage(error) if error.is_out_of_reach());
                // A check of an online Vault file that fails, with nothing to save, is no failed
                // save: its Sync status tells of it.
                if finished.write || (!offline && open.online.is_none()) {
                    open.failed_save_at = Some(finished.edits);
                }
                let problem = match offline {
                    true => SyncProblem::Offline,
                    false => SyncProblem::Failed,
                };
                (Err(error), Some(problem))
            }
        };
        let now = self.clock.now();
        if let Some(sync) = &mut open.online {
            sync.record(problem, now);
        }
        let lock = open.lock_requested && !open.has_unsaved_changes();
        if merged {
            // The Merge may have taken away the Group or Entry the user is in.
            self.keep_view_to_what_exists(&shown_in);
        }
        if lock {
            self.lock_now(&[finished.id]);
        }
        outcome
    }

    /// Starts saving every Vault with unsaved changes or a failed last save, and checking
    /// every Vault whose Vault file may have changed, except those already being saved.
    pub fn begin_saves(&mut self) -> Vec<PendingSave> {
        self.begin_due(|open| open.has_unsaved_changes() || open.save_failed())
    }

    /// Starts saving every Vault not being saved for which `write` says so, and checking
    /// every other one whose Vault file may have changed.
    fn begin_due(&mut self, write: impl Fn(&OpenVault) -> bool) -> Vec<PendingSave> {
        let due: Vec<(VaultId, bool)> = self
            .vaults
            .iter()
            .filter(|open| !open.saving)
            .filter_map(|open| {
                let write = write(open);
                (write || open.file_changed).then_some((open.id, write))
            })
            .collect();
        due.into_iter()
            .filter_map(|(id, write)| self.begin(id, write).ok())
            .collect()
    }

    /// How long until an online Vault file is due to be checked for changes made elsewhere,
    /// if an unlocked Vault has one. Call [`Self::begin_syncs`] then.
    pub fn sync_in(&self) -> Option<Duration> {
        let now = self.clock.now();
        self.vaults
            .iter()
            .filter(|open| !open.saving)
            .filter_map(|open| Some(open.online.as_ref()?.next_check))
            .min()
            .map(|at| at.duration_since(now).unwrap_or_default())
    }

    /// Starts checking every online Vault file that is due for it, Merging in changes made
    /// elsewhere and uploading saves kept on this machine meanwhile. Locked Vaults are never
    /// checked.
    pub fn begin_syncs(&mut self) -> Vec<PendingSave> {
        let now = self.clock.now();
        let due: Vec<VaultId> = self
            .vaults
            .iter()
            .filter(|open| {
                let due = |sync: &OnlineSync| sync.next_check <= now;
                !open.saving && open.online.as_ref().is_some_and(due)
            })
            .map(|open| open.id)
            .collect();
        let mut syncs = Vec::new();
        for id in due {
            match self.begin(id, false) {
                Ok(sync) => syncs.push(sync),
                // Not reachable at all, as without Dropbox: try again later, not at once.
                Err(_) => {
                    if let Some(sync) = self.vault_mut(id).and_then(|open| open.online.as_mut()) {
                        sync.record(Some(SyncProblem::Failed), now);
                    }
                }
            }
        }
        syncs
    }

    /// Starts checking every online Vault file now, without waiting for its turn, like
    /// [`Self::begin_syncs`].
    pub fn sync_now(&mut self) -> Vec<PendingSave> {
        let now = self.clock.now();
        for sync in self
            .vaults
            .iter_mut()
            .filter_map(|open| open.online.as_mut())
        {
            sync.next_check = now;
        }
        self.begin_syncs()
    }

    /// The save status of all open Vaults together.
    pub fn save_status(&self) -> SaveStatus {
        let any = |check: fn(&OpenVault) -> bool| self.vaults.iter().any(check);
        if any(OpenVault::is_saving) {
            SaveStatus::Saving
        } else if any(OpenVault::save_failed) {
            SaveStatus::Failed
        } else if any(OpenVault::has_unsaved_changes) {
            SaveStatus::Unsaved
        } else {
            SaveStatus::Saved
        }
    }

    pub fn autosave(&self) -> bool {
        self.config.autosave
    }

    /// Turns saving after every edit on or off, and remembers the choice in the app config.
    pub fn set_autosave(&mut self, autosave: bool) {
        self.config.autosave = autosave;
        self.save_config();
    }

    /// With autosave on, starts saving every Vault with unsaved changes that isn't already
    /// being saved. Call it after edits, after saves finish and after a Vault file changes. A
    /// Vault whose last save failed is not retried until it is edited again.
    ///
    /// Autosave on or off, it also starts the saves asked for while another save ran, and
    /// checks every Vault whose Vault file may have changed elsewhere: changes found are Merged
    /// in, and the Merge saved unless the Vault has unsaved edits, which wait for a save.
    pub fn begin_autosaves(&mut self) -> Vec<PendingSave> {
        let autosave = self.config.autosave;
        self.begin_due(|open| {
            open.save_requested
                || autosave && open.has_unsaved_changes() && open.failed_save_at != Some(open.edits)
        })
    }
}

/// Every Entry in `group` and its subgroups, listed as from `vault`.
fn listed(vault: VaultId, group: &Group) -> impl Iterator<Item = ListedEntry<'_>> {
    group
        .all_entries()
        .map(move |entry| ListedEntry { vault, entry })
}

/// The Entries of a Vault, except those in its Recycle Bin.
fn entries_outside_recycle_bin(vault: &Vault) -> impl Iterator<Item = &Entry> {
    fn collect<'a>(group: &'a Group, bin: Option<GroupId>, out: &mut Vec<&'a Entry>) {
        if Some(group.id()) == bin {
            return;
        }
        out.extend(group.entries());
        for child in group.groups() {
            collect(child, bin, out);
        }
    }
    let mut entries = Vec::new();
    collect(vault.root(), vault.recycle_bin(), &mut entries);
    entries.into_iter()
}

/// Whether `group` is `within` or below it, adding the Groups it is in below `within`, then
/// `within`, to `parents`, nearest first.
fn parents_of(within: &Group, group: GroupId, parents: &mut Vec<GroupId>) -> bool {
    if within.id() == group {
        return true;
    }
    let found = within
        .groups()
        .iter()
        .any(|child| parents_of(child, group, parents));
    if found {
        parents.push(within.id());
    }
    found
}

/// Reads an icon image, refusing one too large to be an icon before reading it all.
fn read_icon(path: &Path) -> io::Result<Vec<u8>> {
    let limit = keefast_core::MAX_CUSTOM_ICON_BYTES as u64 + 1;
    let mut png = Vec::new();
    std::fs::File::open(path)?
        .take(limit)
        .read_to_end(&mut png)?;
    Ok(png)
}
