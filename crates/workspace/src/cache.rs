//! The encrypted local cache of remote Vault files: the Vault file's bytes and the revision
//! they were last synced with, so a Dropbox Vault opens without a network, and saves made
//! offline are kept until they can be uploaded. The bytes are the Vault file itself, encrypted
//! with the Vault's key; nothing decrypted is ever cached (ADR 0004).

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::atomic_file;
use crate::storage::{Revision, VaultLocation};

/// A remote Vault file as this machine last had it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedFile {
    pub bytes: Vec<u8>,
    /// The revision `bytes` were downloaded at, or, if not uploaded, were based on.
    pub revision: Option<Revision>,
    /// Whether the remote Vault file has everything `bytes` has. A save made offline isn't
    /// uploaded until keefast is back online.
    pub uploaded: bool,
}

/// Keeps remote Vault files on this machine. Injected into the Workspace.
pub trait VaultCache: Send + Sync {
    /// The cached Vault file at `location`, if one is cached and readable.
    fn load(&self, location: &VaultLocation) -> Option<CachedFile>;

    /// Caches `file` for `location`, all or nothing, replacing what was cached.
    fn store(&self, location: &VaultLocation, file: &CachedFile) -> io::Result<()>;
}

/// The cache in a directory: one file per Vault file, named by a hash of its location.
pub struct FileVaultCache {
    dir: PathBuf,
}

/// The first line of every cache file, followed by its metadata as one line of JSON, then the
/// Vault file's bytes.
const MAGIC: &[u8] = b"keefast vault cache 1\n";

#[derive(Serialize, Deserialize)]
struct Metadata {
    revision: Option<String>,
    uploaded: bool,
}

impl FileVaultCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The cache in the OS's local data directory. Not the OS cache directory: a save made
    /// offline lives only here until it is uploaded, so it mustn't be cleaned up.
    pub fn in_os_data_dir() -> Option<Self> {
        Some(Self::new(
            dirs::data_local_dir()?.join("keefast").join("vault-cache"),
        ))
    }

    fn path(&self, location: &VaultLocation) -> PathBuf {
        let key = match location {
            VaultLocation::Local(path) => format!("local:{}", path.display()),
            // Dropbox paths are case-insensitive.
            VaultLocation::Dropbox(path) => format!("dropbox:{}", path.to_lowercase()),
        };
        let hash = Sha256::digest(key.as_bytes());
        let name: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
        self.dir.join(name)
    }
}

impl VaultCache for FileVaultCache {
    fn load(&self, location: &VaultLocation) -> Option<CachedFile> {
        let contents = fs::read(self.path(location)).ok()?;
        let rest = contents.strip_prefix(MAGIC)?;
        let end = rest.iter().position(|&byte| byte == b'\n')?;
        let metadata: Metadata = serde_json::from_slice(&rest[..end]).ok()?;
        Some(CachedFile {
            bytes: rest[end + 1..].to_vec(),
            revision: metadata.revision.map(Revision::new),
            uploaded: metadata.uploaded,
        })
    }

    fn store(&self, location: &VaultLocation, file: &CachedFile) -> io::Result<()> {
        create_private_dir(&self.dir)?;
        let metadata = serde_json::to_vec(&Metadata {
            revision: file
                .revision
                .as_ref()
                .map(|revision| revision.as_str().to_owned()),
            uploaded: file.uploaded,
        })
        .map_err(io::Error::other)?;
        let mut contents = Vec::with_capacity(MAGIC.len() + metadata.len() + 1 + file.bytes.len());
        contents.extend_from_slice(MAGIC);
        contents.extend_from_slice(&metadata);
        contents.push(b'\n');
        contents.extend_from_slice(&file.bytes);
        atomic_file::replace(&self.path(location), &contents)
    }
}

/// Creates the cache directory, readable by the user only where the OS lets keefast say so.
fn create_private_dir(dir: &std::path::Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}
