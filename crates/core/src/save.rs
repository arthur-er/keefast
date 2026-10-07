use crate::{OpenError, Vault, kdbx};

/// Why a Vault could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("the Vault's encryption settings cannot be used to save it: {0}")]
    Settings(String),
}

impl Vault {
    /// Encrypts the Vault as KDBX 4 Vault file bytes, with the key, cipher and key derivation
    /// settings it was opened with and fresh random seeds. Everything in the Vault file it was
    /// opened from is kept, including what keefast doesn't model. Runs key derivation, so keep
    /// it off the UI thread.
    pub fn save(&self) -> Result<Vec<u8>, SaveError> {
        kdbx::write(&self.file, &self.key).map_err(|error| match error {
            OpenError::WrongKey => unreachable!("writing never checks the key"),
            OpenError::Corrupt(reason) | OpenError::Unsupported(reason) => {
                SaveError::Settings(reason)
            }
        })
    }
}
