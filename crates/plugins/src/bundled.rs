//! The Plugins bundled with keefast, and the keys keefast trusts as its own.
//!
//! The bundled Plugins are compiled in as signed packages, made by `build.rs`, and every load
//! reads them as any installed package is read: the container strictly, the signature against
//! the key the build signed with, then the manifest (ADR 0009: keefast has no private shortcut).
//!
//! Two kinds of key are keefast's:
//! - the **release keys** (`keys/release.pub`), whose secret half only keefast's release CI has.
//!   A package they sign is keefast's wherever it comes from, so it may have a `keefast.` id and
//!   be installed from a file;
//! - the **bundled key**, which signed the packages compiled into this build: the release key in
//!   an official release, or a key a development or source build made for itself and threw away.
//!   It is trusted for those compiled-in packages only, never for a package installed from a file.

use crate::manifest::Manifest;
use crate::package::{Package, PublisherKey};

include!(concat!(env!("OUT_DIR"), "/bundled.rs"));

/// Plugin ids only keefast's own Plugins may have.
pub(crate) const KEEFAST_PREFIX: &str = "keefast.";

/// A bundled Plugin, and whether its package passed its checks.
pub(crate) struct Bundled {
    pub manifest: Manifest,
    /// Why it can't run, if its package failed its checks.
    pub package: Result<Package, String>,
}

/// The bundled Plugins, their packages read and checked.
pub(crate) fn bundled_plugins() -> Vec<Bundled> {
    BUNDLED_PACKAGES
        .iter()
        .map(|bytes| match check(bytes) {
            Ok(package) => Bundled {
                manifest: package.manifest.clone(),
                package: Ok(package),
            },
            Err(problem) => Bundled {
                // Only to name it in Settings: nothing else of it is used.
                manifest: keefast_plugin_package::manifest_unchecked(bytes)
                    .and_then(|json| Manifest::parse(&json).ok())
                    .expect("a bundled Plugin's manifest reads"),
                package: Err(problem),
            },
        })
        .collect()
}

fn check(bytes: &[u8]) -> Result<Package, String> {
    let package = Package::read(bytes).map_err(|error| error.to_string())?;
    if package.key != bundled_key() {
        return Err("its package isn't signed by the key keefast was built with".into());
    }
    if !package.manifest.id.starts_with(KEEFAST_PREFIX) {
        return Err(format!(
            "{} isn't one of keefast's ids",
            package.manifest.id
        ));
    }
    Ok(package)
}

/// The key the bundled Plugins were signed with when this keefast was built.
pub fn bundled_key() -> PublisherKey {
    PublisherKey::parse(BUNDLED_KEY).expect("build.rs writes a valid key")
}

/// Whether the bundled Plugins were signed with keefast's release key, as in an official
/// release, rather than with a key this build made for itself.
pub fn bundled_by_release_key() -> bool {
    BUNDLED_BY_RELEASE_KEY
}

/// keefast's release keys, the one releases sign with first.
pub(crate) fn release_keys() -> Vec<PublisherKey> {
    RELEASE_KEYS
        .iter()
        .map(|key| PublisherKey::parse(key).expect("build.rs writes valid keys"))
        .collect()
}

/// Whether a key is one of keefast's release keys: what it signs is keefast's.
pub fn is_release_key(key: &PublisherKey) -> bool {
    release_keys().contains(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_plugins_pass_their_checks() {
        let bundled = bundled_plugins();
        assert!(!bundled.is_empty());
        for plugin in bundled {
            let package = plugin
                .package
                .expect("a bundled Plugin's package checks out");
            assert_eq!(package.key, bundled_key());
            assert_eq!(package.manifest.publisher.as_deref(), Some("keefast"));
        }
        assert_eq!(bundled_by_release_key(), is_release_key(&bundled_key()));
    }

    #[test]
    fn a_bundled_package_changed_after_signing_is_refused() {
        let mut bytes = BUNDLED_PACKAGES[0].to_vec();
        // The component is the largest entry: flip a byte in the middle of the archive.
        let middle = bytes.len() / 2;
        bytes[middle] ^= 1;
        assert!(check(&bytes).is_err());
    }
}
