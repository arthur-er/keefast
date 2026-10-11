//! The Plugin package format (`.kfplugin`), version 1: a manifest, a WASM component and an icon
//! in a tar archive, signed with the publisher's ed25519 key. `docs/plugins/package.md` is the
//! format's spec.
//!
//! This crate knows the container and the signature, and of the manifest only its
//! `publisherKey`. keefast's Plugin host (`keefast-plugins`) reads the rest of the manifest
//! strictly; the `keefast-plugin` command (`keefast-plugin-cli`) and keefast's build, which signs
//! the Plugins bundled with keefast, use this crate to make packages.

use std::collections::BTreeMap;
use std::io::Read;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
pub use ed25519_dalek::SigningKey;
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use sha2::{Digest, Sha256};

/// The most a package file may weigh.
pub const MAX_PACKAGE: u64 = 10 << 20;

/// The first line of the text a package's signature signs.
const SIGNED_HEADER: &str = "keefast-plugin-package-v1\n";

pub const MANIFEST: &str = "plugin.json";
pub const COMPONENT: &str = "plugin.wasm";
pub const ICON: &str = "icon.png";
pub const SIGNATURE: &str = "signature";

/// The entries a package may hold, and the most each may weigh.
const ENTRIES: &[(&str, u64)] = &[
    (MANIFEST, 64 << 10),
    (COMPONENT, 8 << 20),
    (ICON, 256 << 10),
    (SIGNATURE, 64),
];

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// What is wrong with a package's container, key or signature.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("the file is larger than a Plugin package may be ({} MiB)", MAX_PACKAGE >> 20)]
    TooBig,
    #[error("the file isn't a Plugin package: {0}")]
    Malformed(String),
    #[error("the package's entry \"{0}\" points outside the package")]
    UnsafePath(String),
    #[error("the package holds \"{0}\", which a Plugin package can't have")]
    Unknown(String),
    #[error("the package holds \"{0}\" twice")]
    Duplicate(String),
    #[error("the package's \"{name}\" is larger than {most} bytes")]
    TooLarge { name: String, most: u64 },
    #[error("the package has no \"{0}\"")]
    Missing(&'static str),
    #[error("the package's icon isn't a PNG image")]
    IconNotPng,
    #[error("the manifest isn't JSON: {0}")]
    ManifestJson(String),
    #[error("the manifest doesn't name the Plugin's publisher key")]
    NoPublisherKey,
    #[error("the manifest's publisher key isn't an ed25519 public key")]
    BadKey,
    #[error("the manifest names another publisher key than the one signing it")]
    OtherKey,
    #[error("the secret key isn't 32 bytes of base64, as `keefast-plugin keygen` writes it")]
    BadSecretKey,
    #[error(
        "the package's signature doesn't match its contents: it was changed after it was signed, \
         or signed with another key than its manifest names"
    )]
    BadSignature,
}

/// A publisher's ed25519 public key, as a manifest's `publisherKey` gives it (base64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublisherKey(VerifyingKey);

impl PublisherKey {
    pub fn parse(text: &str) -> Result<Self, FormatError> {
        let bytes: [u8; 32] = BASE64
            .decode(text.trim())
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(FormatError::BadKey)?;
        VerifyingKey::from_bytes(&bytes)
            .map(Self)
            .map_err(|_| FormatError::BadKey)
    }

    pub fn of(key: &SigningKey) -> Self {
        Self(key.verifying_key())
    }

    pub fn to_base64(&self) -> String {
        BASE64.encode(self.0.as_bytes())
    }

    /// What the UI shows of the key, so a user can tell two apart: the first 8 bytes of its
    /// SHA-256 as hex, in groups of four, as `3F2A 91C0 7B1E 44D2`.
    pub fn fingerprint(&self) -> String {
        let hash = Sha256::digest(self.0.as_bytes());
        hash[..8]
            .chunks(2)
            .map(|pair| format!("{:02X}{:02X}", pair[0], pair[1]))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), FormatError> {
        let signature = Signature::from_slice(signature).map_err(|_| FormatError::BadSignature)?;
        self.0
            .verify_strict(message, &signature)
            .map_err(|_| FormatError::BadSignature)
    }
}

