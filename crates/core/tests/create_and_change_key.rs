//! Creating a new Vault, and changing a Vault's master key, cipher and key derivation: what is
//! written must open with the new key only, and read the same in an independent KDBX reader.

use keefast_core::{
    Argon2, Cipher, CompositeKey, Encryption, EncryptionError, EntryFields, Kdf, KeyParts,
    OpenError, Timestamp, Vault, new_key_file,
};
use keepass::config::{DatabaseConfig, InnerCipherConfig, KdfConfig, OuterCipherConfig};
use keepass::{Database, DatabaseKey};
use secrecy::SecretString;

const KEEPASSXC_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx3.kdbx");
const FIXTURE_PASSWORD: &str = "fixture-password";
const PASSWORD: &str = "correct horse battery staple";

/// Argon2id light enough for tests.
fn quick() -> Encryption {
    Encryption {
        cipher: Cipher::ChaCha20,
        kdf: Kdf::Argon2id(Argon2 {
            memory: 1024 * 1024,
            iterations: 2,
            parallelism: 2,
        }),
    }
}

fn now() -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_000)
}

fn create(name: &str, key: &CompositeKey, encryption: &Encryption) -> Vault {
    Vault::create(name, key, encryption, now()).expect("the settings are valid")
}

fn fields(title: &str) -> EntryFields {
    EntryFields {
        title: title.into(),
        password: SecretString::from("hunter2"),
        ..EntryFields::default()
    }
}

/// How the independent reader sees the Vault file's encryption.
fn independent_config(bytes: &[u8], key: DatabaseKey) -> DatabaseConfig {
    Database::open(&mut &bytes[..], key)
        .expect("keepass opens what keefast wrote")
        .config
}

#[test]
fn a_new_vault_opens_with_its_master_password_and_holds_only_its_top_group() {
    let key = CompositeKey::password(PASSWORD);
    let vault = create("Personal", &key, &quick());

    let bytes = vault.save().unwrap();
    let reopened = Vault::open(&bytes, &key).unwrap();

    assert_eq!(reopened.root().name(), "Personal");
    assert!(reopened.root().groups().is_empty());
    assert_eq!(reopened.root().all_entries().count(), 0);
    assert!(matches!(
        Vault::open(&bytes, &CompositeKey::password("wrong")),
        Err(OpenError::WrongKey)
    ));
}

#[test]
fn a_new_vault_is_kdbx_4_with_argon2id_and_the_chosen_cipher_for_other_clients() {
    let key = CompositeKey::password(PASSWORD);
    for (cipher, expected) in [
        (Cipher::ChaCha20, OuterCipherConfig::ChaCha20),
        (Cipher::Aes256, OuterCipherConfig::AES256),
    ] {
        let encryption = Encryption { cipher, ..quick() };
        let bytes = create("Personal", &key, &encryption).save().unwrap();

        let config = independent_config(&bytes, DatabaseKey::new().with_password(PASSWORD));

        assert_eq!(config.outer_cipher_config, expected);
        assert_eq!(config.inner_cipher_config, InnerCipherConfig::ChaCha20);
        assert!(
            matches!(
                config.kdf_config,
                KdfConfig::Argon2id {
                    iterations: 2,
                    memory: 1_048_576,
                    parallelism: 2,
                    ..
                }
            ),
            "{:?}",
            config.kdf_config
        );
        assert!(
            matches!(config.version, keepass::config::DatabaseVersion::KDB4(_)),
            "{:?}",
            config.version
        );
    }
}

#[test]
fn the_default_settings_are_aes_256_and_a_strong_argon2id() {
    let defaults = Encryption::default();

    assert_eq!(defaults.cipher, Cipher::Aes256);
    let Kdf::Argon2id(argon2) = defaults.kdf else {
        panic!("Argon2id by default, not {:?}", defaults.kdf);
    };
    assert!(argon2.memory >= 64 * 1024 * 1024);
    assert!(argon2.iterations >= 2);
    assert!(defaults.check().is_ok());
}

#[test]
fn a_new_vault_reports_the_settings_it_was_created_with() {
    let vault = create("Personal", &CompositeKey::password(PASSWORD), &quick());

    assert_eq!(vault.encryption(), quick());
}

#[test]
fn a_new_vault_has_a_recycle_bin_that_appears_on_the_first_delete() {
    let mut vault = create("Personal", &CompositeKey::password(PASSWORD), &quick());
    let root = vault.root().id();
    let entry = vault.create_entry(root, &fields("Mail"), now()).unwrap();

    vault.delete_entry(entry, now()).unwrap();

    let bin = vault.recycle_bin().expect("the Recycle Bin is enabled");
    assert_eq!(vault.group(bin).unwrap().entries()[0].title(), "Mail");
}

