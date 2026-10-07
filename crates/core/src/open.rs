use keepass::config::DatabaseVersion;
use keepass::db::DatabaseOpenError;
use keepass::error::DatabaseKeyError;

use crate::{CompositeKey, Vault};

/// Why a Vault file could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("the master password or key file is wrong")]
    WrongKey,
    #[error("the Vault file is damaged or is not a Vault file: {0}")]
    Corrupt(String),
    #[error("this Vault file format is not supported: {0}")]
    Unsupported(String),
}

impl Vault {
    /// Decrypts KDBX 3.1 or 4.x Vault file bytes. Runs key derivation, so keep it off the UI thread.
    pub fn open(bytes: &[u8], key: &CompositeKey) -> Result<Vault, OpenError> {
        match DatabaseVersion::parse(bytes) {
            Ok(DatabaseVersion::KDB3(_) | DatabaseVersion::KDB4(_)) => {}
            Ok(version) => return Err(OpenError::Unsupported(version.to_string())),
            Err(error) => return Err(OpenError::Corrupt(error.to_string())),
        }
        let database = keepass::Database::parse(bytes, key.to_database_key()).map_err(classify)?;
        Ok(Vault::from_database(&database))
    }
}

fn classify(error: DatabaseOpenError) -> OpenError {
    match error {
        DatabaseOpenError::Key(DatabaseKeyError::IncorrectKey) => OpenError::WrongKey,
        DatabaseOpenError::UnsupportedVersion => OpenError::Unsupported(error.to_string()),
        other => OpenError::Corrupt(other.to_string()),
    }
}