/// A secret key as `keefast-plugin keygen` writes it: its 32-byte seed in standard base64.
pub fn parse_secret_key(text: &str) -> Result<SigningKey, FormatError> {
    let seed: [u8; 32] = BASE64
        .decode(text.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(FormatError::BadSecretKey)?;
    Ok(SigningKey::from_bytes(&seed))
}

/// A secret key as [`parse_secret_key`] reads it.
pub fn secret_key_to_base64(key: &SigningKey) -> String {
    BASE64.encode(key.to_bytes())
}

/// A package whose container is well formed and whose signature checked out: everything in it
/// was signed by `key`. Nothing is said yet of its manifest beyond its `publisherKey`.
#[derive(Debug, Clone)]
pub struct Signed {
    /// The manifest as the package has it.
    pub manifest_json: String,
    pub component: Vec<u8>,
    pub icon: Option<Vec<u8>>,
    pub key: PublisherKey,
    /// The SHA-256 of the text the signature signs, as hex: names exactly this package.
    pub digest: String,
}

/// Reads a package, refusing anything the format doesn't allow, and checks its signature against
/// the publisher key its manifest names.
pub fn read(bytes: &[u8]) -> Result<Signed, FormatError> {
    if bytes.len() as u64 > MAX_PACKAGE {
        return Err(FormatError::TooBig);
    }
    let mut entries = entries(bytes)?;
    let signature = entries
        .remove(SIGNATURE)
        .ok_or(FormatError::Missing(SIGNATURE))?;
    let manifest_bytes = entries
        .get(MANIFEST)
        .ok_or(FormatError::Missing(MANIFEST))?;
    if !entries.contains_key(COMPONENT) {
        return Err(FormatError::Missing(COMPONENT));
    }
    let manifest_json = String::from_utf8(manifest_bytes.clone())
        .map_err(|_| FormatError::Malformed("plugin.json isn't UTF-8 text".into()))?;
    let key = publisher_key(&manifest_json)?;
    let message = signed_text(&entries);
    key.verify(message.as_bytes(), &signature)?;

    let icon = entries.remove(ICON);
    if icon
        .as_ref()
        .is_some_and(|icon| !icon.starts_with(PNG_MAGIC))
    {
        return Err(FormatError::IconNotPng);
    }
    Ok(Signed {
        manifest_json,
        component: entries.remove(COMPONENT).expect("checked above"),
        icon,
        key,
        digest: hex(&Sha256::digest(message.as_bytes())),
    })
}

/// The manifest of a package, its signature unchecked: only to name a package that failed its
/// checks, never to trust anything it says.
pub fn manifest_unchecked(bytes: &[u8]) -> Option<String> {
    if bytes.len() as u64 > MAX_PACKAGE {
        return None;
    }
    String::from_utf8(entries(bytes).ok()?.remove(MANIFEST)?).ok()
}

/// Makes a package of a manifest, a component and an icon, signed with `key`, which must be the
/// publisher key the manifest names (see [`with_publisher_key`]). The manifest isn't checked
/// beyond that: keefast reads it when it installs the package.
pub fn pack(
    manifest_json: &str,
    component: &[u8],
    icon: Option<&[u8]>,
    key: &SigningKey,
) -> Result<Vec<u8>, FormatError> {
    if publisher_key(manifest_json)? != PublisherKey::of(key) {
        return Err(FormatError::OtherKey);
    }
    let mut entries = BTreeMap::new();
    entries.insert(MANIFEST.to_owned(), manifest_json.as_bytes().to_vec());
    entries.insert(COMPONENT.to_owned(), component.to_vec());
    if let Some(icon) = icon {
        if !icon.starts_with(PNG_MAGIC) {
            return Err(FormatError::IconNotPng);
        }
        entries.insert(ICON.to_owned(), icon.to_vec());
    }
    for (name, bytes) in &entries {
        let most = ENTRIES
            .iter()
            .find(|(known, _)| known == name)
            .map_or(0, |(_, most)| *most);
        if bytes.len() as u64 > most {
            return Err(FormatError::TooLarge {
                name: name.clone(),
                most,
            });
        }
    }
    let signature = key.sign(signed_text(&entries).as_bytes()).to_bytes();
    let mut files: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    files.push((SIGNATURE, &signature));
    Ok(tar(&files))
}

/// The manifest's `publisherKey`, read on its own.
pub fn publisher_key(manifest_json: &str) -> Result<PublisherKey, FormatError> {
    let value: serde_json::Value = serde_json::from_str(manifest_json)
        .map_err(|error| FormatError::ManifestJson(error.to_string()))?;
    match value.get("publisherKey").and_then(|key| key.as_str()) {
        Some(key) => PublisherKey::parse(key),
        None => Err(FormatError::NoPublisherKey),
    }
}

/// The manifest naming `key` as its publisher key: unchanged if it already does, with a
/// `publisherKey` added as its first key if it names none, and refused if it names another.
pub fn with_publisher_key(manifest_json: &str, key: &PublisherKey) -> Result<String, FormatError> {
    match publisher_key(manifest_json) {
        Ok(named) if named == *key => Ok(manifest_json.to_owned()),
        Ok(_) => Err(FormatError::OtherKey),
        Err(FormatError::NoPublisherKey) => {
            // Inserted as text, so the rest of the manifest stays as its author wrote it.
            let open = manifest_json.find('{').expect("a JSON object has a brace");
            let (head, rest) = manifest_json.split_at(open + 1);
            let comma = if rest.trim_start().starts_with('}') {
                ""
            } else {
                ","
            };
            Ok(format!(
                "{head}\n  \"publisherKey\": \"{}\"{comma}{rest}",
                key.to_base64()
            ))
        }
        Err(error) => Err(error),
    }
}

/// An uncompressed tar of regular files, its headers the same on every machine.
fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, bytes) in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        builder
            .append_data(&mut header, name, *bytes)
            .expect("writing to memory doesn't fail");
    }
    builder
        .into_inner()
        .expect("writing to memory doesn't fail")
}