#[test]
fn a_new_vault_can_be_keyed_with_a_key_file_alone_or_with_a_password() {
    let key_file = new_key_file();
    for key in [
        CompositeKey::key_file(&key_file).unwrap(),
        CompositeKey::password_and_key_file(PASSWORD, &key_file).unwrap(),
    ] {
        let bytes = create("Keyed", &key, &quick()).save().unwrap();

        assert!(Vault::open(&bytes, &key).is_ok());
        assert!(matches!(
            Vault::open(&bytes, &CompositeKey::password(PASSWORD)),
            Err(OpenError::WrongKey)
        ));
    }
}

#[test]
fn a_vault_knows_what_its_master_key_is_made_of_after_opening_and_changing_it() {
    let key_file = new_key_file();
    let password = CompositeKey::password(PASSWORD);
    let both = CompositeKey::password_and_key_file(PASSWORD, &key_file).unwrap();
    let bytes = create("Keyed", &both, &quick()).save().unwrap();

    let mut vault = Vault::open(&bytes, &both).unwrap();
    assert_eq!(vault.key_parts(), KeyParts::PasswordAndKeyFile);

    vault.change_key(&CompositeKey::key_file(&key_file).unwrap(), now());
    assert_eq!(vault.key_parts(), KeyParts::KeyFile);
    vault.change_key(&password, now());
    assert_eq!(vault.key_parts(), KeyParts::Password);
    // A version of the Vault file opened to Merge it doesn't change the Vault's key.
    assert_eq!(
        vault.open_version(&bytes).unwrap().key_parts(),
        KeyParts::Password
    );
}

#[test]
fn a_new_key_file_is_a_keepass_xml_v2_key_file_unlike_any_other() {
    let first = new_key_file();
    let second = new_key_file();

    let text = std::str::from_utf8(&first).unwrap();
    assert!(text.contains("<Version>2.0</Version>"), "{text}");
    assert_ne!(&first[..], &second[..]);
    // The independent reader takes it as a key file too, and agrees on the key.
    let key = CompositeKey::key_file(&first).unwrap();
    let bytes = create("Keyed", &key, &quick()).save().unwrap();
    let independent = DatabaseKey::new().with_keyfile(&mut &first[..]).unwrap();
    independent_config(&bytes, independent);
}

#[test]
fn after_changing_the_master_key_only_the_new_key_opens_the_saved_vault() {
    let old = CompositeKey::password(PASSWORD);
    let new = CompositeKey::password("a new master password");
    let mut vault = create("Personal", &old, &quick());
    let root = vault.root().id();
    vault.create_entry(root, &fields("Mail"), now()).unwrap();

    vault.change_key(&new, now());
    let bytes = vault.save().unwrap();

    assert!(matches!(
        Vault::open(&bytes, &old),
        Err(OpenError::WrongKey)
    ));
    let reopened = Vault::open(&bytes, &new).unwrap();
    assert_eq!(reopened, vault);
    assert_eq!(reopened.root().entries()[0].title(), "Mail");
}

#[test]
fn a_changed_key_can_still_open_a_version_saved_with_the_old_key_to_merge_it() {
    let old = CompositeKey::password(PASSWORD);
    let mut vault = create("Personal", &old, &quick());
    let root = vault.root().id();
    let mut elsewhere = Vault::open(&vault.save().unwrap(), &old).unwrap();
    elsewhere
        .create_entry(root, &fields("Added elsewhere"), now())
        .unwrap();
    let theirs = elsewhere.save().unwrap();

    vault.change_key(&CompositeKey::password("new"), now());
    let version = vault
        .open_version(&theirs)
        .expect("the old key is kept until the new one is saved");
    vault.merge(&version);

    assert_eq!(vault.root().entries()[0].title(), "Added elsewhere");
}

#[test]
fn once_the_old_key_is_forgotten_versions_saved_with_it_no_longer_open() {
    let old = CompositeKey::password(PASSWORD);
    let mut vault = create("Personal", &old, &quick());
    let theirs = vault.save().unwrap();

    vault.change_key(&CompositeKey::password("new"), now());
    vault.forget_previous_keys();

    assert!(matches!(
        vault.open_version(&theirs),
        Err(OpenError::WrongKey)
    ));
}

