//! The OS secret store, where keefast keeps what must not sit in the app config: Dropbox tokens.

use secrecy::{ExposeSecret, SecretString};

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("the OS secret store is not available: {0}")]
    Unavailable(String),
    #[error("the OS secret store failed: {0}")]
    Failed(String),
}

/// Keeps secrets by name. Injected, so tests use a fake rather than the user's keyring.
pub trait SecretStore: Send + Sync {
    /// The secret kept under `name`, or `None` if there is none.
    fn get(&self, name: &str) -> Result<Option<SecretString>, SecretStoreError>;

    fn set(&self, name: &str, secret: &SecretString) -> Result<(), SecretStoreError>;

    /// Removes the secret kept under `name`. Removing one that isn't there succeeds.
    fn delete(&self, name: &str) -> Result<(), SecretStoreError>;
}

/// The OS secret store: Windows Credential Manager, or the Secret Service on Linux.
pub struct OsSecretStore;

impl OsSecretStore {
    /// The service the secrets are filed under.
    const SERVICE: &str = "keefast";

    fn entry(name: &str) -> Result<keyring::Entry, SecretStoreError> {
        keyring::Entry::new(Self::SERVICE, name).map_err(failed)
    }
}

fn failed(error: keyring::Error) -> SecretStoreError {
    match error {
        keyring::Error::NoDefaultStore
        | keyring::Error::NoStorageAccess(_)
        | keyring::Error::PlatformFailure(_) => SecretStoreError::Unavailable(error.to_string()),
        error => SecretStoreError::Failed(error.to_string()),
    }
}

impl SecretStore for OsSecretStore {
    fn get(&self, name: &str) -> Result<Option<SecretString>, SecretStoreError> {
        match Self::entry(name)?.get_password() {
            Ok(secret) => Ok(Some(SecretString::from(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(failed(error)),
        }
    }

    fn set(&self, name: &str, secret: &SecretString) -> Result<(), SecretStoreError> {
        Self::entry(name)?
            .set_password(secret.expose_secret())
            .map_err(failed)
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(failed(error)),
        }
    }
}
