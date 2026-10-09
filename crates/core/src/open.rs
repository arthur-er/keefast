use crate::{CompositeKey, Vault, kdbx};

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
        let key = key.hash();
        let file = kdbx::read(bytes, &key)?;
        Vault::from_file(file, key)
    }
}
