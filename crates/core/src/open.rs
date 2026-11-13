use crate::{CompositeKey, EncryptionError, Vault, kdbx};

/// Why a Vault file could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("the master password or key file is wrong")]
    WrongKey,
    #[error("the Vault file is damaged or is not a Vault file: {0}")]
    Corrupt(String),
    #[error("this Vault file format is not supported: {0}")]
    Unsupported(String),
    /// The Vault file's key derivation settings cost more than keefast runs, far more than
    /// any client picks. Found before key derivation starts.
    #[error("this Vault file's key derivation costs more than keefast runs: {0}")]
    KdfOverLimit(EncryptionError),
}

impl Vault {
    /// Decrypts KDBX 3.1 or 4.x Vault file bytes. Runs key derivation, so keep it off the UI thread.
    pub fn open(bytes: &[u8], key: &CompositeKey) -> Result<Vault, OpenError> {
        let hash = key.hash();
        let file = kdbx::read(bytes, &hash)?;
        Vault::from_file(file, hash, key.parts())
    }
}
