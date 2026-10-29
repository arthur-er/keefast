//! The cryptography of the KDBX container: outer ciphers, key derivation, block streams and the
//! inner random stream that protects values inside the XML.

use aes::Aes256;
use cipher::block_padding::NoPadding;
use cipher::{
    BlockCipherEncrypt, BlockModeDecrypt, BlockModeEncrypt, KeyInit, KeyIvInit, StreamCipher,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use subtle::ConstantTimeEq;
use twofish::Twofish;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::header::VariantDictionary;
use crate::{Argon2, Kdf, OpenError};

pub(crate) const AES256: Uuid = Uuid::from_u128(0x31c1f2e6_bf71_4350_be58_05216afc5aff);
pub(crate) const CHACHA20: Uuid = Uuid::from_u128(0xd6038a2b_8b6f_4cb5_a524_339a31dbb59a);
pub(crate) const TWOFISH: Uuid = Uuid::from_u128(0xad68f29f_576f_4bb9_a36a_d47af965346c);

/// AES-KDF as KeePass names it, and as KDBX 3.1 implies it.
pub(crate) const AES_KDF: Uuid = Uuid::from_u128(0xc9d9f39a_628a_4460_bf74_0d08c18a4fea);
/// AES-KDF as KeePassXC names it in KDBX 4.
pub(crate) const AES_KDF_KDBX4: Uuid = Uuid::from_u128(0x7c02bb82_79a7_4ac0_927d_114a00648238);
pub(crate) const ARGON2D: Uuid = Uuid::from_u128(0xef636ddf_8c29_444b_91f7_a9a403e30a0c);
pub(crate) const ARGON2ID: Uuid = Uuid::from_u128(0x9e298b19_56db_4773_b23d_fc3ec6f0a1e6);

pub(crate) type Key = Zeroizing<[u8; 32]>;

pub(crate) fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    digest.finalize().into()
}

pub(crate) fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut digest = Sha512::new();
    for part in parts {
        digest.update(part);
    }
    digest.finalize().into()
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes keys of any length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// Whether a MAC, hash or key check matches the expected one, in time that doesn't depend on
/// where they first differ, so an attacker can't learn the expected bytes one at a time.
pub(crate) fn matches(found: &[u8], expected: &[u8]) -> bool {
    found.ct_eq(expected).into()
}

pub(crate) fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the OS random number generator is available");
    bytes
}

/// The cipher that encrypts the whole payload of a Vault file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OuterCipher {
    Aes256,
    ChaCha20,
    Twofish,
}

impl OuterCipher {
    pub(crate) fn from_uuid(uuid: Uuid) -> Result<Self, OpenError> {
        match uuid {
            AES256 => Ok(Self::Aes256),
            CHACHA20 => Ok(Self::ChaCha20),
            TWOFISH => Ok(Self::Twofish),
            other => Err(OpenError::Unsupported(format!("cipher {other}"))),
        }
    }

    pub(crate) fn uuid(self) -> Uuid {
        match self {
            Self::Aes256 => AES256,
            Self::ChaCha20 => CHACHA20,
            Self::Twofish => TWOFISH,
        }
    }

    pub(crate) fn iv_len(self) -> usize {
        match self {
            Self::Aes256 | Self::Twofish => 16,
            Self::ChaCha20 => 12,
        }
    }

    pub(crate) fn encrypt(self, key: &[u8; 32], iv: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let invalid = "key and IV lengths are checked when the header is read";
        match self {
            Self::Aes256 => cbc::Encryptor::<Aes256>::new_from_slices(key, iv)
                .expect(invalid)
                .encrypt_padded_vec::<NoPadding>(&pkcs7_pad(plaintext)),
            Self::Twofish => cbc::Encryptor::<Twofish>::new_from_slices(key, iv)
                .expect(invalid)
                .encrypt_padded_vec::<NoPadding>(&pkcs7_pad(plaintext)),
            Self::ChaCha20 => {
                let mut data = plaintext.to_vec();
                chacha20::ChaCha20::new_from_slices(key, iv)
                    .expect(invalid)
                    .apply_keystream(&mut data);
                data
            }
        }
    }

