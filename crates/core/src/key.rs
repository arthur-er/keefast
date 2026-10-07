use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::kdbx::Key;

/// The secrets that unlock a Vault file. Only a master password for now; key files come later.
#[derive(Clone)]
pub struct CompositeKey {
    password: SecretString,
}

impl CompositeKey {
    pub fn password(password: impl Into<String>) -> Self {
        Self {
            password: SecretString::from(password.into()),
        }
    }

    /// KeePass's composite key: SHA-256 over the SHA-256 of each component.
    pub(crate) fn hash(&self) -> Key {
        let password: Zeroizing<[u8; 32]> =
            Zeroizing::new(Sha256::digest(self.password.expose_secret().as_bytes()).into());
        Zeroizing::new(Sha256::digest(&password[..]).into())
    }
}
