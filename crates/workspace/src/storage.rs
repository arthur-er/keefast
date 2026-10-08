use std::any::Any;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};

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

/// One version of a Vault file, as its Storage provider tells versions apart: a content hash
/// for local files, a revision ID for Dropbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Revision(String);

impl Revision {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// A Vault file's bytes, and which version of it they are.
pub struct StoredFile {
    pub bytes: Vec<u8>,
    pub revision: Revision,
}

/// Keeps a Storage provider watching a Vault file for as long as it is held.
pub struct Watch(#[allow(dead_code)] Box<dyn Any + Send>);

impl Watch {
    /// Keeps `watcher` alive; dropping the Watch drops it.
    pub fn new(watcher: impl Any + Send) -> Self {
        Self(Box::new(watcher))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Vault file not found: {0:?}")]
    NotFound(VaultLocation),
    #[error("the Vault file was changed elsewhere meanwhile")]
    Conflict,
    #[error("could not access the Vault file: {0}")]
    Io(#[from] io::Error),
}

/// Reads and writes Vault files wherever they live. Injected into the Workspace.
pub trait StorageProvider: Send + Sync {
    fn read(&self, location: &VaultLocation) -> Result<StoredFile, StorageError>;

    /// The one location to know a Vault file by, so a Vault file reached two ways (a symlink,
    /// `..` in a path) is still recognised as the same Vault file.
    fn canonical(&self, location: &VaultLocation) -> VaultLocation {
        location.clone()
    }

    /// Replaces the Vault file with `bytes`, all or nothing: if writing fails, the Vault file
    /// is left as it was. With an `expected` revision, the Vault file is replaced only if it is
    /// still at that revision, or gone, and [`StorageError::Conflict`] returned otherwise, so a
    /// change made elsewhere is never overwritten unseen. Returns the revision written.
    fn write(
        &self,
        location: &VaultLocation,
        bytes: &[u8],
        expected: Option<&Revision>,
    ) -> Result<Revision, StorageError>;

    /// Calls `on_change` whenever the Vault file may have changed, until the returned Watch is
    /// dropped. Writes of the provider's own trigger it too. `None` when the provider can't
    /// watch the Vault file.
    fn watch(
        &self,
        _location: &VaultLocation,
        _on_change: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<Watch> {
        None
    }
}

/// The Storage provider for Vault files on the local filesystem.
pub struct LocalStorage;

impl StorageProvider for LocalStorage {
    fn read(&self, location: &VaultLocation) -> Result<StoredFile, StorageError> {
        let VaultLocation::Local(path) = location;
        let bytes = fs::read(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(location.clone()),
            _ => StorageError::Io(error),
        })?;
        let revision = content_revision(&bytes);
        Ok(StoredFile { bytes, revision })
    }

    /// The absolute path with symlinks resolved, when the Vault file exists.
    fn canonical(&self, location: &VaultLocation) -> VaultLocation {
        let VaultLocation::Local(path) = location;
        fs::canonicalize(path).map_or_else(|_| location.clone(), VaultLocation::Local)
    }

    /// Saves through a temporary file and a rename, so a crash mid-save never leaves a
    /// half-written Vault file. A symlinked Vault file is saved where the link points.
    /// The check against `expected` runs just before the rename, but the filesystem can't
    /// make the two one step: a change landing in between is overwritten.
    fn write(
        &self,
        location: &VaultLocation,
        bytes: &[u8],
        expected: Option<&Revision>,
    ) -> Result<Revision, StorageError> {
        let VaultLocation::Local(path) = location;
        let unchanged = |path: &std::path::Path| match (expected, fs::read(path)) {
            (None, _) => Ok(true),
            (Some(expected), Ok(current)) => Ok(content_revision(&current) == *expected),
            (Some(_), Err(error)) if error.kind() == io::ErrorKind::NotFound => Ok(true),
            (Some(_), Err(error)) => Err(error),
        };
        match atomic_file::replace_if(path, bytes, unchanged)? {
            true => Ok(content_revision(bytes)),
            false => Err(StorageError::Conflict),
        }
    }

    /// Watches the Vault file's directory, so the Vault file being replaced by a rename, as
    /// sync tools and keefast itself save, is seen too.
    fn watch(
        &self,
        location: &VaultLocation,
        on_change: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<Watch> {
        let VaultLocation::Local(path) = location;
        let path = fs::canonicalize(path).ok()?;
        let name = path.file_name()?.to_owned();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let relevant = match event {
                    Ok(event) => {
                        // Reading the Vault file, as checking it for changes does, isn't one.
                        let read = matches!(
                            event.kind,
                            EventKind::Access(
                                AccessKind::Read
                                    | AccessKind::Open(_)
                                    | AccessKind::Close(AccessMode::Read)
                            )
                        );
                        !read
                            && event
                                .paths
                                .iter()
                                .any(|changed| changed.file_name() == Some(&name))
                    }
                    // Events may have been missed.
                    Err(_) => true,
                };
                if relevant {
                    on_change();
                }
            })
            .ok()?;
        watcher
            .watch(path.parent()?, RecursiveMode::NonRecursive)
            .ok()?;
        Some(Watch::new(watcher))
    }
}

fn content_revision(bytes: &[u8]) -> Revision {
    let hash = Sha256::digest(bytes);
    Revision(hash.iter().map(|byte| format!("{byte:02x}")).collect())
}
