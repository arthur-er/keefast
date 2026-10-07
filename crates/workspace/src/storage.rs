use std::io;
use std::path::PathBuf;

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
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Vault file not found: {0:?}")]
    NotFound(VaultLocation),
    #[error("could not read the Vault file: {0}")]
    Io(#[from] io::Error),
}

/// Reads Vault files from wherever they live. Injected into the Workspace.
pub trait StorageProvider: Send + Sync {
    fn read(&self, location: &VaultLocation) -> Result<Vec<u8>, StorageError>;
}

/// The Storage provider for Vault files on the local filesystem.
pub struct LocalStorage;

impl StorageProvider for LocalStorage {
    fn read(&self, location: &VaultLocation) -> Result<Vec<u8>, StorageError> {
        let VaultLocation::Local(path) = location;
        std::fs::read(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(location.clone()),
            _ => StorageError::Io(error),
        })
    }
}
