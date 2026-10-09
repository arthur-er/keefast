//! How a Vault file is encrypted, and changing it: the master key, the cipher and the key
//! derivation function (KDF).

use crate::edit::{child_or_insert, set_value};
use crate::kdbx;
use crate::{CompositeKey, Timestamp, Vault};

/// The cipher that encrypts a Vault file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cipher {
    Aes256,
    ChaCha20,
    Twofish,
}

/// How the master key is turned into the key that encrypts a Vault file: the deliberately slow
/// step that makes guessing the master password expensive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kdf {
    /// Argon2id, which resists both GPU cracking and side channels. keefast's default.
    Argon2id(Argon2),
    /// Argon2d, KeePassXC's default.
    Argon2d(Argon2),
    /// Repeated AES encryption, the only KDF of KDBX 3.1.
    AesKdf { rounds: u64 },
}

/// The cost of an Argon2 key derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argon2 {
    /// In bytes, a whole number of KiB, as KDBX stores it.
    pub memory: u64,
    pub iterations: u64,
    /// Lanes, which other clients run in parallel.
    pub parallelism: u32,
}

/// The cipher and key derivation a Vault file is encrypted with. Saving always writes KDBX 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encryption {
    pub cipher: Cipher,
    pub kdf: Kdf,
}

/// Why encryption settings can't be used: key derivation could not run with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EncryptionError {
    #[error("Argon2 needs at least one iteration")]
    Iterations,
    #[error("Argon2 needs 1 to 16777215 lanes")]
    Parallelism,
    #[error("Argon2 needs a whole number of KiB of memory, at least 8 KiB per lane")]
    Memory,
    #[error("AES-KDF needs at least one round")]
    Rounds,
}

impl Argon2 {
    /// Argon2's own limits: the most lanes it allows, and the least memory per lane.
    const MAX_LANES: u32 = 0xFF_FFFF;
    const MIN_KIB_PER_LANE: u64 = 8;

    fn check(&self) -> Result<(), EncryptionError> {
        if self.iterations == 0 || u32::try_from(self.iterations).is_err() {
            return Err(EncryptionError::Iterations);
        }
        if !(1..=Self::MAX_LANES).contains(&self.parallelism) {
            return Err(EncryptionError::Parallelism);
        }
        let kib = self.memory / 1024;
        if !self.memory.is_multiple_of(1024)
            || kib < Self::MIN_KIB_PER_LANE * u64::from(self.parallelism)
            || u32::try_from(kib).is_err()
        {
            return Err(EncryptionError::Memory);
        }
        Ok(())
    }
}

impl Default for Argon2 {
    /// 64 MiB and 5 iterations: about half a second on a recent PC, each time the Vault is
    /// unlocked or saved.
    fn default() -> Self {
        Self {
            memory: 64 * 1024 * 1024,
            iterations: 5,
            parallelism: 4,
        }
    }
}

impl Default for Encryption {
    /// AES-256, which every KeePass client reads, with Argon2id at its default cost.
    fn default() -> Self {
        Self {
            cipher: Cipher::Aes256,
            kdf: Kdf::Argon2id(Argon2::default()),
        }
    }
}

impl Encryption {
    /// Whether key derivation can run with these settings.
    pub fn check(&self) -> Result<(), EncryptionError> {
        match self.kdf {
            Kdf::Argon2id(argon2) | Kdf::Argon2d(argon2) => argon2.check(),
            Kdf::AesKdf { rounds: 0 } => Err(EncryptionError::Rounds),
            Kdf::AesKdf { .. } => Ok(()),
        }
    }
}

impl Vault {
    /// The cipher and key derivation the Vault is saved with.
    pub fn encryption(&self) -> Encryption {
        self.file.settings.encryption()
    }

    /// Saves the Vault with this cipher and key derivation from now on. Settings of the KDF it
    /// has already that keefast doesn't know are kept, as long as the KDF stays the same.
    pub fn set_encryption(&mut self, encryption: &Encryption) -> Result<(), EncryptionError> {
        encryption.check()?;
        self.file.settings.set_encryption(encryption);
        Ok(())
    }

    /// Saves the Vault with this master key from now on.
    ///
    /// The keys the Vault had before stay known, so that versions of the Vault file saved with
    /// any of them, here or elsewhere, can still be opened to Merge them, until
    /// [`Self::forget_previous_keys`] once the Vault file has the new key.
    pub fn change_key(&mut self, key: &CompositeKey, now: Timestamp) {
        let new = key.hash();
        let old = std::mem::replace(&mut self.key, new);
        self.previous_keys.push(old);
        let meta = child_or_insert(&mut self.file.document, "Meta", &[]);
        let changed = child_or_insert(meta, "MasterKeyChanged", &["Color", "DatabaseName"]);
        set_value(changed, &now.encode(), false);
    }

    /// Forgets the keys the Vault had before [`Self::change_key`], once the Vault file has the
    /// new one. Versions of the Vault file saved with an old key no longer open.
    pub fn forget_previous_keys(&mut self) {
        self.previous_keys.clear();
    }
}

/// Builds the header settings of a new Vault file.
pub(crate) fn new_settings(encryption: &Encryption) -> kdbx::Settings {
    let mut settings = kdbx::Settings::new_kdbx4();
    settings.set_encryption(encryption);
    settings
}
