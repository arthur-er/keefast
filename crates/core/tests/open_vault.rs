use keefast_core::{Argon2, CompositeKey, Encryption, EncryptionError, Kdf, OpenError, Vault};
use secrecy::ExposeSecret;

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASSXC_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx3.kdbx");
const KEEPASS2_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";

fn open_fixture() -> Vault {
    Vault::open(KEEPASSXC_KDBX4, &CompositeKey::password(PASSWORD)).expect("fixture opens")
}

#[test]
fn opens_a_keepassxc_kdbx4_vault_file_with_its_master_password() {
    let vault = open_fixture();

    let root = vault.root();
    let group_names: Vec<&str> = root.groups().iter().map(|g| g.name()).collect();
    assert_eq!(group_names, ["Work", "Personal"]);

    let root_titles: Vec<&str> = root.entries().iter().map(|e| e.title()).collect();
    assert_eq!(root_titles, ["Root Entry"]);
}

#[test]
fn opens_a_keepassxc_kdbx3_1_vault_file() {
    let vault = Vault::open(KEEPASSXC_KDBX3, &CompositeKey::password(PASSWORD)).unwrap();

    let titles: Vec<&str> = vault.root().all_entries().map(|e| e.title()).collect();
    assert_eq!(titles, ["Root Entry", "GitHub", "Prod DB", "Email"]);
}

#[test]
fn exposes_entry_fields_with_the_password_kept_secret() {
    let vault = open_fixture();

    let work = &vault.root().groups()[0];
    let github = &work.entries()[0];
    assert_eq!(github.title(), "GitHub");
    assert_eq!(github.username(), "octocat");
    assert_eq!(github.url(), "https://github.com/login");
    assert_eq!(github.notes(), "Work account");
    assert_eq!(github.password().expose_secret(), "gh-secret");
    assert!(!format!("{github:?}").contains("gh-secret"));
}

#[test]
fn nested_groups_are_preserved() {
    let vault = open_fixture();

    let work = &vault.root().groups()[0];
    let servers = &work.groups()[0];
    assert_eq!(servers.name(), "Servers");
    assert_eq!(servers.entries()[0].title(), "Prod DB");
}

#[test]
fn groups_and_entries_can_be_looked_up_by_id() {
    let vault = open_fixture();

    let personal = &vault.root().groups()[1];
    let email = &personal.entries()[0];
    assert_eq!(
        vault.group(personal.id()).map(|g| g.name()),
        Some("Personal")
    );
    assert_eq!(vault.entry(email.id()).map(|e| e.title()), Some("Email"));
}

#[test]
fn all_entries_walks_the_group_and_its_descendants() {
    let vault = open_fixture();

    let titles = |group: &keefast_core::Group| -> Vec<String> {
        group.all_entries().map(|e| e.title().to_owned()).collect()
    };
    assert_eq!(titles(&vault.root().groups()[0]), ["GitHub", "Prod DB"]);
    assert_eq!(titles(vault.root()).len(), 4);
}

#[test]
fn a_wrong_master_password_is_reported_as_a_wrong_key() {
    let result = Vault::open(KEEPASSXC_KDBX4, &CompositeKey::password("not it"));

    assert!(matches!(result, Err(OpenError::WrongKey)));
}

#[test]
fn bytes_that_are_not_a_vault_file_are_reported_as_corrupt() {
    let result = Vault::open(
        b"definitely not a kdbx file",
        &CompositeKey::password(PASSWORD),
    );

    assert!(matches!(result, Err(OpenError::Corrupt(_))));
}

#[test]
fn a_truncated_vault_file_is_reported_as_corrupt() {
    let truncated = &KEEPASSXC_KDBX4[..KEEPASSXC_KDBX4.len() / 2];

    let result = Vault::open(truncated, &CompositeKey::password(PASSWORD));

    assert!(matches!(result, Err(OpenError::Corrupt(_))));
}

#[test]
fn a_kdbx3_1_vault_file_with_an_altered_header_is_reported_as_corrupt() {
    // Insert a comment field: decryption is unaffected, but the header no longer matches the
    // hash KDBX 3.1 keeps of it.
    let mut altered = KEEPASSXC_KDBX3[..12].to_vec();
    altered.extend_from_slice(&[1, 4, 0, b'n', b'o', b't', b'e']);
    altered.extend_from_slice(&KEEPASSXC_KDBX3[12..]);

    let result = Vault::open(&altered, &CompositeKey::password(PASSWORD));

    assert!(matches!(result, Err(OpenError::Corrupt(_))));
}

#[test]
fn a_kdbx_version_from_the_future_is_reported_as_unsupported() {
    let mut future = KEEPASSXC_KDBX4.to_vec();
    future[10..12].copy_from_slice(&5u16.to_le_bytes());

    let result = Vault::open(&future, &CompositeKey::password(PASSWORD));

    assert!(matches!(result, Err(OpenError::Unsupported(_))));
}

