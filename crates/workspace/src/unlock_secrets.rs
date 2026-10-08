use std::fs;
use std::io;
use std::path::PathBuf;

use keefast_core::CompositeKey;
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use crate::OpenVaultError;

/// What the user gave to unlock a Vault file: a master password, a key file on disk, or both.
#[derive(Clone)]
pub struct UnlockSecrets {
    password: SecretString,
    key_file: Option<PathBuf>,
}

impl UnlockSecrets {
    pub fn new(password: SecretString, key_file: Option<PathBuf>) -> Self {
        Self { password, key_file }
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
        let Some(path) = &self.key_file else {
            return Ok(vec![CompositeKey::password(password)]);
        };
        let contents = Zeroizing::new(fs::read(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => OpenVaultError::KeyFileNotFound(path.clone()),
            _ => OpenVaultError::KeyFileUnreadable(error),
        })?);
        let with_password = CompositeKey::password_and_key_file(password, &contents)?;
        Ok(if password.is_empty() {
            vec![CompositeKey::key_file(&contents)?, with_password]
        } else {
            vec![with_password]
        })
    }
}
