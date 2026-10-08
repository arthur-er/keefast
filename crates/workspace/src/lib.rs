//! keefast's headless app model: the set of open Vaults, with its environment injected.
//! No GPUI here; the app renders this state and sends it commands.

mod atomic_file;
mod browser;
mod browser_hosts;
mod clock;
mod config;
mod generator;
mod storage;

use std::io;
use std::sync::Arc;

use keefast_core::{
    CompositeKey, EditError, Entry, EntryFields, EntryId, GeneratorError, Group, GroupId,
    OpenError, SaveError, Timestamp, Vault,
};
use secrecy::SecretString;

pub use browser::{
    AccessAnswer, AccessId, AccessRequest, BrowserReply, BrowserSession, ConnectionId,
    KEEPASSXC_VERSION, Outgoing, PairingId, PairingRequest, PendingAccess, PendingPairing,
    ProtocolError,
};
pub use browser_hosts::{
    ALLOWED_ORIGINS, Browser, BrowserHosts, HOST_BINARY, HOST_NAME, NativeMessagingHosts,
};
pub use clock::{Clock, SystemClock};
pub use config::{AppConfig, ConfigError, ConfigStore, FileConfigStore};
pub use generator::{GeneratorMode, GeneratorSettings};
pub use storage::{LocalStorage, StorageError, StorageProvider, VaultLocation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VaultId(u64);

/// What the Entry list shows: every open Vault, one Vault, or one Group of a Vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryScope {
    AllVaults,
    Vault(VaultId),
    Group(VaultId, GroupId),
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
    #[error(transparent)]
    Vault(#[from] OpenError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditVaultError {
    #[error("the Vault is not open")]
    VaultNotOpen,
    #[error(transparent)]
    Edit(#[from] EditError),
}

#[derive(Debug, thiserror::Error)]
pub enum SaveVaultError {
    #[error("the Vault is not open")]
    VaultNotOpen,
    #[error("the Vault is already being saved")]
    InProgress,
    #[error(transparent)]
    Vault(#[from] SaveError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Where the open Vaults stand with their Vault files, most pressing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    Saving,
    Failed,
    Unsaved,
    Saved,
}

/// A Vault the Workspace has open, together with where its Vault file lives.
pub struct OpenVault {
    id: VaultId,
    location: VaultLocation,
    vault: Vault,
    /// Counts edits; `saved_edits` is the count the Vault file last caught up with.
    edits: u64,
    saved_edits: u64,
    saving: bool,
    /// The edit count at which the last save failed, if it did.
    failed_save_at: Option<u64>,
}

impl OpenVault {
    pub fn id(&self) -> VaultId {
        self.id
    }

    pub fn location(&self) -> &VaultLocation {
        &self.location
    }

    pub fn display_name(&self) -> String {
        self.location.display_name()
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

    /// Whether the last save failed and the Vault hasn't been saved since.
    pub fn save_failed(&self) -> bool {
        self.failed_save_at.is_some()
    }
}

/// An open-Vault command that has not run yet. It owns everything it needs, so the slow part
/// (reading the Vault file and key derivation) can run off the UI thread.
pub struct PendingOpen {
    storage: Arc<dyn StorageProvider>,
    location: VaultLocation,
    key: CompositeKey,
}

impl PendingOpen {
    pub fn unlock(self) -> Result<UnlockedVault, OpenVaultError> {
        let bytes = self.storage.read(&self.location)?;
        let vault = Vault::open(&bytes, &self.key)?;
        Ok(UnlockedVault {
            location: self.location,
            vault,
        })
    }
}

/// A decrypted Vault waiting to be added to the Workspace with [`Workspace::add_unlocked`].
pub struct UnlockedVault {
    location: VaultLocation,
    vault: Vault,
}

/// A save that has not run yet: a snapshot of the Vault, so the slow part (key derivation,
/// encryption and writing) can run off the UI thread while editing goes on.
pub struct PendingSave {
    id: VaultId,
    edits: u64,
    storage: Arc<dyn StorageProvider>,
    location: VaultLocation,
    vault: Vault,
}

impl PendingSave {
    pub fn vault_id(&self) -> VaultId {
        self.id
    }

    pub fn run(self) -> FinishedSave {
        let result = self
            .vault
            .save()
            .map_err(SaveVaultError::from)
            .and_then(|bytes| Ok(self.storage.write(&self.location, &bytes)?));
        FinishedSave {
            id: self.id,
            edits: self.edits,
            result,
        }
    }
}

/// The outcome of a [`PendingSave`], to hand back with [`Workspace::finish_save`].
pub struct FinishedSave {
    id: VaultId,
    edits: u64,
    result: Result<(), SaveVaultError>,
}

pub struct Workspace {
    storage: Arc<dyn StorageProvider>,
    clock: Arc<dyn Clock>,
    vaults: Vec<OpenVault>,
    next_id: u64,
    config: AppConfig,
    /// Where the config is kept between runs; `None` keeps it in memory only.
    config_store: Option<Arc<dyn ConfigStore>>,
    config_error: Option<ConfigError>,
    /// The Vault the user is looking at, which the browser pairs with.
    focused_vault: Option<VaultId>,
    browser: browser::BrowserConnections,
    browser_hosts: Option<Arc<dyn BrowserHosts>>,
    /// Where the native host was last registered, or why it couldn't be.
    browser_hosts_status: Option<io::Result<Vec<Browser>>>,
}

impl Workspace {
    /// How many recent Vault files are remembered.
    pub const MAX_RECENT_VAULTS: usize = 10;

    pub fn new(storage: Arc<dyn StorageProvider>) -> Self {
        Self::with_clock(storage, Arc::new(SystemClock))
    }

    pub fn with_clock(storage: Arc<dyn StorageProvider>, clock: Arc<dyn Clock>) -> Self {
        Self {
            storage,
            clock,
            vaults: Vec::new(),
            next_id: 0,
            config: AppConfig::default(),
            config_store: None,
            config_error: None,
            focused_vault: None,
            browser: Default::default(),
            browser_hosts: None,
            browser_hosts_status: None,
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

    /// Changes how passwords are generated, and remembers it in the app config.
    pub fn set_generator_settings(&mut self, settings: GeneratorSettings) {
        if self.config.generator != settings {
            self.config.generator = settings;
            self.save_config();
        }
    }

    /// A new password or passphrase made with the user's generator settings.
    pub fn generate_password(&self) -> Result<SecretString, GeneratorError> {
        self.config.generator.recipe().generate()
    }

    /// Tells the Workspace which Vault the user is looking at, if one. The browser pairs with
    /// that Vault.
    pub fn focus_vault(&mut self, vault: Option<VaultId>) {
        self.focused_vault = vault;
    }

    /// Vault files opened lately, most recent first, whether open now or not.
    pub fn recent_vaults(&self) -> &[VaultLocation] {
        &self.config.recent_vaults
    }

    /// The recent Vault files that aren't open now: those offered for reopening.
    pub fn closed_recent_vaults(&self) -> impl Iterator<Item = &VaultLocation> {
        self.config
            .recent_vaults
            .iter()
            .filter(|location| self.open_vault_at(location).is_none())
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

    /// Reads, decrypts and adds a Vault in one blocking call.
    pub fn open_vault(
        &mut self,
        location: &VaultLocation,
        key: &CompositeKey,
    ) -> Result<VaultId, OpenVaultError> {
        let pending = self.begin_open(location.clone(), key.clone())?;
        Ok(self.add_unlocked(pending.unlock()?))
    }

    /// Starts opening a Vault file. Run the returned open off the UI thread, then hand the
    /// unlocked Vault to [`Self::add_unlocked`]. A Vault file that is already open, under
    /// whatever path, is refused.
    pub fn begin_open(
        &self,
        location: VaultLocation,
        key: CompositeKey,
    ) -> Result<PendingOpen, OpenVaultError> {
        let location = self.storage.canonical(&location);
        if let Some(open) = self.open_vault_at(&location) {
            return Err(OpenVaultError::AlreadyOpen(open));
        }
        Ok(PendingOpen {
            storage: Arc::clone(&self.storage),
            location,
            key,
        })
    }

    /// Adds an unlocked Vault and remembers its Vault file as the most recent. If the same
    /// Vault file was opened meanwhile, that Vault is kept and this one dropped.
    pub fn add_unlocked(&mut self, unlocked: UnlockedVault) -> VaultId {
        self.remember_recent(&unlocked.location);
        if let Some(open) = self.open_vault_at(&unlocked.location) {
            return open;
        }
        let id = VaultId(self.next_id);
        self.next_id += 1;
        self.vaults.push(OpenVault {
            id,
            location: unlocked.location,
            vault: unlocked.vault,
            edits: 0,
            saved_edits: 0,
            saving: false,
            failed_save_at: None,
        });
        id
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
            EntryScope::Vault(vault) => self.vault(vault),
            EntryScope::AllVaults => match self.vaults.as_slice() {
                [only] => Some(only),
                _ => None,
            },
        }?;
        Some((open.id, open.vault.root().id()))
    }

    pub fn vault(&self, id: VaultId) -> Option<&OpenVault> {
        self.vaults.iter().find(|vault| vault.id == id)
    }

    fn vault_mut(&mut self, id: VaultId) -> Option<&mut OpenVault> {
        self.vaults.iter_mut().find(|vault| vault.id == id)
    }

    pub fn group(&self, vault: VaultId, group: GroupId) -> Option<&Group> {
        self.vault(vault)?.vault.group(group)
    }

    pub fn entry(&self, vault: VaultId, entry: EntryId) -> Option<&Entry> {
        self.vault(vault)?.vault.entry(entry)
    }

    /// The Entry list: every Entry in scope, including those in subgroups, sorted by title.
    /// Entries with the same title stay in the order their Vaults were opened.
    pub fn visible_entries(&self, scope: EntryScope) -> Vec<ListedEntry<'_>> {
        let groups: Vec<(VaultId, &Group)> = match scope {
            EntryScope::AllVaults => self
                .vaults
                .iter()
                .map(|open| (open.id, open.vault.root()))
                .collect(),
            EntryScope::Vault(vault) => self
                .vault(vault)
                .map(|open| (vault, open.vault.root()))
                .into_iter()
                .collect(),
            EntryScope::Group(vault, group) => self
                .group(vault, group)
                .map(|group| (vault, group))
                .into_iter()
                .collect(),
        };
        let mut entries: Vec<ListedEntry> = groups
            .into_iter()
            .flat_map(|(vault, group)| {
                group
                    .all_entries()
                    .map(move |entry| ListedEntry { vault, entry })
            })
            .collect();
        entries.sort_by_cached_key(|listed| listed.entry.title().to_lowercase());
        entries
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

    fn edit<T>(
        &mut self,
        id: VaultId,
        edit: impl FnOnce(&mut Vault, Timestamp) -> Result<T, EditError>,
    ) -> Result<T, EditVaultError> {
        let now = Timestamp::from_system_time(self.clock.now());
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
    /// to [`Self::finish_save`]. Only one save per Vault runs at a time.
    pub fn begin_save(&mut self, id: VaultId) -> Result<PendingSave, SaveVaultError> {
        let storage = Arc::clone(&self.storage);
        let open = self.vault_mut(id).ok_or(SaveVaultError::VaultNotOpen)?;
        if open.saving {
            return Err(SaveVaultError::InProgress);
        }
        open.saving = true;
        Ok(PendingSave {
            id,
            edits: open.edits,
            storage,
            location: open.location.clone(),
            vault: open.vault.clone(),
        })
    }

    /// Records a save's outcome. Edits made while it ran stay unsaved.
    pub fn finish_save(&mut self, finished: FinishedSave) -> Result<(), SaveVaultError> {
        let Some(open) = self.vault_mut(finished.id) else {
            return finished.result;
        };
        open.saving = false;
        match &finished.result {
            Ok(()) => {
                open.saved_edits = open.saved_edits.max(finished.edits);
                open.failed_save_at = None;
            }
            Err(_) => open.failed_save_at = Some(finished.edits),
        }
        finished.result
    }

    /// Starts saving every Vault with unsaved changes or a failed last save, except those
    /// already being saved.
    pub fn begin_saves(&mut self) -> Vec<PendingSave> {
        let due: Vec<VaultId> = self
            .vaults
            .iter()
            .filter(|open| (open.has_unsaved_changes() || open.save_failed()) && !open.saving)
            .map(|open| open.id)
            .collect();
        due.into_iter()
            .filter_map(|id| self.begin_save(id).ok())
            .collect()
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
    /// being saved. Call it after edits and after saves finish. A Vault whose last save failed
    /// is not retried until it is edited again.
    pub fn begin_autosaves(&mut self) -> Vec<PendingSave> {
        if !self.config.autosave {
            return Vec::new();
        }
        let due: Vec<VaultId> = self
            .vaults
            .iter()
            .filter(|open| {
                open.has_unsaved_changes()
                    && !open.saving
                    && open.failed_save_at != Some(open.edits)
            })
            .map(|open| open.id)
            .collect();
        due.into_iter()
            .filter_map(|id| self.begin_save(id).ok())
            .collect()
    }
}
