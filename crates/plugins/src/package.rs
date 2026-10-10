//! A Plugin package (`.kfplugin`): a manifest, a WASM component and an icon in a tar archive,
//! signed with its publisher's ed25519 key. `PACKAGE.md` beside this crate is the format's spec.
//! A package is read strictly, and its signature checked before anything in it is used.

use std::collections::BTreeMap;
use std::io::Read;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::PluginError;
use crate::manifest::Manifest;

/// The most a package file may weigh.
pub const MAX_PACKAGE: u64 = 10 << 20;

/// The first line of the text a package's signature signs.
const SIGNED_HEADER: &str = "keefast-plugin-package-v1\n";

const MANIFEST: &str = "plugin.json";
const COMPONENT: &str = "plugin.wasm";
const ICON: &str = "icon.png";
const SIGNATURE: &str = "signature";

/// The entries a package may hold, and the most each may weigh.
const ENTRIES: &[(&str, u64)] = &[
    (MANIFEST, 64 << 10),
    (COMPONENT, 8 << 20),
    (ICON, 256 << 10),
    (SIGNATURE, 64),
];

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
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
    #[error("the manifest doesn't name the Plugin's publisher and publisher key")]
    NoPublisher,
    #[error("the manifest's publisher key isn't an ed25519 public key")]
    BadKey,
    #[error(
        "the package's signature doesn't match its contents: it was changed after it was signed, \
         or signed with another key than its manifest names"
    )]
    BadSignature,
    #[error("{0}")]
    Manifest(#[from] PluginError),
}

/// A publisher's ed25519 public key, as a manifest's `publisherKey` gives it (base64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublisherKey(VerifyingKey);

impl PublisherKey {
    pub fn parse(text: &str) -> Result<Self, PackageError> {
        let bytes: [u8; 32] = BASE64
            .decode(text.trim())
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(PackageError::BadKey)?;
        VerifyingKey::from_bytes(&bytes)
            .map(Self)
            .map_err(|_| PackageError::BadKey)
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

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), PackageError> {
        let signature = Signature::from_slice(signature).map_err(|_| PackageError::BadSignature)?;
        self.0
            .verify_strict(message, &signature)
            .map_err(|_| PackageError::BadSignature)
    }
}

/// A package whose signature checked out: everything in it was signed by `key`.
#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: Manifest,
    /// The manifest as the package has it, for the host to load.
    pub manifest_json: String,
    pub component: Vec<u8>,
    pub icon: Option<Vec<u8>>,
    pub key: PublisherKey,
    /// The SHA-256 of the text the signature signs, as hex: names exactly this package.
    pub digest: String,
}

impl Package {
    /// Reads a package, refusing anything `PACKAGE.md` doesn't allow, and checks its signature
    /// against the publisher key its manifest names.
    pub fn read(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() as u64 > MAX_PACKAGE {
            return Err(PackageError::TooBig);
        }
        let mut entries = entries(bytes)?;
        let signature = entries
            .remove(SIGNATURE)
            .ok_or(PackageError::Missing(SIGNATURE))?;
        let manifest_bytes = entries
            .get(MANIFEST)
            .ok_or(PackageError::Missing(MANIFEST))?;
        if !entries.contains_key(COMPONENT) {
            return Err(PackageError::Missing(COMPONENT));
        }
        let manifest_json = String::from_utf8(manifest_bytes.clone())
            .map_err(|_| PackageError::Malformed("plugin.json isn't UTF-8 text".into()))?;
        // The key is read before the manifest is checked, so a package whose manifest a
        // newer keefast would read still fails here only if it was tampered with.
        let key = publisher_key(&manifest_json)?;
        let message = signed_text(&entries);
        key.verify(message.as_bytes(), &signature)?;

        let manifest = Manifest::parse(&manifest_json)?;
        if manifest.publisher.is_none() {
            return Err(PackageError::NoPublisher);
        }
        let icon = entries.remove(ICON);
        if icon
            .as_ref()
            .is_some_and(|icon| !icon.starts_with(PNG_MAGIC))
        {
            return Err(PackageError::IconNotPng);
        }
        Ok(Self {
            manifest,
            manifest_json,
            component: entries.remove(COMPONENT).expect("checked above"),
            icon,
            key,
            digest: hex(&Sha256::digest(message.as_bytes())),
        })
    }
}