    /// Decrypts without removing block padding, so the caller can tell a wrong key (checked
    /// first) from a damaged file (bad padding, see [`pkcs7_unpad`]).
    pub(crate) fn decrypt_raw(
        self,
        key: &[u8; 32],
        iv: &[u8],
        ciphertext: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, OpenError> {
        let corrupt = |_| OpenError::Corrupt("the encrypted payload is truncated".into());
        let bad_iv = |_| OpenError::Corrupt("the encryption IV has the wrong length".into());
        let plaintext = match self {
            Self::Aes256 => cbc::Decryptor::<Aes256>::new_from_slices(key, iv)
                .map_err(bad_iv)?
                .decrypt_padded_vec::<NoPadding>(ciphertext)
                .map_err(corrupt)?,
            Self::Twofish => cbc::Decryptor::<Twofish>::new_from_slices(key, iv)
                .map_err(bad_iv)?
                .decrypt_padded_vec::<NoPadding>(ciphertext)
                .map_err(corrupt)?,
            Self::ChaCha20 => {
                let mut data = ciphertext.to_vec();
                chacha20::ChaCha20::new_from_slices(key, iv)
                    .map_err(bad_iv)?
                    .apply_keystream(&mut data);
                data
            }
        };
        Ok(Zeroizing::new(plaintext))
    }

    pub(crate) fn is_block_cipher(self) -> bool {
        !matches!(self, Self::ChaCha20)
    }
}

fn pkcs7_pad(data: &[u8]) -> Zeroizing<Vec<u8>> {
    let pad = 16 - data.len() % 16;
    let mut padded = Zeroizing::new(Vec::with_capacity(data.len() + pad));
    padded.extend_from_slice(data);
    padded.resize(data.len() + pad, pad as u8);
    padded
}

pub(crate) fn pkcs7_unpad(mut data: Zeroizing<Vec<u8>>) -> Result<Zeroizing<Vec<u8>>, OpenError> {
    let corrupt = || OpenError::Corrupt("the encrypted payload has invalid padding".into());
    let pad = *data.last().ok_or_else(corrupt)? as usize;
    if pad == 0 || pad > 16 || pad > data.len() {
        return Err(corrupt());
    }
    if data[data.len() - pad..]
        .iter()
        .any(|&byte| byte as usize != pad)
    {
        return Err(corrupt());
    }
    let len = data.len() - pad;
    data.truncate(len);
    Ok(data)
}

/// The key that encrypts the payload, from the master seed and the transformed key.
pub(crate) fn cipher_key(master_seed: &[u8], transformed: &Key) -> Key {
    Zeroizing::new(sha256(&[master_seed, &transformed[..]]))
}

/// The key behind KDBX 4's header and block HMACs.
pub(crate) fn hmac_key(master_seed: &[u8], transformed: &Key) -> Zeroizing<[u8; 64]> {
    Zeroizing::new(sha512(&[master_seed, &transformed[..], &[1]]))
}

/// Turns the composite key into the key that, with the master seed, encrypts the payload.
/// This is the deliberately slow step; settings that would make it slower than keefast runs
/// fail with [`OpenError::KdfOverLimit`] before it starts.
pub(crate) fn transform_key(
    kdf: &VariantDictionary,
    composite: &[u8; 32],
) -> Result<Key, OpenError> {
    let missing = |name: &str| OpenError::Corrupt(format!("KDF parameter {name} is missing"));
    let uuid = kdf
        .bytes("$UUID")
        .and_then(|bytes| Uuid::from_slice(bytes).ok())
        .ok_or_else(|| missing("$UUID"))?;
    let salt = kdf.bytes("S").ok_or_else(|| missing("S"))?;

    match uuid {
        AES_KDF | AES_KDF_KDBX4 => {
            let rounds = kdf.u64("R").ok_or_else(|| missing("R"))?;
            Kdf::AesKdf { rounds }
                .check_limits()
                .map_err(OpenError::KdfOverLimit)?;
            aes_kdf(composite, salt, rounds)
        }
        ARGON2D | ARGON2ID => {
            let variant = if uuid == ARGON2D {
                argon2::Variant::Argon2d
            } else {
                argon2::Variant::Argon2id
            };
            let version = match kdf.u32("V").ok_or_else(|| missing("V"))? {
                0x10 => argon2::Version::Version10,
                0x13 => argon2::Version::Version13,
                other => return Err(OpenError::Unsupported(format!("Argon2 version {other:#x}"))),
            };
            let memory = kdf.u64("M").ok_or_else(|| missing("M"))?;
            let iterations = kdf.u64("I").ok_or_else(|| missing("I"))?;
            let lanes = kdf.u32("P").ok_or_else(|| missing("P"))?;
            let cost = Argon2 {
                memory,
                iterations,
                parallelism: lanes,
            };
            cost.check_limits().map_err(OpenError::KdfOverLimit)?;
            let out_of_range =
                |name: &str| OpenError::Unsupported(format!("Argon2 {name} out of range"));
            let config = argon2::Config {
                variant,
                version,
                mem_cost: u32::try_from(memory / 1024).map_err(|_| out_of_range("memory"))?,
                time_cost: u32::try_from(iterations).map_err(|_| out_of_range("iterations"))?,
                lanes,
                secret: kdf.bytes("K").unwrap_or_default(),
                ad: kdf.bytes("A").unwrap_or_default(),
                hash_length: 32,
                ..argon2::Config::default()
            };
            let hash =
                Zeroizing::new(argon2::hash_raw(composite, salt, &config).map_err(|error| {
                    OpenError::Unsupported(format!("Argon2 parameters: {error}"))
                })?);
            let mut key = Zeroizing::new([0; 32]);
            key.copy_from_slice(&hash);
            Ok(key)
        }
        other => Err(OpenError::Unsupported(format!(
            "key derivation function {other}"
        ))),
    }
}

fn aes_kdf(composite: &[u8; 32], seed: &[u8], rounds: u64) -> Result<Key, OpenError> {
    let cipher = Aes256::new_from_slice(seed)
        .map_err(|_| OpenError::Corrupt("the AES-KDF seed must be 32 bytes".into()))?;
    let mut blocks = Zeroizing::new(*composite);
    let (left, right) = blocks.split_at_mut(16);
    let mut left = cipher::Block::<Aes256>::try_from(&*left).expect("16-byte half");
    let mut right = cipher::Block::<Aes256>::try_from(&*right).expect("16-byte half");
    for _ in 0..rounds {
        cipher.encrypt_block(&mut left);
        cipher.encrypt_block(&mut right);
    }
    let key = Zeroizing::new(sha256(&[&left, &right]));
    left.fill(0);
    right.fill(0);
    Ok(key)
}

/// Keys the per-block HMACs of KDBX 4; `u64::MAX` keys the header's HMAC.
fn block_hmac_key(index: u64, hmac_key: &[u8; 64]) -> [u8; 64] {
    sha512(&[&index.to_le_bytes(), hmac_key])
}

pub(crate) fn header_hmac(header: &[u8], hmac_key: &[u8; 64]) -> [u8; 32] {
    hmac_sha256(&block_hmac_key(u64::MAX, hmac_key), &[header])
}

/// Reads KDBX 4's HMAC block stream: authenticated blocks ending with an empty one.
pub(crate) fn read_hmac_blocks(mut data: &[u8], hmac_key: &[u8; 64]) -> Result<Vec<u8>, OpenError> {
    let truncated = || OpenError::Corrupt("the block stream is truncated".into());
    let mut out = Vec::new();
    for index in 0u64.. {
        let hmac = data.get(..32).ok_or_else(truncated)?;
        let size_bytes = data.get(32..36).ok_or_else(truncated)?;
        let size = u32::from_le_bytes(size_bytes.try_into().expect("4 bytes")) as usize;
        let block = data.get(36..36 + size).ok_or_else(truncated)?;
        let expected = hmac_sha256(
            &block_hmac_key(index, hmac_key),
            &[&index.to_le_bytes(), size_bytes, block],
        );
        if !matches(hmac, &expected) {
            return Err(OpenError::Corrupt(format!(
                "block {index} fails its integrity check"
            )));
        }
        if size == 0 {
            break;
        }
        out.extend_from_slice(block);
        data = &data[36 + size..];
    }
    Ok(out)
}

pub(crate) fn write_hmac_blocks(data: &[u8], hmac_key: &[u8; 64]) -> Vec<u8> {
    const BLOCK_SIZE: usize = 1024 * 1024;
    let mut out = Vec::with_capacity(data.len() + 64);
    let blocks = data.chunks(BLOCK_SIZE).chain(std::iter::once(&[][..]));
    for (index, block) in (0u64..).zip(blocks) {
        let size = (block.len() as u32).to_le_bytes();
        let hmac = hmac_sha256(
            &block_hmac_key(index, hmac_key),
            &[&index.to_le_bytes(), &size, block],
        );
        out.extend_from_slice(&hmac);
        out.extend_from_slice(&size);
        out.extend_from_slice(block);
    }
    out
}

/// Reads KDBX 3.1's hashed block stream: SHA-256-checked blocks ending with an empty one.
pub(crate) fn read_hashed_blocks(mut data: &[u8]) -> Result<Vec<u8>, OpenError> {
    let truncated = || OpenError::Corrupt("the block stream is truncated".into());
    let mut out = Vec::new();
    loop {
        let hash = data.get(4..36).ok_or_else(truncated)?;
        let size = data.get(36..40).ok_or_else(truncated)?;
        let size = u32::from_le_bytes(size.try_into().expect("4 bytes")) as usize;
        if size == 0 {
            break;
        }
        let block = data.get(40..40 + size).ok_or_else(truncated)?;
        if !matches(hash, &sha256(&[block])) {
            return Err(OpenError::Corrupt(
                "a block fails its integrity check".into(),
            ));
        }
        out.extend_from_slice(block);
        data = &data[40 + size..];
    }
    Ok(out)
}

/// The inner random stream: a keystream XORed, in document order, over every protected value.
pub(crate) enum InnerStream {
    None,
    Salsa20(Box<salsa20::Salsa20>),
    ChaCha20(Box<chacha20::ChaCha20>),
}

pub(crate) const INNER_STREAM_SALSA20: u32 = 2;
pub(crate) const INNER_STREAM_CHACHA20: u32 = 3;

impl InnerStream {
    pub(crate) fn new(id: u32, key: &[u8]) -> Result<Self, OpenError> {
        match id {
            0 => Ok(Self::None),
            INNER_STREAM_SALSA20 => {
                let key = Zeroizing::new(sha256(&[key]));
                const NONCE: [u8; 8] = [0xE8, 0x30, 0x09, 0x4B, 0x97, 0x20, 0x5D, 0x2A];
                let cipher = salsa20::Salsa20::new_from_slices(&key[..], &NONCE)
                    .expect("SHA-256 output is a Salsa20 key");
                Ok(Self::Salsa20(Box::new(cipher)))
            }
            INNER_STREAM_CHACHA20 => {
                let hash = Zeroizing::new(sha512(&[key]));
                let cipher = chacha20::ChaCha20::new_from_slices(&hash[..32], &hash[32..44])
                    .expect("SHA-512 output splits into a ChaCha20 key and nonce");
                Ok(Self::ChaCha20(Box::new(cipher)))
            }
            other => Err(OpenError::Unsupported(format!(
                "inner random stream {other}"
            ))),
        }
    }

    pub(crate) fn apply(&mut self, data: &mut [u8]) {
        match self {
            Self::None => {}
            Self::Salsa20(cipher) => cipher.apply_keystream(data),
            Self::ChaCha20(cipher) => cipher.apply_keystream(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outer_cipher_decrypts_what_it_encrypts() {
        let key = [7; 32];
        let plaintext = b"seventeen bytes!!";
        for cipher in [
            OuterCipher::Aes256,
            OuterCipher::ChaCha20,
            OuterCipher::Twofish,
        ] {
            let iv = &[9; 16][..cipher.iv_len()];

            let encrypted = cipher.encrypt(&key, iv, plaintext);
            let mut decrypted = cipher.decrypt_raw(&key, iv, &encrypted).unwrap();
            if cipher.is_block_cipher() {
                decrypted = pkcs7_unpad(decrypted).unwrap();
            }

            assert_ne!(&encrypted[..plaintext.len()], plaintext, "{cipher:?}");
            assert_eq!(&decrypted[..], plaintext, "{cipher:?}");
        }
    }
}
