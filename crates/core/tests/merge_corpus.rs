//! Merging Vaults from the real-client corpus into each other, in both directions. Unrelated
//! Vaults share no UUIDs, so this is what a Vault file that was replaced by another one (a
//! different Vault saved over the same path, say) looks like to a Merge. A Merge runs by itself
//! on every save and on every external change, so it must never panic, whatever it is given.

use keefast_core::{CompositeKey, Vault};

const PASSWORD: &str = "fixture-password";

const CORPUS: &[(&str, &[u8])] = &[
    (
        "keepass2-kdbx3",
        include_bytes!("../../../fixtures/keepass2-kdbx3.kdbx"),
    ),
    (
        "keepass2-kdbx4",
        include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx"),
    ),
    (
        "keepassxc-browser",
        include_bytes!("../../../fixtures/keepassxc-browser.kdbx"),
    ),
    (
        "keepassxc-kdbx3",
        include_bytes!("../../../fixtures/keepassxc-kdbx3.kdbx"),
    ),
    (
        "keepassxc-kdbx4",
        include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx"),
    ),
    (
        "keeweb-kdbx4",
        include_bytes!("../../../fixtures/keeweb-kdbx4.kdbx"),
    ),
    (
        "pykeepass-unknown-xml-kdbx3",
        include_bytes!("../../../fixtures/pykeepass-unknown-xml-kdbx3.kdbx"),
    ),
    (
        "pykeepass-unknown-xml-kdbx4",
        include_bytes!("../../../fixtures/pykeepass-unknown-xml-kdbx4.kdbx"),
    ),
];

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

/// Pairs that are also saved and reopened: key derivation makes that slow, so not every pair.
const SAVED: &[(&str, &str)] = &[
    ("keepass2-kdbx4", "keepassxc-kdbx4"),
    ("keepassxc-kdbx4", "keepass2-kdbx4"),
    ("keepassxc-browser", "keepass2-kdbx3"),
];

#[test]
fn every_pair_of_corpus_vaults_merges_in_both_directions() {
    let vaults: Vec<(&str, Vault)> = CORPUS
        .iter()
        .map(|(name, bytes)| (*name, open(bytes)))
        .collect();
    let entries = |vault: &Vault| vault.root().all_entries().count();

    for (into_name, into) in &vaults {
        for (from_name, from) in &vaults {
            let mut vault = into.clone();

            let summary = vault.merge(from);

            // Whatever the other Vault holds is in this one now.
            assert!(
                entries(&vault) >= entries(from),
                "merging {from_name} into {into_name} lost Entries (merged: {summary:?})"
            );
            // The Vault is still the one it was.
            assert_eq!(
                vault.root().id(),
                into.root().id(),
                "merging {from_name} into {into_name} changed the root Group"
            );
            // Merging the same version again changes nothing.
            assert!(
                !vault.merge(from).changed(),
                "merging {from_name} into {into_name} twice changed the Vault the second time"
            );

            if SAVED.contains(&(from_name, into_name)) {
                let saved = vault.save().unwrap_or_else(|error| {
                    panic!("merging {from_name} into {into_name} left a Vault that doesn't save: {error}")
                });
                assert_eq!(entries(&open(&saved)), entries(&vault));
            }
        }
    }
}
