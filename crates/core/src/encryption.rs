//! How a Vault file is encrypted, and changing it: the master key, the cipher and the key
//! derivation function (KDF), and what the KDF costs on this machine.

use std::time::{Duration, Instant};

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
        self.kdf.check()
    }
}

impl Kdf {
    /// The most memory a [`Self::sample`] of Argon2 fills, in KiB.
    const SAMPLE_KIB: u64 = 16 * 1024;
    /// The most lanes a sample of Argon2 has: more than the cores of any machine keefast runs on,
    /// so the sample runs as many of them at once as the real derivation does.
    const SAMPLE_LANES: u32 = 64;
    /// The most rounds a sample of AES-KDF runs.
    const SAMPLE_ROUNDS: u64 = 100_000;

    /// Whether key derivation can run with these settings.
    pub fn check(&self) -> Result<(), EncryptionError> {
        match *self {
            Kdf::Argon2id(argon2) | Kdf::Argon2d(argon2) => argon2.check(),
            Kdf::AesKdf { rounds: 0 } => Err(EncryptionError::Rounds),
            Kdf::AesKdf { .. } => Ok(()),
        }
    }

    /// About how long deriving the key with these settings takes on this machine, which is what
    /// unlocking a Vault file costs, and saving it too. It times a [`Self::sample`] of the
    /// work and scales it up, so it takes a few tens of milliseconds whatever the settings: still
    /// too long for the UI thread.
    pub fn unlock_time(&self) -> Result<Duration, EncryptionError> {
        self.check()?;
        let (sample, scale) = self.sample();
        let started = Instant::now();
        // Checked settings always derive a key, and so do their samples.
        crate::kdbx::derive_throwaway_key(sample).expect("checked KDF settings derive a key");
        Ok(started.elapsed().mul_f64(scale))
    }

    /// A small run of this KDF, and how many times its work these settings do. Argon2's time
    /// grows with its memory times its iterations, so its sample is one iteration over at most
    /// [`Self::SAMPLE_KIB`], with the same lanes (up to [`Self::SAMPLE_LANES`]) to run them in
    /// parallel as the real one does; AES-KDF's grows with its rounds. Settings smaller than the
    /// sample are their own, at one iteration.
    pub(crate) fn sample(&self) -> (Kdf, f64) {
        let argon2 = |argon2: Argon2| {
            let kib = argon2.memory / 1024;
            let sample = Argon2 {
                memory: kib.min(Self::SAMPLE_KIB) * 1024,
                iterations: 1,
                parallelism: argon2.parallelism.min(Self::SAMPLE_LANES),
            };
            let scale = argon2.memory as f64 / sample.memory as f64 * argon2.iterations as f64;
            (sample, scale)
        };
        match *self {
            Kdf::Argon2id(settings) => {
                let (sample, scale) = argon2(settings);
                (Kdf::Argon2id(sample), scale)
            }
            Kdf::Argon2d(settings) => {
                let (sample, scale) = argon2(settings);
                (Kdf::Argon2d(sample), scale)
            }
            Kdf::AesKdf { rounds } => {
                let sample = rounds.min(Self::SAMPLE_ROUNDS);
                (
                    Kdf::AesKdf { rounds: sample },
                    rounds as f64 / sample as f64,
                )
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn argon2(memory: u64, iterations: u64, parallelism: u32) -> Argon2 {
        Argon2 {
            memory,
            iterations,
            parallelism,
        }
    }

    #[test]
    fn an_argon2_sample_is_one_iteration_over_less_memory_with_the_same_lanes() {
        let (sample, scale) = Kdf::Argon2id(Argon2::default()).sample();
        assert_eq!(sample, Kdf::Argon2id(argon2(16 * MIB, 1, 4)));
        // 64 MiB is 4 times the sample's memory, 5 times over.
        assert_eq!(scale, 20.0);

        let (sample, scale) = Kdf::Argon2d(argon2(4 * MIB, 3, 2)).sample();
        assert_eq!(sample, Kdf::Argon2d(argon2(4 * MIB, 1, 2)));
        assert_eq!(scale, 3.0);
    }

    #[test]
    fn an_argon2_sample_keeps_enough_memory_for_its_lanes() {
        let (sample, _) = Kdf::Argon2id(argon2(1024 * MIB, 1, 0xFF_FFFF)).sample();
        let Kdf::Argon2id(sample) = sample else {
            panic!("the sample is Argon2id")
        };
        assert_eq!(sample.parallelism, 64);
        assert_eq!(sample.check(), Ok(()));
    }

    #[test]
    fn an_aes_kdf_sample_runs_fewer_rounds() {
        assert_eq!(
            Kdf::AesKdf { rounds: 2_000_000 }.sample(),
            (Kdf::AesKdf { rounds: 100_000 }, 20.0)
        );
        assert_eq!(
            Kdf::AesKdf { rounds: 6_000 }.sample(),
            (Kdf::AesKdf { rounds: 6_000 }, 1.0)
        );
    }

    #[test]
    fn the_unlock_time_is_refused_for_settings_that_cannot_derive_a_key() {
        assert_eq!(
            Kdf::AesKdf { rounds: 0 }.unlock_time(),
            Err(EncryptionError::Rounds)
        );
        assert_eq!(
            Kdf::Argon2id(argon2(64 * MIB, 0, 4)).unlock_time(),
            Err(EncryptionError::Iterations)
        );
    }

    #[test]
    fn the_unlock_time_is_about_what_deriving_the_key_takes() {
        // Twice the sample's memory, three times over: six samples' work. The fastest of a few
        // tries of each, so other tests running at the same time don't decide it.
        let kdf = Kdf::Argon2id(argon2(32 * MIB, 3, 2));
        let fastest = |time: &dyn Fn() -> Duration| (0..3).map(|_| time()).min().unwrap();
        let estimate = fastest(&|| kdf.unlock_time().unwrap());
        let actual = fastest(&|| {
            let started = Instant::now();
            crate::kdbx::derive_throwaway_key(kdf).unwrap();
            started.elapsed()
        });
        let ratio = estimate.as_secs_f64() / actual.as_secs_f64();
        assert!(
            (0.4..2.5).contains(&ratio),
            "estimated {estimate:?} for {actual:?}"
        );
    }
}
