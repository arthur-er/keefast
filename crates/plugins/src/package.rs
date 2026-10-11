//! A Plugin package (`.kfplugin`): a manifest, a WASM component and an icon in a tar archive,
//! signed with its publisher's ed25519 key. `docs/plugins/package.md` is the format's spec, and
//! `keefast-plugin-package` reads and makes the container; here its manifest is read strictly,
//! after its signature checked out.

use keefast_plugin_package::{self as format, FormatError, SigningKey};

pub use format::{MAX_PACKAGE, PublisherKey};

use crate::PluginError;
use crate::manifest::Manifest;

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
    #[error("the manifest names another publisher key than the one signing it")]
    OtherKey,
    #[error(
        "the package's signature doesn't match its contents: it was changed after it was signed, \
         or signed with another key than its manifest names"
    )]
    BadSignature,
    #[error("{0}")]
    Manifest(#[from] PluginError),
}

impl From<FormatError> for PackageError {
    fn from(error: FormatError) -> Self {
        match error {
            FormatError::TooBig => Self::TooBig,
            FormatError::Malformed(why) => Self::Malformed(why),
            FormatError::UnsafePath(name) => Self::UnsafePath(name),
            FormatError::Unknown(name) => Self::Unknown(name),
            FormatError::Duplicate(name) => Self::Duplicate(name),
            FormatError::TooLarge { name, most } => Self::TooLarge { name, most },
            FormatError::Missing(name) => Self::Missing(name),
            FormatError::IconNotPng => Self::IconNotPng,
            FormatError::ManifestJson(why) => Self::Manifest(PluginError::Manifest(why)),
            FormatError::NoPublisherKey => Self::NoPublisher,
            FormatError::BadKey | FormatError::BadSecretKey => Self::BadKey,
            FormatError::OtherKey => Self::OtherKey,
            FormatError::BadSignature => Self::BadSignature,
        }
    }
}

/// A package whose signature checked out, and whose manifest reads: everything in it was signed
/// by `key`.
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
    /// Reads a package, refusing anything the format doesn't allow, checks its signature against
    /// the publisher key its manifest names, then reads the manifest strictly.
    pub fn read(bytes: &[u8]) -> Result<Self, PackageError> {
        // The key is checked before the manifest is read, so a package whose manifest a newer
        // keefast would read still fails here only if it was tampered with.
        let signed = format::read(bytes)?;
        let manifest = Manifest::parse(&signed.manifest_json)?;
        if manifest.publisher.is_none() {
            return Err(PackageError::NoPublisher);
        }
        Ok(Self {
            manifest,
            manifest_json: signed.manifest_json,
            component: signed.component,
            icon: signed.icon,
            key: signed.key,
            digest: signed.digest,
        })
    }
}

/// Makes a package of a manifest, a component and an icon, signed with `key`, which must be the
/// publisher key the manifest names. Unlike `keefast_plugin_package::pack`, it refuses a manifest
/// keefast wouldn't install.
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
    Ok(format::pack(manifest_json, component, icon, key)?)
}
