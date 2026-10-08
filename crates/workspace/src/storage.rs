use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
    #[error("could not access the Vault file: {0}")]
    Io(#[from] io::Error),
}

/// Reads and writes Vault files wherever they live. Injected into the Workspace.
pub trait StorageProvider: Send + Sync {
    fn read(&self, location: &VaultLocation) -> Result<Vec<u8>, StorageError>;

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

    /// Writes a temporary file next to the Vault file, flushes it to disk, then renames it over
    /// the Vault file, so a crash mid-save never leaves a half-written Vault file.
    fn write(&self, location: &VaultLocation, bytes: &[u8]) -> Result<(), StorageError> {
        let VaultLocation::Local(path) = location;
        let temp = temp_path(path);
        let result = write_and_rename(&temp, path, bytes);
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        Ok(result?)
    }
}

fn write_and_rename(temp: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    if let Ok(existing) = fs::metadata(path) {
        file.set_permissions(existing.permissions())?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)?;
    sync_parent(path);
    Ok(())
}

/// A hidden, unique name in the Vault file's directory, so the rename stays on one filesystem.
fn temp_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = format!(
        ".{name}.{}-{}.keefast-tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    path.with_file_name(unique)
}

/// Makes the rename itself durable. Best effort: not every platform can sync a directory.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        if let Ok(dir) = fs::File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}
