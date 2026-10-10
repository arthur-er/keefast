//! Reading `.kfplugin` packages: their signature, and everything a package mustn't be.

mod common;

use common::*;
use ed25519_dalek::Signer;
use keefast_plugins::package::{MAX_PACKAGE, pack};
use keefast_plugins::{Package, PackageError, PluginError, PublisherKey};
use tar::EntryType;

#[test]
fn a_signed_package_reads_with_its_key() {
    let key = key(1);
    let package = Package::read(&iban(&key)).unwrap();
    assert_eq!(package.manifest.id, "acme.iban");
    assert_eq!(package.manifest.publisher.as_deref(), Some("Acme Ltd"));
    assert_eq!(package.key, PublisherKey::of(&key));
    assert_eq!(package.component, COMPONENT);
    assert_eq!(package.icon.as_deref(), Some(ICON));
    assert_eq!(package.digest.len(), 64);
}

#[test]
fn the_order_of_entries_doesnt_change_the_digest() {
    let package = iban(&key(1));
    let mut entries = unpack(&package);
    entries.reverse();
    let reordered = Package::read(&repack(&entries)).unwrap();
    assert_eq!(reordered.digest, Package::read(&package).unwrap().digest);
}

#[test]
fn a_package_changed_after_signing_is_refused() {
    let mut entries = unpack(&iban(&key(1)));
    let component = entries
        .iter_mut()
        .find(|(name, _)| name == "plugin.wasm")
        .unwrap();
    let last = component.1.len() - 1;
    component.1[last] ^= 1;
    assert_eq!(
        Package::read(&repack(&entries)).unwrap_err(),
        PackageError::BadSignature
    );
}

#[test]
fn a_manifest_changed_after_signing_is_refused() {
    let key = key(1);
    let mut entries = unpack(&iban(&key));
    let more = manifest(&key, "1.0", &["IBAN", "BIC"], &["copy", "readProtected"]);
    entries
        .iter_mut()
        .find(|(name, _)| name == "plugin.json")
        .unwrap()
        .1 = more.to_string().into_bytes();
    assert_eq!(
        Package::read(&repack(&entries)).unwrap_err(),
        PackageError::BadSignature
    );
}

#[test]
fn a_package_signed_by_another_key_than_its_manifest_names_is_refused() {
    let named = key(1);
    let signer = key(2);
    // pack refuses to make one, so sign it by hand.
    assert!(
        pack(
            &manifest(&named, "1", &["IBAN"], &[]).to_string(),
            COMPONENT,
            None,
            &signer
        )
        .is_err()
    );
    let mut entries = unpack(&iban(&named));
    let signed: Vec<(String, Vec<u8>)> = entries
        .iter()
        .filter(|(name, _)| name != "signature")
        .cloned()
        .collect();
    let text = signed_text(&signed);
    entries
        .iter_mut()
        .find(|(name, _)| name == "signature")
        .unwrap()
        .1 = signer.sign(text.as_bytes()).to_bytes().to_vec();
    assert_eq!(
        Package::read(&repack(&entries)).unwrap_err(),
        PackageError::BadSignature
    );

    // Swapping in the signer's key changes the manifest, so the signature fails just the same.
    let mut entries = unpack(&iban(&named));
    entries
        .iter_mut()
        .find(|(name, _)| name == "plugin.json")
        .unwrap()
        .1 = manifest(&signer, "1.0", &["IBAN", "BIC"], &["copy"])
        .to_string()
        .into_bytes();
    assert_eq!(
        Package::read(&repack(&entries)).unwrap_err(),
        PackageError::BadSignature
    );
}

/// The text a package's signature signs, as `PACKAGE.md` gives it.
fn signed_text(entries: &[(String, Vec<u8>)]) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted = entries.to_vec();
    sorted.sort();
    let mut text = "keefast-plugin-package-v1\n".to_owned();
    for (name, bytes) in sorted {
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        text.push_str(&format!("{name}\n{}\n{hash}\n", bytes.len()));
    }
    text
}

