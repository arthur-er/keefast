use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::kdbx::Key;
use crate::xml;

/// The secrets that unlock a Vault file: a master password, a key file, or both.
#[derive(Clone)]
pub struct CompositeKey {
    /// The 32-byte key of each component, password first, as KeePass combines them.
    components: Vec<Zeroizing<Vec<u8>>>,
}

/// Why a key file cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum KeyFileError {
    #[error("the key file is damaged: {0}")]
    Corrupt(String),
    #[error("this key file format is not supported: {0}")]
    Unsupported(String),
}

impl CompositeKey {
    pub fn password(password: impl Into<String>) -> Self {
        Self {
            components: vec![password_component(&SecretString::from(password.into()))],
        }
    }

    /// A key file alone, given its contents. Every KeePass key file format is understood: XML
    /// v1.0 and v2.0, 32 raw bytes, 64 hex digits, and any other file, which is hashed.
    pub fn key_file(contents: &[u8]) -> Result<Self, KeyFileError> {
        Ok(Self {
            components: vec![key_file_component(contents)?],
        })
    }

    pub fn password_and_key_file(
        password: impl Into<String>,
        key_file: &[u8],
    ) -> Result<Self, KeyFileError> {
        let mut key = Self::password(password);
        key.components.push(key_file_component(key_file)?);
        Ok(key)
    }

    /// KeePass's composite key: SHA-256 over each component's key.
    pub(crate) fn hash(&self) -> Key {
        let mut digest = Sha256::new();
        for component in &self.components {
            digest.update(&component[..]);
        }
        Zeroizing::new(digest.finalize().into())
    }
}

fn password_component(password: &SecretString) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(Sha256::digest(password.expose_secret().as_bytes()).to_vec())
}

/// The key a key file stands for, the way KeePass reads it: as an XML key file if it is one,
/// else as a raw or hex key by its exact length, else as the hash of the whole file.
fn key_file_component(contents: &[u8]) -> Result<Zeroizing<Vec<u8>>, KeyFileError> {
    if let Some(key) = xml_key(contents)? {
        return Ok(key);
    }
    match contents.len() {
        32 => return Ok(Zeroizing::new(contents.to_vec())),
        64 => {
            if let Some(key) = decode_hex(contents) {
                return Ok(key);
            }
        }
        _ => {}
    }
    Ok(Zeroizing::new(Sha256::digest(contents).to_vec()))
}

/// The key in an XML key file, or `None` when the file isn't an XML key file at all.
fn xml_key(contents: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>, KeyFileError> {
    let Ok(text) = std::str::from_utf8(contents) else {
        return Ok(None);
    };
    let Ok(document) = xml::parse(text) else {
        return Ok(None);
    };
    let version = document
        .child("Meta")
        .and_then(|meta| meta.child("Version"));
    let data = document.child("Key").and_then(|key| key.child("Data"));
    let (true, Some(version), Some(data)) = (document.name == "KeyFile", version, data) else {
        return Ok(None);
    };
    let version = version.text();
    let text = Zeroizing::new(data.text());
    match version.trim().split('.').next() {
        Some("1") => BASE64
            .decode(text.trim())
            .map(Zeroizing::new)
            .map(Some)
            .map_err(|_| KeyFileError::Corrupt("its key is not Base64".into())),
        Some("2") => {
            let digits: Zeroizing<Vec<u8>> = Zeroizing::new(
                text.bytes()
                    .filter(|byte| !byte.is_ascii_whitespace())
                    .collect(),
            );
            let key = decode_hex(&digits)
                .ok_or_else(|| KeyFileError::Corrupt("its key is not hexadecimal".into()))?;
            // v2.0 carries the first 4 bytes of the key's SHA-256, to catch typos and damage.
            if let Some(hash) = data.attribute("Hash") {
                let expected = &Sha256::digest(&key[..])[..4];
                if decode_hex(hash.trim().as_bytes())
                    .as_deref()
                    .map(|h| &h[..])
                    != Some(expected)
                {
                    return Err(KeyFileError::Corrupt(
                        "its key does not match its hash".into(),
                    ));
                }
            }
            Ok(Some(key))
        }
        _ => Err(KeyFileError::Unsupported(format!(
            "key file version {}",
            version.trim()
        ))),
    }
}

fn decode_hex(digits: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    let digit = |byte: u8| char::from(byte).to_digit(16);
    let mut bytes = Zeroizing::new(Vec::with_capacity(digits.len() / 2));
    for pair in digits.chunks(2) {
        bytes.push((digit(pair[0])? * 16 + digit(pair[1])?) as u8);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_four_bytes_that_are_not_hex_are_hashed() {
        let contents = [b'z'; 64];

        let key = key_file_component(&contents).unwrap();

        assert_eq!(&key[..], &Sha256::digest(contents)[..]);
    }

    #[test]
    fn xml_that_is_not_a_key_file_is_hashed() {
        let contents = b"<?xml version=\"1.0\"?><Notes><Key><Data>AAAA</Data></Key></Notes>";

        let key = key_file_component(contents).unwrap();

        assert_eq!(&key[..], &Sha256::digest(contents)[..]);
    }

    #[test]
    fn an_empty_key_file_is_hashed() {
        let key = key_file_component(b"").unwrap();

        assert_eq!(&key[..], &Sha256::digest(b"")[..]);
    }

    #[test]
    fn hex_decodes_either_case() {
        assert_eq!(
            decode_hex(b"0aFf").as_deref().map(|b| &b[..]),
            Some(&[10, 255][..])
        );
        assert!(decode_hex(b"0g").is_none());
        assert!(decode_hex(b"abc").is_none());
    }
}
