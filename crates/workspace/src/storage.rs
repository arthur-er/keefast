use std::fs;
use std::io;
use std::path::PathBuf;

use crate::atomic_file;

/// Where a Vault file lives, in terms a Storage provider understands.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VaultLocation {
    Local(PathBuf),
}

impl VaultLocation {
    /// A short, human-readable name for the Vault file: its file name without extension.
    pub fn display_name(&self) -> String {
        match self {
            VaultLocation::Local(path) => path
                .file_stem()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
        }
    }

    /// Where the Vault file is, in full: its path, or its Storage provider reference.
    pub fn description(&self) -> String {
        match self {
            VaultLocation::Local(path) => path.display().to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Vault file not found: {0:?}")]
    NotFound(VaultLocation),
    #[error("could not access the Vault file: {0}")]
    Io(#[from] io::Error),
}

/// Reads and writes Vault files wherever they live. Injected into the Workspace.
pub trait StorageProvider: Send + Sync {
    fn read(&self, location: &VaultLocation) -> Result<Vec<u8>, StorageError>;

    /// The one location to know a Vault file by, so a Vault file reached two ways (a symlink,
    /// `..` in a path) is still recognised as the same Vault file.
    fn canonical(&self, location: &VaultLocation) -> VaultLocation {
        location.clone()
    }

    /// Replaces the Vault file with `bytes`, all or nothing: if writing fails, the Vault file
    /// is left as it was.
    fn write(&self, location: &VaultLocation, bytes: &[u8]) -> Result<(), StorageError>;
}

/// The Storage provider for Vault files on the local filesystem.
pub struct LocalStorage;

impl StorageProvider for LocalStorage {
    fn read(&self, location: &VaultLocation) -> Result<Vec<u8>, StorageError> {
        let VaultLocation::Local(path) = location;
        fs::read(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(location.clone()),
            _ => StorageError::Io(error),
        })
    }

    /// The absolute path with symlinks resolved, when the Vault file exists.
    fn canonical(&self, location: &VaultLocation) -> VaultLocation {
        let VaultLocation::Local(path) = location;
        fs::canonicalize(path).map_or_else(|_| location.clone(), VaultLocation::Local)
    }

    /// Saves through a temporary file and a rename, so a crash mid-save never leaves a
    /// half-written Vault file. A symlinked Vault file is saved where the link points.
    fn write(&self, location: &VaultLocation, bytes: &[u8]) -> Result<(), StorageError> {
        let VaultLocation::Local(path) = location;
        Ok(atomic_file::replace(path, bytes)?)
    }
}
