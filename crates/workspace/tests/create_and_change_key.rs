//! Creating a new Vault file, and changing an open Vault's master key, cipher and key
//! derivation. A changed key reaches the Vault file through a save, which still Merges changes
//! made elsewhere with the old key; once saved, the old key is forgotten.

mod support;

use std::sync::Arc;

use keefast_core::{
    Argon2, Cipher, CompositeKey, Encryption, EncryptionError, EntryFields, Kdf, OpenError,
    Timestamp, Vault,
};
use keefast_workspace::{
    ChangeKeyError, CreateVaultError, EntryScope, NewKeyError, NewVault, SaveStatus,
    SaveVaultError, UnlockSecrets, VaultId, VaultLocation, Workspace, write_new_key_file,
};
use secrecy::SecretString;
use support::{FakeClipboard, FakeClock, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault};

const START: u64 = 1_800_000_000;
const NEW_PASSWORD: &str = "a brand new master password";

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

fn workspace(storage: &Arc<FakeStorage>) -> Workspace {
    let clock = Arc::new(FakeClock::at_unix_seconds(START));
    Workspace::with_clock(storage.clone(), clock)
}

fn new_vault(name: &str, secrets: UnlockSecrets) -> NewVault {
    NewVault {
        name: name.into(),
        secrets,
        encryption: quick(),
    }
}

fn create(workspace: &mut Workspace, location: &VaultLocation, new: NewVault) -> VaultId {
    let pending = workspace.begin_create(location.clone(), new).unwrap();
    workspace.add_unlocked(pending.create().unwrap())
}

fn fields(title: &str) -> EntryFields {
    EntryFields {
        title: title.into(),
        password: SecretString::from("hunter2"),
        ..Default::default()
    }
}

fn titles(vault: &Vault) -> Vec<String> {
    let mut titles: Vec<String> = vault
        .root()
        .all_entries()
        .map(|entry| entry.title().to_owned())
        .collect();
    titles.sort();
    titles
}

/// Opens a personal Vault from the KeePassXC fixture.
fn opened() -> (Arc<FakeStorage>, Workspace, VaultId) {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let mut workspace = workspace(&storage);
    let vault = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    (storage, workspace, vault)
}

fn stored(storage: &FakeStorage) -> Vec<u8> {
    storage.file(&personal_vault()).unwrap()
}

