//! Creating Vault files, and changing an open Vault's master key, cipher and key derivation.
//!
//! A change is an edit like any other: the Vault file gets it when the Vault is next saved. Until
//! then the Vault keeps the keys it had before, one of which its Vault file has, so the save can
//! still open and Merge a version of the Vault file another device saved meanwhile. Once a save
//! has written the latest key, the old ones are forgotten (ADR 0004): a version saved elsewhere
//! with one later can no longer be Merged, and the save fails rather than overwrite it
//! (ADR 0006).

use std::sync::Arc;

use keefast_core::{Encryption, EncryptionError, SaveError, Timestamp, Vault};

use crate::{
    CachedFile, Fetched, NewKeyError, OpenVault, StorageError, StorageProvider, UnlockSecrets,
    UnlockedVault, VaultCache, VaultId, VaultLocation, Workspace,
};

/// What a new Vault is made with.
pub struct NewVault {
    /// The name of the Vault's top Group; the Vault file's name when empty.
    pub name: String,
    pub secrets: UnlockSecrets,
    pub encryption: Encryption,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateVaultError {
    #[error("a Vault from this Vault file is open")]
    AlreadyOpen(VaultId),
    #[error("a file is already there")]
    FileExists,
    #[error(transparent)]
    Key(#[from] NewKeyError),
    #[error(transparent)]
    Encryption(#[from] EncryptionError),
    #[error(transparent)]
    Save(#[from] SaveError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

#[derive(Debug, thiserror::Error)]
pub enum ChangeKeyError {
    #[error("the Vault is not open")]
    VaultNotOpen,
    #[error(transparent)]
    Key(#[from] NewKeyError),
    #[error(transparent)]
    Encryption(#[from] EncryptionError),
}

/// A create-Vault command that has not run yet. It owns everything it needs, so the slow part
/// (reading the key file, key derivation and writing) can run off the UI thread.
pub struct PendingCreate {
    storage: Arc<dyn StorageProvider>,
    /// The local cache, for a Vault file in an online Storage provider.
    cache: Option<Arc<dyn VaultCache>>,
    location: VaultLocation,
    new: NewVault,
    now: Timestamp,
}

impl PendingCreate {
    /// Writes the new Vault file, and returns its Vault to add with [`Workspace::add_unlocked`].
    /// A file already there is never replaced: it may be another Vault file.
    pub fn create(self) -> Result<UnlockedVault, CreateVaultError> {
        match self.storage.read(&self.location) {
            Ok(_) => return Err(CreateVaultError::FileExists),
            Err(StorageError::NotFound(_)) => {}
            Err(error) => return Err(error.into()),
        }
        let key = self.new.secrets.new_key()?;
        let vault = Vault::create(&self.new.name, &key, &self.new.encryption, self.now)?;
        let bytes = vault.save()?;
        let revision = self.storage.write(&self.location, &bytes, None)?;
        // Known by its canonical location now that it exists.
        let location = self.storage.canonical(&self.location);
        if let Some(cache) = &self.cache {
            // Best effort: without it, the Vault just won't open offline.
            let _ = cache.store(
                &location,
                &CachedFile {
                    bytes,
                    revision: Some(revision.clone()),
                    uploaded: true,
                },
            );
        }
        Ok(UnlockedVault {
            location,
            vault,
            revision: Some(revision),
            fetched: Fetched::FromProvider,
            os_locks: None,
        })
    }
}

impl Workspace {
    /// Starts creating a new Vault file at `location`. Run the returned command off the UI
    /// thread, then hand the new Vault to [`Self::add_unlocked`]. A Vault file whose Vault is
    /// open, unlocked or locked, is refused.
    pub fn begin_create(
        &self,
        location: VaultLocation,
        mut new: NewVault,
    ) -> Result<PendingCreate, CreateVaultError> {
        new.encryption.check()?;
        if new.name.trim().is_empty() {
            new.name = location.display_name();
        }
        let storage = self.storage_for(&location)?;
        let location = storage.canonical(&location);
        if let Some(open) = self
            .open_vault_at(&location)
            .or_else(|| self.locked_vault_at(&location))
        {
            return Err(CreateVaultError::AlreadyOpen(open));
        }
        Ok(PendingCreate {
            storage,
            cache: self.cache_for(&location),
            location,
            new,
            now: self.now(),
        })
    }

    /// Gives an open Vault a new master key: a master password, a key file, or both. Reads the
    /// key file, if any. The Vault file has the new key once the Vault is saved.
    pub fn change_master_key(
        &mut self,
        id: VaultId,
        secrets: &UnlockSecrets,
    ) -> Result<(), ChangeKeyError> {
        if self.vault(id).is_none() {
            return Err(ChangeKeyError::VaultNotOpen);
        }
        let key = secrets.new_key()?;
        let now = self.now();
        let open = self.vault_mut(id).ok_or(ChangeKeyError::VaultNotOpen)?;
        open.vault.change_key(&key, now);
        open.note_key_change();
        Ok(())
    }

    /// Sets the cipher and key derivation an open Vault is saved with. The Vault file has them
    /// once the Vault is saved. Settings the Vault has already change nothing.
    pub fn set_encryption(
        &mut self,
        id: VaultId,
        encryption: &Encryption,
    ) -> Result<(), ChangeKeyError> {
        let open = self.vault_mut(id).ok_or(ChangeKeyError::VaultNotOpen)?;
        if open.vault.encryption() == *encryption {
            return Ok(());
        }
        open.vault.set_encryption(encryption)?;
        open.edits += 1;
        Ok(())
    }
}

impl OpenVault {
    /// Counts a change of master key as an edit, remembering which, so the old key is
    /// forgotten once a save has caught up with it.
    fn note_key_change(&mut self) {
        self.edits += 1;
        self.key_changed_at = Some(self.edits);
    }

    /// After a save that caught up with the Vault at `edits`: forgets the keys the Vault had
    /// before the last change of key, if the save wrote it.
    pub(crate) fn key_change_saved(&mut self, edits: u64) {
        if self.key_changed_at.is_some_and(|changed| changed <= edits) {
            self.vault.forget_previous_keys();
            self.key_changed_at = None;
        }
    }
}
