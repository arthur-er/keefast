//! keefast's headless app model: the set of open Vaults, with its environment injected.
//! No GPUI here; the app renders this state and sends it commands.

mod storage;

use std::sync::Arc;

use keefast_core::{CompositeKey, Entry, GroupId, OpenError, Vault};

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

/// A Vault the Workspace has open, together with where its Vault file lives.
pub struct OpenVault {
    id: VaultId,
    location: VaultLocation,
    vault: Vault,
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

pub struct Workspace {
    storage: Arc<dyn StorageProvider>,
    vaults: Vec<OpenVault>,
    next_id: u64,
}

impl Workspace {
    pub fn new(storage: Arc<dyn StorageProvider>) -> Self {
        Self {
            storage,
            vaults: Vec::new(),
            next_id: 0,
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
        });
        id
    }

    pub fn vaults(&self) -> impl Iterator<Item = &OpenVault> {
        self.vaults.iter()
    }

    pub fn vault(&self, id: VaultId) -> Option<&OpenVault> {
        self.vaults.iter().find(|vault| vault.id == id)
    }

    /// The Entry list for a Vault: every Entry in the selected Group (the whole Vault when
    /// `None`) and its subgroups, sorted by title.
    pub fn visible_entries(&self, id: VaultId, group: Option<GroupId>) -> Vec<&Entry> {
        let Some(open) = self.vault(id) else {
            return Vec::new();
        };
        let root = open.vault.root();
        let Some(group) = group.map_or(Some(root), |group| open.vault.group(group)) else {
            return Vec::new();
        };
        let mut entries: Vec<&Entry> = group.all_entries().collect();
        entries.sort_by_cached_key(|entry| entry.title().to_lowercase());
        entries
    }
}