#[test]
fn a_created_vault_is_written_opened_shown_and_remembered() {
    let storage = Arc::new(FakeStorage::default());
    let mut workspace = workspace(&storage);

    let id = create(
        &mut workspace,
        &personal_vault(),
        new_vault("Personal", UnlockSecrets::password(NEW_PASSWORD)),
    );

    let written = storage
        .file(&personal_vault())
        .expect("the Vault file is written");
    let reopened = Vault::open(&written, &CompositeKey::password(NEW_PASSWORD)).unwrap();
    assert_eq!(reopened.root().name(), "Personal");
    assert_eq!(reopened.encryption(), quick());
    assert_eq!(
        workspace.vault(id).unwrap().vault().root().name(),
        "Personal"
    );
    assert_eq!(workspace.view().scope, EntryScope::Vault(id));
    assert_eq!(workspace.recent_vaults(), [personal_vault()]);
    assert_eq!(workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn a_created_vault_is_saved_like_any_other() {
    let storage = Arc::new(FakeStorage::default());
    let mut workspace = workspace(&storage);
    let id = create(
        &mut workspace,
        &personal_vault(),
        new_vault("Personal", UnlockSecrets::password(NEW_PASSWORD)),
    );
    let root = workspace.vault(id).unwrap().vault().root().id();

    workspace.create_entry(id, root, &fields("Mail")).unwrap();
    workspace.save_vault(id).unwrap();

    let written = storage.file(&personal_vault()).unwrap();
    let reopened = Vault::open(&written, &CompositeKey::password(NEW_PASSWORD)).unwrap();
    assert_eq!(titles(&reopened), ["Mail"]);
}

#[test]
fn a_vault_file_already_open_is_not_created_over() {
    let (storage, workspace, id) = opened();

    let result = workspace.begin_create(
        personal_vault(),
        new_vault("Personal", UnlockSecrets::password(NEW_PASSWORD)),
    );

    assert!(matches!(result, Err(CreateVaultError::AlreadyOpen(open)) if open == id));
    assert_eq!(stored(&storage), KEEPASSXC_KDBX4);
}

#[test]
fn a_new_vault_needs_a_master_password_or_a_key_file() {
    let storage = Arc::new(FakeStorage::default());
    let workspace = workspace(&storage);
    let pending = workspace
        .begin_create(
            personal_vault(),
            new_vault("Personal", UnlockSecrets::password("")),
        )
        .unwrap();

    let result = pending.create();

    assert!(matches!(
        result,
        Err(CreateVaultError::Key(NewKeyError::NoKey))
    ));
    assert!(storage.file(&personal_vault()).is_none());
}

#[test]
fn settings_key_derivation_cannot_run_with_are_refused_before_anything_is_written() {
    let storage = Arc::new(FakeStorage::default());
    let workspace = workspace(&storage);
    let mut new = new_vault("Personal", UnlockSecrets::password(NEW_PASSWORD));
    new.encryption.kdf = Kdf::AesKdf { rounds: 0 };

    let result = workspace.begin_create(personal_vault(), new);

    assert!(matches!(
        result,
        Err(CreateVaultError::Encryption(EncryptionError::Rounds))
    ));
}

#[test]
fn a_new_key_file_is_written_and_keys_a_new_vault_alone() {
    let dir = tempfile::tempdir().unwrap();
    let key_file = dir.path().join("personal.keyx");
    write_new_key_file(&key_file).unwrap();
    let storage = Arc::new(FakeStorage::default());
    let mut workspace = workspace(&storage);

    create(
        &mut workspace,
        &personal_vault(),
        new_vault(
            "Personal",
            UnlockSecrets::new(SecretString::from(""), Some(key_file.clone())),
        ),
    );

    let contents = std::fs::read(&key_file).unwrap();
    let written = storage.file(&personal_vault()).unwrap();
    assert!(Vault::open(&written, &CompositeKey::key_file(&contents).unwrap()).is_ok());
}

#[test]
fn an_existing_file_is_never_overwritten_with_a_new_key_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.keyx");
    std::fs::write(&path, b"another Vault's key").unwrap();

    let result = write_new_key_file(&path);

    assert_eq!(
        result.unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"another Vault's key");
}

#[test]
fn a_changed_master_key_is_unsaved_until_saved_then_only_it_opens_the_vault_file() {
    let (storage, mut workspace, id) = opened();

    workspace
        .change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();

    assert_eq!(workspace.save_status(), SaveStatus::Unsaved);
    assert!(Vault::open(&stored(&storage), &CompositeKey::password(PASSWORD)).is_ok());
    workspace.save_vault(id).unwrap();
    assert_eq!(workspace.save_status(), SaveStatus::Saved);
    assert!(matches!(
        Vault::open(&stored(&storage), &CompositeKey::password(PASSWORD)),
        Err(OpenError::WrongKey)
    ));
    assert!(Vault::open(&stored(&storage), &CompositeKey::password(NEW_PASSWORD)).is_ok());
}

#[test]
fn a_new_master_key_needs_a_password_or_a_key_file() {
    let (_, mut workspace, id) = opened();

    let result = workspace.change_master_key(id, &UnlockSecrets::password(""));

    assert!(matches!(
        result,
        Err(ChangeKeyError::Key(NewKeyError::NoKey))
    ));
    assert_eq!(workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn a_missing_key_file_leaves_the_master_key_as_it_was() {
    let (_, mut workspace, id) = opened();
    let missing = std::env::temp_dir().join("keefast-018-no-such-key-file.keyx");

    let result = workspace.change_master_key(
        id,
        &UnlockSecrets::new(SecretString::from(NEW_PASSWORD), Some(missing)),
    );

    assert!(matches!(
        result,
        Err(ChangeKeyError::Key(NewKeyError::KeyFileNotFound(_)))
    ));
    assert_eq!(workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn changes_made_elsewhere_with_the_old_key_are_merged_into_the_save_of_the_new_one() {
    let (storage, mut workspace, id) = opened();
    let mut theirs = Vault::open(&stored(&storage), &CompositeKey::password(PASSWORD)).unwrap();
    let root = theirs.root().id();
    theirs
        .create_entry(
            root,
            &fields("Added elsewhere"),
            Timestamp::from_unix_seconds(START as i64),
        )
        .unwrap();
    storage.change_elsewhere(&personal_vault(), &theirs.save().unwrap());

    workspace
        .change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();
    workspace.save_vault(id).unwrap();

    let saved = Vault::open(&stored(&storage), &CompositeKey::password(NEW_PASSWORD)).unwrap();
    assert!(titles(&saved).contains(&"Added elsewhere".to_owned()));
    assert!(titles(workspace.vault(id).unwrap().vault()).contains(&"Added elsewhere".to_owned()));
}

#[test]
fn a_change_racing_the_save_of_a_new_key_is_merged_and_the_save_retried() {
    let (storage, mut workspace, id) = opened();
    let mut theirs = Vault::open(&stored(&storage), &CompositeKey::password(PASSWORD)).unwrap();
    let root = theirs.root().id();
    theirs
        .create_entry(
            root,
            &fields("Raced"),
            Timestamp::from_unix_seconds(START as i64),
        )
        .unwrap();
    storage.change_before_next_write(&personal_vault(), &theirs.save().unwrap());

    workspace
        .change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();
    workspace.save_vault(id).unwrap();

    let saved = Vault::open(&stored(&storage), &CompositeKey::password(NEW_PASSWORD)).unwrap();
    assert!(titles(&saved).contains(&"Raced".to_owned()));
}

#[test]
fn once_the_new_key_is_saved_a_copy_with_the_old_key_is_not_merged_or_overwritten() {
    let (storage, mut workspace, id) = opened();
    let old_copy = Vault::open(&stored(&storage), &CompositeKey::password(PASSWORD)).unwrap();
    workspace
        .change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();
    workspace.save_vault(id).unwrap();

    // Another device, still on the old key, saves over the Vault file.
    let mut theirs = old_copy;
    let root = theirs.root().id();
    theirs
        .create_entry(
            root,
            &fields("Old key"),
            Timestamp::from_unix_seconds(START as i64),
        )
        .unwrap();
    let their_bytes = theirs.save().unwrap();
    storage.change_elsewhere(&personal_vault(), &their_bytes);
    let root = workspace.vault(id).unwrap().vault().root().id();
    workspace
        .create_entry(id, root, &fields("New key"))
        .unwrap();
    let result = workspace.save_vault(id);

    assert!(matches!(
        result,
        Err(SaveVaultError::Merge(OpenError::WrongKey))
    ));
    assert_eq!(stored(&storage), their_bytes, "their Vault file is kept");
    assert_eq!(workspace.save_status(), SaveStatus::Failed);
}

#[test]
fn changing_the_cipher_and_key_derivation_is_saved() {
    let (storage, mut workspace, id) = opened();

    workspace.set_encryption(id, &quick()).unwrap();

    assert_eq!(workspace.vault(id).unwrap().vault().encryption(), quick());
    assert_eq!(workspace.save_status(), SaveStatus::Unsaved);
    workspace.save_vault(id).unwrap();
    let saved = Vault::open(&stored(&storage), &CompositeKey::password(PASSWORD)).unwrap();
    assert_eq!(saved.encryption(), quick());
}

#[test]
fn settings_the_key_derivation_cannot_run_with_are_refused() {
    let (_, mut workspace, id) = opened();
    let before = workspace.vault(id).unwrap().vault().encryption();
    let mut settings = quick();
    settings.kdf = Kdf::Argon2id(Argon2 {
        memory: 1024 * 1024,
        iterations: 0,
        parallelism: 1,
    });

    let result = workspace.set_encryption(id, &settings);

    assert!(matches!(
        result,
        Err(ChangeKeyError::Encryption(EncryptionError::Iterations))
    ));
    assert_eq!(workspace.vault(id).unwrap().vault().encryption(), before);
    assert_eq!(workspace.save_status(), SaveStatus::Saved);
}

#[test]
fn a_locked_vault_cannot_have_its_key_changed() {
    let (_, mut workspace, id) = opened();
    workspace.lock_vault(id, &mut FakeClipboard::default());

    let result = workspace.change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD));

    assert!(matches!(result, Err(ChangeKeyError::VaultNotOpen)));
}

#[test]
fn a_locked_vault_with_a_new_key_saved_unlocks_with_the_new_key_only() {
    let (_, mut workspace, id) = opened();
    workspace
        .change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();
    workspace.save_vault(id).unwrap();
    assert!(
        workspace
            .lock_vault(id, &mut FakeClipboard::default())
            .is_empty()
    );

    let old = workspace.open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD));
    assert!(old.is_err());
    let unlocked = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();
    assert_eq!(unlocked, id);
}

#[test]
fn an_existing_file_is_never_replaced_with_a_new_vault() {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let workspace = workspace(&storage);
    let pending = workspace
        .begin_create(
            personal_vault(),
            new_vault("Personal", UnlockSecrets::password(NEW_PASSWORD)),
        )
        .unwrap();

    let result = pending.create();

    assert!(matches!(result, Err(CreateVaultError::FileExists)));
    assert_eq!(stored(&storage), KEEPASSXC_KDBX4);
}

#[test]
fn a_new_vault_without_a_name_is_named_after_its_vault_file() {
    let storage = Arc::new(FakeStorage::default());
    let mut workspace = workspace(&storage);

    let id = create(
        &mut workspace,
        &personal_vault(),
        new_vault(" ", UnlockSecrets::password(NEW_PASSWORD)),
    );

    let name = personal_vault().display_name();
    assert_eq!(workspace.vault(id).unwrap().vault().root().name(), name);
}

#[test]
fn a_save_asked_for_while_another_runs_follows_it_even_without_autosave() {
    let (storage, mut workspace, id) = opened();
    let running = workspace.begin_save(id).unwrap();
    workspace
        .change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();

    assert!(matches!(
        workspace.begin_save(id),
        Err(SaveVaultError::InProgress)
    ));
    workspace.finish_save(running.run()).unwrap();
    let follow_ups = workspace.begin_autosaves();
    assert_eq!(follow_ups.len(), 1);
    for save in follow_ups {
        workspace.finish_save(save.run()).unwrap();
    }

    assert!(Vault::open(&stored(&storage), &CompositeKey::password(NEW_PASSWORD)).is_ok());
    assert_eq!(workspace.save_status(), SaveStatus::Saved);
    assert!(
        workspace.begin_autosaves().is_empty(),
        "asked once, saved once"
    );
}

#[test]
fn a_key_changed_again_while_the_first_change_is_saved_still_merges_and_saves() {
    let (storage, mut workspace, id) = opened();
    workspace
        .change_master_key(id, &UnlockSecrets::password("first new password"))
        .unwrap();
    let first = workspace.begin_save(id).unwrap();
    workspace
        .change_master_key(id, &UnlockSecrets::password(NEW_PASSWORD))
        .unwrap();
    workspace.finish_save(first.run()).unwrap();

    // Another device that has the first new key changes the Vault file meanwhile.
    let mut theirs = Vault::open(
        &stored(&storage),
        &CompositeKey::password("first new password"),
    )
    .unwrap();
    let root = theirs.root().id();
    theirs
        .create_entry(
            root,
            &fields("Elsewhere"),
            Timestamp::from_unix_seconds(START as i64),
        )
        .unwrap();
    storage.change_elsewhere(&personal_vault(), &theirs.save().unwrap());
    workspace.save_vault(id).unwrap();

    let saved = Vault::open(&stored(&storage), &CompositeKey::password(NEW_PASSWORD)).unwrap();
    assert!(titles(&saved).contains(&"Elsewhere".to_owned()));
}

#[test]
fn settings_the_vault_has_already_are_no_change() {
    let (_, mut workspace, id) = opened();
    let current = workspace.vault(id).unwrap().vault().encryption();

    workspace.set_encryption(id, &current).unwrap();

    assert_eq!(workspace.save_status(), SaveStatus::Saved);
}
