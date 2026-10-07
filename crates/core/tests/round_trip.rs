//! The round-trip corpus (ADR 0002): real Vault files written by KeePassXC, KeePass2, KeeWeb and
//! pykeepass (see scripts/make-fixtures.sh) must survive open → save → reopen with nothing lost.
//!
//! The `keepass` crate serves as an independent KDBX reader, so these tests don't only check that
//! keefast agrees with itself.

use keefast_core::{CompositeKey, OpenError, Vault};
use keepass::config::{DatabaseVersion, KdfConfig, OuterCipherConfig};
use keepass::{Database, DatabaseKey};

const PASSWORD: &str = "fixture-password";

const CORPUS: &[(&str, &[u8])] = &[
    (
        "keepassxc-kdbx3",
        include_bytes!("../../../fixtures/keepassxc-kdbx3.kdbx"),
    ),
    (
        "keepassxc-kdbx4",
        include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx"),
    ),
    (
        "keepass2-kdbx3",
        include_bytes!("../../../fixtures/keepass2-kdbx3.kdbx"),
    ),
    (
        "keepass2-kdbx4",
        include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx"),
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

/// The corpus files the `keepass` crate reads correctly. It rejects unknown XML outright, and of
/// a KDBX 3.1 file's attachments it keeps only the last (see ADR 0002).
const INDEPENDENTLY_READABLE: &[&str] = &[
    "keepassxc-kdbx3",
    "keepassxc-kdbx4",
    "keepass2-kdbx4",
    "keeweb-kdbx4",
];

/// The corpus files the `keepass` crate can parse at all, if not always faithfully.
fn independently_parseable() -> impl Iterator<Item = &'static (&'static str, &'static [u8])> {
    CORPUS
        .iter()
        .filter(|(name, _)| !name.starts_with("pykeepass-unknown-xml"))
}

fn fixture(name: &str) -> &'static [u8] {
    CORPUS
        .iter()
        .find(|(fixture, _)| *fixture == name)
        .map(|(_, bytes)| *bytes)
        .expect("fixture is in the corpus")
}

fn key() -> CompositeKey {
    CompositeKey::password(PASSWORD)
}

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &key()).expect("Vault file opens")
}

fn saved(name: &str) -> Vec<u8> {
    open(fixture(name)).save().expect("Vault saves")
}

/// Parses with the `keepass` crate.
fn independent(bytes: &[u8]) -> Database {
    Database::parse(bytes, DatabaseKey::new().with_password(PASSWORD)).expect("keepass parses")
}

/// The decrypted XML, as the `keepass` crate sees it.
fn independent_xml(bytes: &[u8]) -> String {
    let xml = Database::get_xml(&mut &bytes[..], DatabaseKey::new().with_password(PASSWORD))
        .expect("keepass decrypts");
    String::from_utf8(xml).expect("UTF-8 XML")
}

#[test]
fn the_corpus_covers_every_format_variant() {
    let configs: Vec<_> = independently_parseable()
        .map(|(_, bytes)| independent(bytes).config)
        .collect();
    let has = |check: &dyn Fn(&keepass::config::DatabaseConfig) -> bool| configs.iter().any(check);

    assert!(has(&|c| matches!(c.version, DatabaseVersion::KDB3(1))));
    assert!(has(&|c| matches!(c.version, DatabaseVersion::KDB4(0))));
    assert!(has(&|c| matches!(c.version, DatabaseVersion::KDB4(1))));
    assert!(has(&|c| c.outer_cipher_config == OuterCipherConfig::AES256));
    assert!(has(
        &|c| c.outer_cipher_config == OuterCipherConfig::ChaCha20
    ));
    assert!(has(&|c| matches!(c.kdf_config, KdfConfig::Aes { .. })));
    assert!(has(&|c| matches!(c.kdf_config, KdfConfig::Argon2 { .. })));
    assert!(has(&|c| matches!(c.kdf_config, KdfConfig::Argon2id { .. })));
}

#[test]
fn every_corpus_file_reopens_identical_after_a_save() {
    for (name, bytes) in CORPUS {
        let original = open(bytes);

        let reopened = open(&original.save().expect("Vault saves"));

        assert!(reopened == original, "{name} changed in a save");
        assert_eq!(format!("{reopened:?}"), format!("{original:?}"), "{name}");
    }
}

#[test]
fn an_independent_reader_sees_the_same_content_after_a_save() {
    for name in INDEPENDENTLY_READABLE {
        let original = independent(fixture(name));

        let mut resaved = independent(&saved(name));

        assert!(
            matches!(resaved.config.version, DatabaseVersion::KDB4(_)),
            "{name}"
        );
        // Seeds, the inner stream and the format version change in a save; content must not.
        resaved.config = original.config.clone();
        assert!(resaved == original, "{name} reads differently after a save");
    }
}

#[test]
fn a_save_keeps_the_cipher_compression_and_key_derivation_settings() {
    for (name, bytes) in independently_parseable() {
        let original = independent(bytes).config;

        let resaved = independent(&saved(name)).config;

        assert_eq!(
            resaved.outer_cipher_config, original.outer_cipher_config,
            "{name}"
        );
        assert_eq!(
            resaved.compression_config, original.compression_config,
            "{name}"
        );
        assert_eq!(resaved.kdf_config, original.kdf_config, "{name}");
        assert_eq!(
            resaved.public_custom_data, original.public_custom_data,
            "{name}"
        );
    }
}