/// Makes a package of a manifest, a component and an icon, signed with `key`, which must be
/// the publisher key the manifest names. Plugin authors run it through
/// `cargo run -p keefast-plugins --example pack`.
pub fn pack(
    manifest_json: &str,
    component: &[u8],
    icon: Option<&[u8]>,
    key: &SigningKey,
) -> Result<Vec<u8>, PackageError> {
    let manifest = Manifest::parse(manifest_json)?;
    if manifest.publisher.is_none() {
        return Err(PackageError::NoPublisher);
    }
    if publisher_key(manifest_json)? != PublisherKey::of(key) {
        return Err(PackageError::BadSignature);
    }
    let mut entries = BTreeMap::new();
    entries.insert(MANIFEST.to_owned(), manifest_json.as_bytes().to_vec());
    entries.insert(COMPONENT.to_owned(), component.to_vec());
    if let Some(icon) = icon {
        if !icon.starts_with(PNG_MAGIC) {
            return Err(PackageError::IconNotPng);
        }
        entries.insert(ICON.to_owned(), icon.to_vec());
    }
    let signature = key.sign(signed_text(&entries).as_bytes());
    let mut files: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let signature = signature.to_bytes();
    files.push((SIGNATURE, &signature));
    Ok(tar(&files))
}

/// An uncompressed tar of regular files, its headers the same on every machine.
pub(crate) fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
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

/// The manifest's `publisherKey`, read on its own.
fn publisher_key(manifest_json: &str) -> Result<PublisherKey, PackageError> {
    let value: serde_json::Value = serde_json::from_str(manifest_json)
        .map_err(|error| PluginError::Manifest(error.to_string()))?;
    match value.get("publisherKey").and_then(|key| key.as_str()) {
        Some(key) => PublisherKey::parse(key),
        None => Err(PackageError::NoPublisher),
    }
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
fn entries(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
    let malformed = |error: std::io::Error| PackageError::Malformed(error.to_string());
    let mut archive = tar::Archive::new(bytes);
    let mut found = BTreeMap::new();
    for entry in archive.entries().map_err(malformed)? {
        let entry = entry.map_err(malformed)?;
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        if name.starts_with('/') || name.contains('\\') || name.split('/').any(|part| part == "..")
        {
            return Err(PackageError::UnsafePath(name));
        }
        let Some(&(_, most)) = ENTRIES.iter().find(|(known, _)| *known == name) else {
            return Err(PackageError::Unknown(name));
        };
        if !entry.header().entry_type().is_file() {
            return Err(PackageError::Malformed(format!(
                "\"{name}\" isn't a regular file"
            )));
        }
        if found.contains_key(&name) {
            return Err(PackageError::Duplicate(name));
        }
        if entry.size() > most {
            return Err(PackageError::TooLarge { name, most });
        }
        let mut contents = Vec::new();
        entry
            .take(most + 1)
            .read_to_end(&mut contents)
            .map_err(malformed)?;
        found.insert(name, contents);
    }
    if found.is_empty() {
        return Err(PackageError::Malformed("it holds nothing".into()));
    }
    Ok(found)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_is_four_groups_of_hex() {
        let key = PublisherKey::of(&SigningKey::from_bytes(&[7; 32]));
        let fingerprint = key.fingerprint();
        assert_eq!(fingerprint.len(), 19);
        assert_eq!(fingerprint.split(' ').count(), 4);
        assert_eq!(PublisherKey::parse(&key.to_base64()), Ok(key));
    }

    #[test]
    fn a_key_must_be_32_bytes_of_base64() {
        assert_eq!(PublisherKey::parse("nope"), Err(PackageError::BadKey));
        assert_eq!(
            PublisherKey::parse(&BASE64.encode([1; 31])),
            Err(PackageError::BadKey)
        );
    }
}
