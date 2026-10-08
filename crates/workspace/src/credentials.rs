use std::fs;
use std::io;
use std::path::PathBuf;

use keefast_core::CompositeKey;
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use crate::OpenVaultError;

/// What the user gave to unlock a Vault file: a master password, a key file on disk, or both.
#[derive(Clone)]
pub struct Credentials {
    password: SecretString,
    key_file: Option<PathBuf>,
}

impl Credentials {
    /// With a key file, an empty password means the key file alone, as in KeePassXC.
    pub fn new(password: SecretString, key_file: Option<PathBuf>) -> Self {
        Self { password, key_file }
    }

    pub fn password(password: impl Into<String>) -> Self {
        Self::new(SecretString::from(password.into()), None)
    }

    /// Reads the key file, if any, and combines it with the password. Blocks on the disk.
    pub(crate) fn composite_key(&self) -> Result<CompositeKey, OpenVaultError> {
        let password = self.password.expose_secret();
        let Some(path) = &self.key_file else {
            return Ok(CompositeKey::password(password));
        };
        let contents = Zeroizing::new(fs::read(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => OpenVaultError::KeyFileNotFound(path.clone()),
            _ => OpenVaultError::KeyFileUnreadable(error),
        })?);
        Ok(if password.is_empty() {
            CompositeKey::key_file(&contents)?
        } else {
            CompositeKey::password_and_key_file(password, &contents)?
        })
    }
}
