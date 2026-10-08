//! keefast's headless app model: the set of open Vaults, with its environment injected.
//! No GPUI here; the app renders this state and sends it commands.

mod clock;
mod storage;

use std::sync::Arc;

use keefast_core::{
    CompositeKey, EditError, Entry, EntryFields, EntryId, Group, GroupId, OpenError, SaveError,
    Timestamp, Vault,
};

pub use clock::{Clock, SystemClock};
pub use storage::{LocalStorage, StorageError, StorageProvider, VaultLocation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VaultId(u64);

#[derive(Debug, thiserror::Error)]
pub enum OpenVaultError {
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
    autosave: bool,
}

impl Workspace {
    pub fn new(storage: Arc<dyn StorageProvider>) -> Self {
        Self::with_clock(storage, Arc::new(SystemClock))
    }

    pub fn with_clock(storage: Arc<dyn StorageProvider>, clock: Arc<dyn Clock>) -> Self {
        Self {
            storage,
            clock,
            vaults: Vec::new(),
            next_id: 0,
            autosave: false,
        }
    }

    /// Reads, decrypts and adds a Vault in one blocking call.
    pub fn open_vault(
        &mut self,
        location: &VaultLocation,
        key: &CompositeKey,
    ) -> Result<VaultId, OpenVaultError> {
        let pending = self.begin_open(location.clone(), key.clone());
        Ok(self.add_unlocked(pending.unlock()?))
    }

    pub fn begin_open(&self, location: VaultLocation, key: CompositeKey) -> PendingOpen {
        PendingOpen {
            storage: Arc::clone(&self.storage),
            location,
            key,
        }
    }

    pub fn add_unlocked(&mut self, unlocked: UnlockedVault) -> VaultId {
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

    /// The Entry list for a Vault: every Entry in the selected Group (the whole Vault when
    /// `None`) and its subgroups, sorted by title.
    pub fn visible_entries(&self, id: VaultId, group: Option<GroupId>) -> Vec<&Entry> {
        let group = match group {
            Some(group) => self.group(id, group),
            None => self.vault(id).map(|open| open.vault.root()),
        };
        let Some(group) = group else {
            return Vec::new();
        };
        let mut entries: Vec<&Entry> = group.all_entries().collect();
        entries.sort_by_cached_key(|entry| entry.title().to_lowercase());
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
        self.autosave
    }

    /// Turns saving after every edit on or off.
    pub fn set_autosave(&mut self, autosave: bool) {
        self.autosave = autosave;
    }

    /// With autosave on, starts saving every Vault with unsaved changes that isn't already
    /// being saved. Call it after edits and after saves finish. A Vault whose last save failed
    /// is not retried until it is edited again.
    pub fn begin_autosaves(&mut self) -> Vec<PendingSave> {
        if !self.autosave {
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
