use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use keefast_core::{CompositeKey, KeyFileError, new_key_file};
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use crate::OpenVaultError;

/// What the user gave to unlock a Vault file: a master password, a key file on disk, or both.
/// Also what they give as the master key of a new Vault, or as an open Vault's new master key.
#[derive(Clone)]
pub struct UnlockSecrets {
    password: SecretString,
    key_file: Option<KeyFile>,
}

/// Where a key file comes from.
#[derive(Clone)]
enum KeyFile {
    /// Read from disk when the Vault file is unlocked.
    Path(PathBuf),
    /// Already read, as by `keefast-agent unlock`, which reads the operator's key file and
    /// hands its contents to a Headless host that may not be able to read it (ADR 0012).
    Contents(Zeroizing<Vec<u8>>),
}

/// Why secrets can't be a Vault's new master key.
#[derive(Debug, thiserror::Error)]
pub enum NewKeyError {
    #[error("a master key needs a master password, a key file, or both")]
    NoKey,
    #[error("key file not found: {0}")]
    KeyFileNotFound(PathBuf),
    #[error("could not read the key file: {0}")]
    KeyFileUnreadable(#[source] io::Error),
    #[error(transparent)]
    KeyFile(#[from] KeyFileError),
}

/// A key file that could not be read.
struct KeyFileRead {
    path: PathBuf,
    error: io::Error,
}

impl From<KeyFileRead> for OpenVaultError {
    fn from(KeyFileRead { path, error }: KeyFileRead) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => OpenVaultError::KeyFileNotFound(path),
            _ => OpenVaultError::KeyFileUnreadable(error),
        }
    }
}

impl From<KeyFileRead> for NewKeyError {
    fn from(KeyFileRead { path, error }: KeyFileRead) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => NewKeyError::KeyFileNotFound(path),
            _ => NewKeyError::KeyFileUnreadable(error),
        }
    }
}

impl UnlockSecrets {
    pub fn new(password: SecretString, key_file: Option<PathBuf>) -> Self {
        Self {
            password,
            key_file: key_file.map(KeyFile::Path),
        }
    }

    /// Secrets whose key file was read already, given as its contents.
    pub fn with_key_file_contents(password: SecretString, contents: Zeroizing<Vec<u8>>) -> Self {
        Self {
            password,
            key_file: Some(KeyFile::Contents(contents)),
        }
    }

    pub fn password(password: impl Into<String>) -> Self {
        Self::new(SecretString::from(password.into()), None)
    }

    /// The composite keys to try, in order. Reads the key file, if any, so it blocks on the disk.
    ///
    /// With a key file, an empty password means the key file alone, as in KeePassXC. KeePass 2
    /// can also make a Vault file with an empty password plus a key file, so that is tried next.
    pub(crate) fn composite_keys(&self) -> Result<Vec<CompositeKey>, OpenVaultError> {
        let password = self.password.expose_secret();
        let Some(contents) = self.read_key_file()? else {
            return Ok(vec![CompositeKey::password(password)]);
        };
        let with_password = CompositeKey::password_and_key_file(password, &contents)?;
        Ok(if password.is_empty() {
            vec![CompositeKey::key_file(&contents)?, with_password]
        } else {
            vec![with_password]
        })
    }

    /// The master key these secrets make for a new Vault, or as an open Vault's new key: the
    /// key file alone when the password is empty, as KeePassXC makes it. Reads the key file,
    /// if any, so it blocks on the disk.
    pub(crate) fn new_key(&self) -> Result<CompositeKey, NewKeyError> {
        let password = self.password.expose_secret();
        match (self.read_key_file()?, password.is_empty()) {
            (None, true) => Err(NewKeyError::NoKey),
            (None, false) => Ok(CompositeKey::password(password)),
            (Some(contents), true) => Ok(CompositeKey::key_file(&contents)?),
            (Some(contents), false) => {
                Ok(CompositeKey::password_and_key_file(password, &contents)?)
            }
        }
    }

    fn read_key_file(&self) -> Result<Option<Zeroizing<Vec<u8>>>, KeyFileRead> {
        let path = match &self.key_file {
            None => return Ok(None),
            Some(KeyFile::Contents(contents)) => return Ok(Some(contents.clone())),
            Some(KeyFile::Path(path)) => path,
        };
        let contents = fs::read(path).map_err(|error| KeyFileRead {
            path: path.clone(),
            error,
        })?;
        Ok(Some(Zeroizing::new(contents)))
    }
}

/// Writes a new key file with a random key at `path`, in KeePass's XML v2.0 format. An existing
/// file is never replaced, since it may be another Vault's key file.
pub fn write_new_key_file(path: &Path) -> io::Result<()> {
    let contents = new_key_file();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&contents)?;
    file.sync_all()
}
