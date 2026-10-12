//! Packages of a made-up third-party Plugin, `acme.iban`, for the package and install tests. Its
//! component is the Card Plugin's: what matters here is that one loads.

#![allow(dead_code)]

use ed25519_dalek::SigningKey;
use keefast_plugins::PublisherKey;
use keefast_plugins::package::pack;
use serde_json::{Value, json};

pub const COMPONENT: &[u8] = include_bytes!("../../../../plugins/card/card.wasm");

/// A minimal PNG's first bytes: enough for an icon to pass as one.
pub const ICON: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

pub fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// The manifest of `acme.iban`, version `version`, signed by `key`, owning `fields` and asking
/// for `capabilities`.
pub fn manifest(key: &SigningKey, version: &str, fields: &[&str], capabilities: &[&str]) -> Value {
    let fields: Vec<Value> = fields
        .iter()
        .enumerate()
        .map(|(index, name)| {
            json!({ "key": format!("f{index}"), "name": name, "label": name, "kind": "text" })
        })
        .collect();
    json!({
        "id": "acme.iban",
        "name": "IBAN",
        "version": version,
        "publisher": "Acme Ltd",
        "publisherKey": PublisherKey::of(key).to_base64(),
        "apiVersion": 1,
        "capabilities": capabilities,
        "contributions": { "facets": [{
            "id": "iban",
            "title": "Bank account",
            "detect": { "fields": [fields[0]["name"]] },
            "fields": fields,
        }]},
    })
}

pub fn package_of(manifest: &Value, key: &SigningKey) -> Vec<u8> {
    pack(&manifest.to_string(), COMPONENT, Some(ICON), key).expect("the package packs")
}

/// `acme.iban` 1.0, owning `IBAN` and `BIC`, asking to copy them.
pub fn iban(key: &SigningKey) -> Vec<u8> {
    package_of(&manifest(key, "1.0", &["IBAN", "BIC"], &["copy"]), key)
}

/// A tar of these entries, as given: names, kinds and all.
pub fn raw_tar(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, kind, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        // Written by hand, so names a well-behaved writer refuses (`../x`) get in.
        let field = &mut header.as_old_mut().name;
        field.fill(0);
        field[..name.len()].copy_from_slice(name.as_bytes());
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(*kind);
        header.set_cksum();
        builder.append(&header, *bytes).unwrap();
    }
    builder.into_inner().unwrap()
}

/// A package's entries, in order.
pub fn unpack(package: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let mut archive = tar::Archive::new(package);
    archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

/// A package of these entries, regular files all.
pub fn repack(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let entries: Vec<_> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), tar::EntryType::Regular, bytes.as_slice()))
        .collect();
    raw_tar(&entries)
}