#[test]
fn unknown_xml_elements_and_attributes_survive_a_save_verbatim() {
    for name in ["pykeepass-unknown-xml-kdbx3", "pykeepass-unknown-xml-kdbx4"] {
        let xml = independent_xml(&saved(name));

        for expected in [
            r#"<KeefastFutureMeta Origin="pykeepass">"#,
            r#"<Nested Flag="1">meta-payload</Nested>"#,
            "<Empty/>",
            r#"FutureGroupAttribute="group-attribute""#,
            "<FutureGroupSetting>group-payload</FutureGroupSetting>",
            r#"FutureEntryAttribute="entry-attribute""#,
            r#"<FutureEntryField Kind="opaque">entry-payload</FutureEntryField>"#,
            r#"FutureValueAttribute="value-attribute""#,
            "<FutureRootElement>root-payload</FutureRootElement>",
        ] {
            assert!(xml.contains(expected), "{name} lost {expected}");
        }
    }
    let xml = independent_xml(&saved("pykeepass-unknown-xml-kdbx4"));
    assert!(xml.contains("<FutureHistoryField>history-payload</FutureHistoryField>"));
}

#[test]
fn custom_data_survives_a_save_verbatim() {
    let expectations = [
        ("keepassxc-kdbx4", "KPXC_BROWSER_fixture-browser"),
        ("keepassxc-kdbx4", "not-a-real-key"),
        ("keepassxc-kdbx4", "<Key>keefast-fixture</Key>"),
        ("keepass2-kdbx3", "<Key>KeePassFixturePlugin.Settings</Key>"),
        (
            "keepass2-kdbx4",
            "<Key>KeePassFixturePlugin.EntryState</Key>",
        ),
        (
            "keepass2-kdbx4",
            "<Key>KeePassFixturePlugin.GroupState</Key>",
        ),
        ("keeweb-kdbx4", "<Key>KeeWebFixture.Settings</Key>"),
        ("keeweb-kdbx4", "<Value>theme=dark</Value>"),
        ("keeweb-kdbx4", "<Key>KeeWebFixture.EntryState</Key>"),
    ];
    for (name, expected) in expectations {
        assert!(
            independent_xml(fixture(name)).contains(expected),
            "{name} lacks {expected}"
        );

        assert!(
            independent_xml(&saved(name)).contains(expected),
            "{name} lost {expected}"
        );
    }
}

#[test]
fn attachments_history_and_custom_icons_survive_a_save() {
    for name in [
        "keepassxc-kdbx3",
        "keepass2-kdbx3",
        "keepass2-kdbx4",
        "keeweb-kdbx4",
    ] {
        let resaved = independent(&saved(name));

        let attachments = resaved.iter_all_attachments().count();
        let icons = resaved.iter_all_custom_icons().count();
        let with_history = resaved
            .iter_all_entries()
            .filter(|entry| {
                entry
                    .history
                    .as_ref()
                    .is_some_and(|h| !h.get_entries().is_empty())
            })
            .count();
        assert!(attachments > 0, "{name} has no attachments");
        assert!(
            icons > 0 || name == "keepassxc-kdbx3",
            "{name} has no custom icons"
        );
        assert!(with_history > 0, "{name} has no history");
    }
}

#[test]
fn kdbx3_attachments_move_into_the_kdbx4_attachment_pool() {
    let resaved = independent(&saved("keepass2-kdbx3"));

    let email = resaved
        .iter_all_entries()
        .find(|entry| entry.get_title() == Some("Email"))
        .expect("Email entry");
    let attachment = |name| {
        email
            .attachment_by_name(name)
            .expect("attachment")
            .data
            .clone()
    };
    let codes = attachment("recovery-codes.txt");
    assert_eq!(codes.get(), b"recovery-code-1\nrecovery-code-2\n");
    assert!(!codes.is_protected());
    let protected = attachment("protected.bin");
    assert_eq!(protected.get(), &[0, 1, 2, 3, 254, 255]);
    assert!(protected.is_protected());
    // Shared by two Entries, stored once.
    assert_eq!(resaved.iter_all_attachments().count(), 2);
}

#[test]
fn a_saved_vault_file_opens_only_with_its_master_password() {
    let bytes = saved("keepass2-kdbx4");

    let result = Vault::open(&bytes, &CompositeKey::password("not it"));

    assert!(matches!(result, Err(OpenError::WrongKey)));
}

#[test]
fn every_save_encrypts_with_fresh_seeds() {
    let vault = open(fixture("keeweb-kdbx4"));

    let first = vault.save().unwrap();
    let second = vault.save().unwrap();

    assert_ne!(first, second);
    assert!(open(&first) == open(&second));
}

#[test]
fn a_damaged_save_is_reported_as_corrupt() {
    let mut bytes = saved("keeweb-kdbx4");
    let last = bytes.len() - 40;
    bytes[last] ^= 1;

    let result = Vault::open(&bytes, &key());

    assert!(matches!(result, Err(OpenError::Corrupt(_))));
}