/// The text a package's signature signs: a header line, then each entry but the signature, by
/// name, as its name, size and SHA-256.
fn signed_text(entries: &BTreeMap<String, Vec<u8>>) -> String {
    let mut text = SIGNED_HEADER.to_owned();
    for (name, bytes) in entries {
        text.push_str(&format!(
            "{name}\n{}\n{}\n",
            bytes.len(),
            hex(&Sha256::digest(bytes))
        ));
    }
    text
}

/// A package's entries by name, each checked against `ENTRIES`.
fn entries(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, FormatError> {
    let malformed = |error: std::io::Error| FormatError::Malformed(error.to_string());
    let mut archive = tar::Archive::new(bytes);
    let mut found = BTreeMap::new();
    for entry in archive.entries().map_err(malformed)? {
        let entry = entry.map_err(malformed)?;
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        if name.starts_with('/') || name.contains('\\') || name.split('/').any(|part| part == "..")
        {
            return Err(FormatError::UnsafePath(name));
        }
        let Some(&(_, most)) = ENTRIES.iter().find(|(known, _)| *known == name) else {
            return Err(FormatError::Unknown(name));
        };
        if !entry.header().entry_type().is_file() {
            return Err(FormatError::Malformed(format!(
                "\"{name}\" isn't a regular file"
            )));
        }
        if found.contains_key(&name) {
            return Err(FormatError::Duplicate(name));
        }
        if entry.size() > most {
            return Err(FormatError::TooLarge { name, most });
        }
        let mut contents = Vec::new();
        entry
            .take(most + 1)
            .read_to_end(&mut contents)
            .map_err(malformed)?;
        found.insert(name, contents);
    }
    if found.is_empty() {
        return Err(FormatError::Malformed("it holds nothing".into()));
    }
    Ok(found)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn a_fingerprint_is_four_groups_of_hex() {
        let key = PublisherKey::of(&key(7));
        let fingerprint = key.fingerprint();
        assert_eq!(fingerprint.len(), 19);
        assert_eq!(fingerprint.split(' ').count(), 4);
        assert_eq!(PublisherKey::parse(&key.to_base64()), Ok(key));
    }

    #[test]
    fn a_key_must_be_32_bytes_of_base64() {
        assert_eq!(PublisherKey::parse("nope"), Err(FormatError::BadKey));
        assert_eq!(
            PublisherKey::parse(&BASE64.encode([1; 31])),
            Err(FormatError::BadKey)
        );
        assert_eq!(
            parse_secret_key("nope").unwrap_err(),
            FormatError::BadSecretKey
        );
        let secret = key(3);
        assert_eq!(
            parse_secret_key(&secret_key_to_base64(&secret))
                .unwrap()
                .to_bytes(),
            secret.to_bytes()
        );
    }

    #[test]
    fn a_publisher_key_is_added_keeping_the_manifest_as_written() {
        let key = PublisherKey::of(&key(1));
        let manifest = "{\n  \"id\": \"acme.iban\"\n}\n";
        let keyed = with_publisher_key(manifest, &key).unwrap();
        assert_eq!(publisher_key(&keyed), Ok(key));
        assert!(keyed.ends_with("\n  \"id\": \"acme.iban\"\n}\n"));
        assert_eq!(with_publisher_key(&keyed, &key).unwrap(), keyed);
        let other = PublisherKey::of(&self::key(2));
        assert_eq!(
            with_publisher_key(&keyed, &other),
            Err(FormatError::OtherKey)
        );
        assert_eq!(
            publisher_key(&with_publisher_key("{}", &key).unwrap()),
            Ok(key)
        );
    }

    #[test]
    fn a_package_reads_back_as_packed() {
        let secret = key(1);
        let manifest =
            with_publisher_key(r#"{"id": "acme.iban"}"#, &PublisherKey::of(&secret)).unwrap();
        let package = pack(&manifest, b"\0asm", None, &secret).unwrap();
        let signed = read(&package).unwrap();
        assert_eq!(signed.manifest_json, manifest);
        assert_eq!(signed.component, b"\0asm");
        assert_eq!(signed.key, PublisherKey::of(&secret));
        assert_eq!(manifest_unchecked(&package), Some(manifest.clone()));
        assert_eq!(
            pack(&manifest, b"\0asm", None, &key(2)).unwrap_err(),
            FormatError::OtherKey
        );
    }
}