#[test]
fn changing_the_key_twice_before_saving_keeps_every_key_the_vault_file_may_have() {
    let old = CompositeKey::password(PASSWORD);
    let mut vault = create("Personal", &old, &quick());
    let with_old_key = vault.save().unwrap();

    vault.change_key(&CompositeKey::password("first"), now());
    // A save may write the first change while the second is made.
    let with_first_key = vault.save().unwrap();
    vault.change_key(&CompositeKey::password("second"), now());

    assert!(vault.open_version(&with_old_key).is_ok());
    assert!(vault.open_version(&with_first_key).is_ok());
}

#[test]
fn changing_cipher_and_key_derivation_is_saved_and_read_by_other_clients() {
    let key = CompositeKey::password(PASSWORD);
    let mut vault = create("Personal", &key, &quick());
    let settings = Encryption {
        cipher: Cipher::Aes256,
        kdf: Kdf::Argon2d(Argon2 {
            memory: 2 * 1024 * 1024,
            iterations: 3,
            parallelism: 1,
        }),
    };

    vault.set_encryption(&settings).unwrap();
    let bytes = vault.save().unwrap();

    assert_eq!(Vault::open(&bytes, &key).unwrap().encryption(), settings);
    let config = independent_config(&bytes, DatabaseKey::new().with_password(PASSWORD));
    assert_eq!(config.outer_cipher_config, OuterCipherConfig::AES256);
    assert!(
        matches!(
            config.kdf_config,
            KdfConfig::Argon2 {
                iterations: 3,
                memory: 2_097_152,
                parallelism: 1,
                ..
            }
        ),
        "{:?}",
        config.kdf_config
    );
}

#[test]
fn aes_kdf_can_be_chosen_with_its_rounds() {
    let key = CompositeKey::password(PASSWORD);
    let mut vault = create("Personal", &key, &quick());
    let settings = Encryption {
        cipher: Cipher::Twofish,
        kdf: Kdf::AesKdf { rounds: 1_000 },
    };

    vault.set_encryption(&settings).unwrap();
    let bytes = vault.save().unwrap();

    assert_eq!(Vault::open(&bytes, &key).unwrap().encryption(), settings);
    let config = independent_config(&bytes, DatabaseKey::new().with_password(PASSWORD));
    assert_eq!(config.outer_cipher_config, OuterCipherConfig::Twofish);
    assert!(matches!(
        config.kdf_config,
        KdfConfig::Aes { rounds: 1_000 }
    ));
}

#[test]
fn a_kdbx_3_vault_reports_aes_kdf_and_moves_to_argon2id_when_asked() {
    let key = CompositeKey::password(FIXTURE_PASSWORD);
    let mut vault = Vault::open(KEEPASSXC_KDBX3, &key).unwrap();
    assert!(matches!(vault.encryption().kdf, Kdf::AesKdf { .. }));

    vault.set_encryption(&quick()).unwrap();
    let bytes = vault.save().unwrap();

    let reopened = Vault::open(&bytes, &key).unwrap();
    assert_eq!(reopened.encryption(), quick());
    assert_eq!(reopened, vault);
}

#[test]
fn settings_key_derivation_cannot_run_with_are_refused_and_change_nothing() {
    let argon2 = |memory, iterations, parallelism| Encryption {
        cipher: Cipher::ChaCha20,
        kdf: Kdf::Argon2id(Argon2 {
            memory,
            iterations,
            parallelism,
        }),
    };
    let cases = [
        (argon2(1024 * 1024, 0, 1), EncryptionError::Iterations),
        (argon2(1024 * 1024, 1, 0), EncryptionError::Parallelism),
        // Argon2 needs 8 KiB per lane.
        (argon2(64 * 1024, 1, 16), EncryptionError::Memory),
        (argon2(1024 * 1024 + 1, 1, 1), EncryptionError::Memory),
        (
            Encryption {
                cipher: Cipher::Aes256,
                kdf: Kdf::AesKdf { rounds: 0 },
            },
            EncryptionError::Rounds,
        ),
    ];
    let mut vault = create("Personal", &CompositeKey::password(PASSWORD), &quick());

    for (settings, error) in cases {
        assert_eq!(vault.set_encryption(&settings), Err(error));
        assert_eq!(settings.check(), Err(error));
        assert!(matches!(
            Vault::create("x", &CompositeKey::password(PASSWORD), &settings, now()),
            Err(found) if found == error
        ));
        assert_eq!(vault.encryption(), quick());
    }
}