#[test]
fn the_spec_s_signed_text_is_what_is_signed() {
    let key = key(3);
    let package = iban(&key);
    let entries = unpack(&package);
    let signed: Vec<_> = entries
        .iter()
        .filter(|(name, _)| name != "signature")
        .cloned()
        .collect();
    let signature = &entries
        .iter()
        .find(|(name, _)| name == "signature")
        .unwrap()
        .1;
    let signature = ed25519_dalek::Signature::from_slice(signature).unwrap();
    key.verifying_key()
        .verify_strict(signed_text(&signed).as_bytes(), &signature)
        .unwrap();
}

#[test]
fn a_package_without_a_publisher_key_or_signature_is_refused() {
    let key = key(1);
    let mut unsigned = manifest(&key, "1.0", &["IBAN"], &[]);
    unsigned.as_object_mut().unwrap().remove("publisherKey");
    let entries = vec![
        ("plugin.json".to_owned(), unsigned.to_string().into_bytes()),
        ("plugin.wasm".to_owned(), COMPONENT.to_vec()),
        ("signature".to_owned(), vec![0; 64]),
    ];
    assert_eq!(
        Package::read(&repack(&entries)).unwrap_err(),
        PackageError::NoPublisher
    );

    let entries: Vec<_> = unpack(&iban(&key))
        .into_iter()
        .filter(|(name, _)| name != "signature")
        .collect();
    assert_eq!(
        Package::read(&repack(&entries)).unwrap_err(),
        PackageError::Missing("signature")
    );
}

#[test]
fn garbage_isnt_a_package() {
    assert!(matches!(
        Package::read(b"not a package at all, just text").unwrap_err(),
        PackageError::Malformed(_)
    ));
    assert!(matches!(
        Package::read(&[]).unwrap_err(),
        PackageError::Malformed(_)
    ));
    let huge = vec![0; MAX_PACKAGE as usize + 1];
    assert_eq!(Package::read(&huge).unwrap_err(), PackageError::TooBig);
}

#[test]
fn entries_pointing_outside_the_package_are_refused() {
    for name in [
        "../plugin.json",
        "/etc/plugin.json",
        "icons/../../x",
        "a\\..\\b",
    ] {
        let package = raw_tar(&[(name, EntryType::Regular, b"{}")]);
        assert_eq!(
            Package::read(&package).unwrap_err(),
            PackageError::UnsafePath(name.to_owned()),
            "{name}"
        );
    }
}

#[test]
fn unknown_entries_links_and_directories_are_refused() {
    let unknown = raw_tar(&[("run.sh", EntryType::Regular, b"#!/bin/sh")]);
    assert_eq!(
        Package::read(&unknown).unwrap_err(),
        PackageError::Unknown("run.sh".into())
    );
    let link = raw_tar(&[("plugin.json", EntryType::Symlink, b"")]);
    assert!(matches!(
        Package::read(&link).unwrap_err(),
        PackageError::Malformed(_)
    ));
    let dir = raw_tar(&[("icons/", EntryType::Directory, b"")]);
    assert_eq!(
        Package::read(&dir).unwrap_err(),
        PackageError::Unknown("icons/".into())
    );
}

#[test]
fn duplicate_entries_are_refused() {
    let mut entries = unpack(&iban(&key(1)));
    entries.push(entries[0].clone());
    assert!(matches!(
        Package::read(&repack(&entries)).unwrap_err(),
        PackageError::Duplicate(_)
    ));
}

#[test]
fn oversize_entries_are_refused() {
    let big = vec![b' '; (64 << 10) + 1];
    let package = raw_tar(&[("plugin.json", EntryType::Regular, &big)]);
    assert_eq!(
        Package::read(&package).unwrap_err(),
        PackageError::TooLarge {
            name: "plugin.json".into(),
            most: 64 << 10
        }
    );
}

#[test]
fn an_icon_must_be_a_png() {
    let key = key(1);
    let result = pack(
        &manifest(&key, "1.0", &["IBAN"], &[]).to_string(),
        COMPONENT,
        Some(b"<svg onload=alert(1)>"),
        &key,
    );
    assert_eq!(result.unwrap_err(), PackageError::IconNotPng);
}

#[test]
fn a_package_s_manifest_is_read_strictly() {
    let key = key(1);
    let mut asks = manifest(&key, "1.0", &["IBAN"], &[]);
    asks["capabilities"] = serde_json::json!(["network"]);
    assert!(matches!(
        pack(&asks.to_string(), COMPONENT, None, &key).unwrap_err(),
        PackageError::Manifest(PluginError::Manifest(_))
    ));
}