#[test]
fn a_keepass_1_file_is_reported_as_unsupported() {
    // KeePass 1.x (KDB) shares the signature but has its own file type.
    let mut kdb = vec![0x03, 0xd9, 0xa2, 0x9a, 0x65, 0xfb, 0x4b, 0xb5];
    kdb.extend_from_slice(&[0; 116]);

    let result = Vault::open(&kdb, &CompositeKey::password(PASSWORD));

    assert!(matches!(result, Err(OpenError::Unsupported(_))));
}

/// A KDBX 4 Vault file with the KDF parameter `name`, a 64-bit number, set to `value`, and its
/// header hash made to match, as a crafted Vault file would have it.
fn kdbx4_with_kdf_parameter(file: &[u8], name: &str, value: u64) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let mut file = file.to_vec();
    let le_u32 = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap()) as usize;
    let mut found = false;
    let mut pos = 12;
    let header_end = loop {
        let (id, len) = (file[pos], le_u32(&file[pos + 1..pos + 5]));
        let start = pos + 5;
        pos = start + len;
        if id == 0 {
            break pos;
        }
        if id != 11 {
            continue;
        }
        // The KDF parameters: a version, then (type, key length, key, value length, value).
        let mut at = start + 2;
        while file[at] != 0 {
            let key_len = le_u32(&file[at + 1..at + 5]);
            let value_at = at + 5 + key_len;
            let value_len = le_u32(&file[value_at..value_at + 4]);
            if &file[at + 5..value_at] == name.as_bytes() {
                assert_eq!(value_len, 8, "{name} is a 64-bit number");
                file[value_at + 4..value_at + 12].copy_from_slice(&value.to_le_bytes());
                found = true;
            }
            at = value_at + 4 + value_len;
        }
    };
    assert!(found, "the Vault file has KDF parameter {name}");
    let hash: [u8; 32] = Sha256::digest(&file[..header_end]).into();
    file[header_end..header_end + 32].copy_from_slice(&hash);
    file
}

/// A KDBX 3.1 Vault file with `rounds` of AES-KDF. Only the hash in its encrypted XML covers
/// its header, and that is checked after key derivation.
fn kdbx3_with_rounds(file: &[u8], rounds: u64) -> Vec<u8> {
    let mut file = file.to_vec();
    let mut pos = 12;
    while file[pos] != 0 {
        let len = u16::from_le_bytes([file[pos + 1], file[pos + 2]]) as usize;
        if file[pos] == 6 {
            file[pos + 3..pos + 11].copy_from_slice(&rounds.to_le_bytes());
        }
        pos += 3 + len;
    }
    file
}

/// Which limit opening the Vault file went over. Without the limits, these tests would run out
/// of memory or run for years.
fn over_limit(file: &[u8]) -> EncryptionError {
    match Vault::open(file, &CompositeKey::password(PASSWORD)) {
        Err(OpenError::KdfOverLimit(error)) => error,
        Err(error) => panic!("opening failed otherwise: {error}"),
        Ok(_) => panic!("the Vault file opened"),
    }
}

#[test]
fn argon2_memory_over_the_limit_fails_before_key_derivation_starts() {
    // 1 TiB, which KeePassXC lets users type in but no machine has to give.
    let file = kdbx4_with_kdf_parameter(KEEPASS2_KDBX4, "M", 1 << 40);

    assert_eq!(over_limit(&file), EncryptionError::Memory);
}

#[test]
fn argon2_work_over_the_limit_fails_before_key_derivation_starts() {
    let most_kdbx_holds = kdbx4_with_kdf_parameter(KEEPASS2_KDBX4, "I", u64::from(u32::MAX));
    let more = kdbx4_with_kdf_parameter(KEEPASS2_KDBX4, "I", u64::MAX);

    assert_eq!(over_limit(&most_kdbx_holds), EncryptionError::Cost);
    assert_eq!(over_limit(&more), EncryptionError::Iterations);
}

#[test]
fn aes_kdf_rounds_over_the_limit_fail_before_key_derivation_starts() {
    let kdbx3 = kdbx3_with_rounds(KEEPASSXC_KDBX3, Kdf::MAX_AES_ROUNDS + 1);
    let kdbx4 = kdbx4_with_kdf_parameter(KEEPASSXC_KDBX4, "R", u64::MAX);

    assert_eq!(over_limit(&kdbx3), EncryptionError::Rounds);
    assert_eq!(over_limit(&kdbx4), EncryptionError::Rounds);
}

#[test]
fn key_derivation_at_the_limits_can_be_chosen() {
    let defaults = Encryption::default();
    let argon2 = Argon2 {
        memory: Argon2::MAX_MEMORY,
        iterations: Argon2::MAX_COST / Argon2::MAX_MEMORY,
        parallelism: 8,
    };
    let rounds = Kdf::MAX_AES_ROUNDS;
    for kdf in [Kdf::Argon2id(argon2), Kdf::AesKdf { rounds }] {
        let settings = Encryption { kdf, ..defaults };
        assert_eq!(settings.check(), Ok(()), "{kdf:?}");
    }
}
