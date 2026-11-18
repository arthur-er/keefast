//! Connecting a Dropbox account, browsing it, and opening Vault files from it, against a fake
//! Dropbox served over HTTP.

mod support;

use std::sync::Arc;

use keefast_core::{Argon2, Cipher, Encryption, Kdf};
use keefast_workspace::{
    CreateVaultError, Dropbox, DropboxAccount, DropboxConfig, DropboxError, DropboxItem,
    EntryScope, NewVault, OpenVaultError, StorageError, StorageProvider, SyncStatus, UnlockSecrets,
    VaultId, VaultLocation, Workspace,
};
use support::fake_dropbox::{ACCOUNT_EMAIL, ACCOUNT_NAME, APP_KEY, FakeDropbox};
use support::{
    AppearsAfterCheck, FakeConfigStore, FakeSecretStore, FakeStorage, FakeVaultCache,
    KEEPASSXC_KDBX4, PASSWORD,
};

fn config(fake: &FakeDropbox) -> DropboxConfig {
    DropboxConfig {
        app_key: APP_KEY.into(),
        api_url: fake.url().into(),
        content_url: fake.url().into(),
        authorize_url: format!("{}/oauth2/authorize", fake.url()),
        redirect_port: 0,
    }
}

fn dropbox(fake: &FakeDropbox, secrets: &Arc<FakeSecretStore>) -> Arc<Dropbox> {
    Arc::new(Dropbox::new(config(fake), secrets.clone()))
}

/// Connects the way the user does: keefast sends them to Dropbox's page, they allow it, and
/// Dropbox sends the browser back to keefast.
fn connect(dropbox: &Dropbox) -> DropboxAccount {
    let authorization = dropbox.begin_connect().expect("connecting starts");
    let browser = FakeDropbox::visit_in_browser(authorization.url());
    let account = dropbox
        .finish_connect(authorization)
        .expect("Dropbox connects");
    let page = browser.join().unwrap();
    assert!(page.contains("Dropbox is connected"), "{page}");
    account
}

fn connected(fake: &FakeDropbox) -> (Arc<Dropbox>, Arc<FakeSecretStore>) {
    let secrets = Arc::new(FakeSecretStore::default());
    let dropbox = dropbox(fake, &secrets);
    connect(&dropbox);
    (dropbox, secrets)
}

fn personal() -> VaultLocation {
    VaultLocation::Dropbox("/Vaults/Personal.kdbx".into())
}

fn names(items: &[DropboxItem]) -> Vec<&str> {
    items.iter().map(|item| item.name.as_str()).collect()
}

#[test]
fn connecting_uses_pkce_without_a_client_secret_and_keeps_tokens_in_the_secret_store() {
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());
    let dropbox = dropbox(&fake, &secrets);
    assert_eq!(dropbox.account().unwrap(), None);

    let authorization = dropbox.begin_connect().unwrap();
    let url = authorization.url().to_owned();
    assert!(url.starts_with(&format!("{}/oauth2/authorize?", fake.url())));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("token_access_type=offline"));
    let browser = FakeDropbox::visit_in_browser(&url);
    let account = dropbox.finish_connect(authorization).unwrap();
    browser.join().unwrap();

    assert_eq!(
        account,
        DropboxAccount {
            name: ACCOUNT_NAME.into(),
            email: ACCOUNT_EMAIL.into(),
        }
    );
    assert_eq!(dropbox.account().unwrap(), Some(account));
    assert!(!fake.saw_client_secret());
    let stored = secrets
        .secret("dropbox")
        .expect("the tokens are in the secret store");
    assert!(stored.contains("refresh-"), "{stored}");
}

#[test]
fn a_machine_without_a_browser_connects_by_the_code_dropboxs_page_shows() {
    // `keefast-agent dropbox connect` on a server: the user opens the page on another machine,
    // and pastes the code Dropbox shows there.
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());
    let dropbox = dropbox(&fake, &secrets);

    let authorization = dropbox.begin_connect_by_code().unwrap();
    assert!(!authorization.url().contains("redirect_uri"));
    assert!(authorization.url().contains("code_challenge_method=S256"));
    let page = FakeDropbox::visit_in_browser(authorization.url())
        .join()
        .unwrap();
    let code = page
        .strip_prefix("Your code: ")
        .expect("the page shows a code");
    let wrong = dropbox.finish_connect_with_code(&authorization, "not-the-code");
    let account = dropbox
        .finish_connect_with_code(&authorization, &format!("  {code}\n"))
        .unwrap();

    assert!(wrong.is_err());
    assert_eq!(account.email, ACCOUNT_EMAIL);
    assert_eq!(dropbox.account().unwrap(), Some(account));
    assert!(secrets.secret("dropbox").unwrap().contains("refresh-"));
}

#[test]
fn another_account_is_connected_after_dropbox_asks_to_sign_in_and_the_first_stays_till_then() {
    let fake = FakeDropbox::start();
    let (dropbox, _) = connected(&fake);
    let first = dropbox.account().unwrap();
    assert!(first.is_some());

    let authorization = dropbox.begin_connect_another().unwrap();
    assert!(authorization.url().contains("force_reauthentication=true"));
    assert_eq!(
        dropbox.account().unwrap(),
        first,
        "connected while the user signs in"
    );
    authorization.canceller().cancel();
    assert!(matches!(
        dropbox.finish_connect(authorization),
        Err(DropboxError::Cancelled)
    ));
    assert_eq!(
        dropbox.account().unwrap(),
        first,
        "stopping keeps the account"
    );

    let authorization = dropbox.begin_connect_another().unwrap();
    let browser = FakeDropbox::visit_in_browser(authorization.url());
    let account = dropbox.finish_connect(authorization).unwrap();
    browser.join().unwrap();
    assert_eq!(dropbox.account().unwrap(), Some(account));
}

#[test]
fn connecting_lets_in_the_account_the_browser_is_signed_in_to() {
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());

    let authorization = dropbox(&fake, &secrets).begin_connect().unwrap();

    assert!(!authorization.url().contains("force_reauthentication"));
}

#[test]
fn tokens_never_reach_the_app_config() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, secrets) = connected(&fake);
    let config = Arc::new(FakeConfigStore::default());
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default()))
        .with_config(config.clone())
        .with_dropbox(dropbox);
    workspace
        .open_vault(&personal(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    let saved = format!("{:?}", config.saved().unwrap());
    let stored = secrets.secret("dropbox").unwrap();
    let refresh_token = stored
        .split('"')
        .find(|part| part.starts_with("refresh-"))
        .unwrap();
    assert!(!saved.contains(refresh_token), "{saved}");
    assert!(!saved.contains("access-"), "{saved}");
}

#[test]
fn a_connected_account_is_remembered_through_the_secret_store() {
    let fake = FakeDropbox::start();
    fake.add_folder("/Vaults");
    let (_, secrets) = connected(&fake);

    // keefast starts again: only the secret store knows the account.
    let dropbox = dropbox(&fake, &secrets);

    assert_eq!(dropbox.account().unwrap().unwrap().email, ACCOUNT_EMAIL);
    assert_eq!(names(&dropbox.list_folder("").unwrap()), ["Vaults"]);
    assert_eq!(
        fake.refreshes(),
        1,
        "the refresh token got a new access token"
    );
}

#[test]
fn an_expired_access_token_is_renewed_without_asking_the_user() {
    let fake = FakeDropbox::start();
    fake.add_folder("/Vaults");
    let (dropbox, _) = connected(&fake);

    fake.expire_access_tokens();

    assert_eq!(names(&dropbox.list_folder("").unwrap()), ["Vaults"]);
    assert_eq!(fake.refreshes(), 1);
}

#[test]
fn the_user_refusing_on_dropbox_leaves_keefast_disconnected() {
    let fake = FakeDropbox::start();
    fake.deny_authorization();
    let secrets = Arc::new(FakeSecretStore::default());
    let dropbox = dropbox(&fake, &secrets);

    let authorization = dropbox.begin_connect().unwrap();
    let browser = FakeDropbox::visit_in_browser(authorization.url());
    let result = dropbox.finish_connect(authorization);
    let page = browser.join().unwrap();

    assert!(
        matches!(&result, Err(DropboxError::Refused(reason)) if reason.contains("chose not to")),
        "{result:?}"
    );
    assert!(page.contains("Dropbox was not connected"), "{page}");
    assert_eq!(dropbox.account().unwrap(), None);
    assert!(secrets.is_empty());
}

#[test]
fn connecting_can_be_cancelled_while_waiting_for_the_browser() {
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());
    let dropbox = dropbox(&fake, &secrets);
    let authorization = dropbox.begin_connect().unwrap();

    authorization.canceller().cancel();

    assert!(matches!(
        dropbox.finish_connect(authorization),
        Err(DropboxError::Cancelled)
    ));
    assert!(secrets.is_empty());
}

#[test]
fn connecting_again_right_after_cancelling_gets_the_port_back() {
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());
    let mut config = config(&fake);
    // The registered redirect URI has a fixed port; find one that's free.
    config.redirect_port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dropbox = Arc::new(Dropbox::new(config, secrets));
    let first = dropbox.begin_connect().unwrap();
    first.canceller().cancel();
    let waiting = {
        let dropbox = dropbox.clone();
        std::thread::spawn(move || dropbox.finish_connect(first))
    };

    let second = dropbox.begin_connect().expect("the port is free again");
    let browser = FakeDropbox::visit_in_browser(second.url());
    dropbox.finish_connect(second).unwrap();
    browser.join().unwrap();

    assert!(matches!(
        waiting.join().unwrap(),
        Err(DropboxError::Cancelled)
    ));
}

#[test]
fn a_redirect_without_the_authorizations_state_is_turned_away() {
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());
    let dropbox = dropbox(&fake, &secrets);
    let authorization = dropbox.begin_connect().unwrap();
    let url = url::Url::parse(authorization.url()).unwrap();
    let redirect = url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .unwrap()
        .1
        .into_owned();
    let canceller = authorization.canceller();

    let forged = std::thread::spawn(move || {
        let response = ureq::get(&format!("{redirect}?code=forged&state=guess")).call();
        canceller.cancel();
        response.map(|response| response.status().as_u16())
    });
    let result = dropbox.finish_connect(authorization);

    assert!(matches!(result, Err(DropboxError::Cancelled)), "{result:?}");
    assert!(matches!(
        forged.join().unwrap(),
        Err(ureq::Error::StatusCode(404))
    ));
    assert!(secrets.is_empty());
}

#[test]
fn without_an_app_key_dropbox_cant_be_connected() {
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());
    let mut config = config(&fake);
    config.app_key = String::new();
    let dropbox = Dropbox::new(config, secrets);

    assert!(matches!(
        dropbox.begin_connect(),
        Err(DropboxError::NotConfigured)
    ));
}

#[test]
fn the_whole_dropbox_can_be_browsed_folders_first() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    fake.add_file("/Vaults/Work/work.kdbx", KEEPASSXC_KDBX4);
    fake.add_file("/Vaults/notes.txt", b"hello");
    fake.add_file("/top.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, _) = connected(&fake);

    let top = dropbox.list_folder("").unwrap();
    assert_eq!(names(&top), ["Vaults", "top.kdbx"]);
    assert!(top[0].is_folder);

    let vaults = dropbox.list_folder(&top[0].path).unwrap();
    assert_eq!(
        vaults,
        [
            DropboxItem {
                name: "Work".into(),
                path: "/Vaults/Work".into(),
                is_folder: true,
                size: None,
                modified: None,
            },
            DropboxItem {
                name: "notes.txt".into(),
                path: "/Vaults/notes.txt".into(),
                is_folder: false,
                size: Some(5),
                modified: Some("2026-10-07T23:15:22Z".into()),
            },
            DropboxItem {
                name: "Personal.kdbx".into(),
                path: "/Vaults/Personal.kdbx".into(),
                is_folder: false,
                size: Some(KEEPASSXC_KDBX4.len() as u64),
                modified: Some("2026-10-07T23:15:22Z".into()),
            },
        ]
    );
    assert_eq!(vaults[2].location(), personal());
}

#[test]
fn a_long_folder_is_listed_in_full_across_pages() {
    let fake = FakeDropbox::start();
    for n in 0..7 {
        fake.add_file(&format!("/Many/vault{n}.kdbx"), b"x");
    }
    fake.page_size(3);
    let (dropbox, _) = connected(&fake);

    assert_eq!(dropbox.list_folder("/Many").unwrap().len(), 7);
}

#[test]
fn a_folder_that_is_gone_is_reported() {
    let fake = FakeDropbox::start();
    let (dropbox, _) = connected(&fake);

    assert!(matches!(
        dropbox.list_folder("/Nowhere"),
        Err(DropboxError::NotFound)
    ));
}

#[test]
fn a_vault_file_opens_from_dropbox_and_is_remembered_as_a_dropbox_reference() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, _) = connected(&fake);
    let config = Arc::new(FakeConfigStore::default());
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default()))
        .with_config(config.clone())
        .with_dropbox(dropbox);

    let vault = workspace
        .open_vault(&personal(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    assert_eq!(workspace.vault(vault).unwrap().display_name(), "Personal");
    assert_eq!(workspace.visible_entries(EntryScope::Vault(vault)).len(), 4);
    assert_eq!(config.saved().unwrap().recent_vaults, [personal()]);
}

#[test]
fn file_names_beyond_ascii_reach_dropbox_escaped() {
    let fake = FakeDropbox::start();
    fake.add_file("/Tresor/Persönlich 🔑.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, _) = connected(&fake);
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default())).with_dropbox(dropbox);
    let location = VaultLocation::Dropbox("/Tresor/Persönlich 🔑.kdbx".into());

    let vault = workspace
        .open_vault(&location, &UnlockSecrets::password(PASSWORD))
        .unwrap();

    assert_eq!(
        workspace.vault(vault).unwrap().display_name(),
        "Persönlich 🔑"
    );
    assert!(fake.api_args().iter().all(|arg| arg.is_ascii()));
}

#[test]
fn a_vault_file_missing_from_dropbox_is_not_found() {
    let fake = FakeDropbox::start();
    let (dropbox, _) = connected(&fake);
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default())).with_dropbox(dropbox);

    let result = workspace.open_vault(&personal(), &UnlockSecrets::password(PASSWORD));

    assert!(matches!(
        result,
        Err(OpenVaultError::Storage(StorageError::NotFound(_)))
    ));
}

fn group_names(workspace: &Workspace, vault: VaultId) -> Vec<String> {
    let root = workspace.vault(vault).unwrap().vault().root();
    root.groups()
        .iter()
        .map(|group| group.name().to_owned())
        .collect()
}

#[test]
fn a_change_saved_from_another_device_is_merged_not_overwritten() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, _) = connected(&fake);
    let open = |dropbox: &Arc<Dropbox>| {
        let mut workspace =
            Workspace::new(Arc::new(FakeStorage::default())).with_dropbox(dropbox.clone());
        let vault = workspace
            .open_vault(&personal(), &UnlockSecrets::password(PASSWORD))
            .unwrap();
        (workspace, vault)
    };
    let (mut laptop, on_laptop) = open(&dropbox);
    let (mut phone, on_phone) = open(&dropbox);

    let root = phone.vault(on_phone).unwrap().vault().root().id();
    phone
        .create_group(on_phone, root, "From the phone")
        .unwrap();
    phone.save_vault(on_phone).unwrap();
    let root = laptop.vault(on_laptop).unwrap().vault().root().id();
    laptop
        .create_group(on_laptop, root, "From the laptop")
        .unwrap();
    laptop.save_vault(on_laptop).unwrap();

    let (reopened, vault) = open(&dropbox);
    let groups = group_names(&reopened, vault);
    assert!(groups.contains(&"From the phone".to_owned()), "{groups:?}");
    assert!(groups.contains(&"From the laptop".to_owned()), "{groups:?}");
}

/// A Dropbox Vault open with its local cache, as the app keeps it.
fn opened_with_cache(dropbox: &Arc<Dropbox>) -> (Workspace, VaultId, Arc<FakeVaultCache>) {
    let cache = Arc::new(FakeVaultCache::default());
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default()))
        .with_dropbox(dropbox.clone())
        .with_vault_cache(cache.clone());
    let vault = workspace
        .open_vault(&personal(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    (workspace, vault, cache)
}

fn add_group(workspace: &mut Workspace, vault: VaultId, name: &str) {
    let root = workspace.vault(vault).unwrap().vault().root().id();
    workspace.create_group(vault, root, name).unwrap();
}

/// Another device adds a Group to the Vault file in Dropbox.
fn add_group_elsewhere(fake: &FakeDropbox, name: &str) {
    let key = keefast_core::CompositeKey::password(PASSWORD);
    let mut theirs =
        keefast_core::Vault::open(&fake.file("/Vaults/Personal.kdbx").unwrap(), &key).unwrap();
    let root = theirs.root().id();
    theirs
        .create_group(
            root,
            name,
            keefast_core::Timestamp::from_unix_seconds(2_000_000_000),
        )
        .unwrap();
    fake.add_file("/Vaults/Personal.kdbx", &theirs.save().unwrap());
}

fn sync(workspace: &mut Workspace) {
    for sync in workspace.sync_now() {
        let _ = workspace.finish_save(sync.run());
    }
}

/// A save while Dropbox doesn't let keefast in waits in the local cache, the Vault showing
/// Offline, until the account is connected again: then it is Merged and uploaded.
fn saves_wait_in_the_cache_until_connected_again(lose_access: impl FnOnce(&FakeDropbox, &Dropbox)) {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, _) = connected(&fake);
    let (mut workspace, vault, cache) = opened_with_cache(&dropbox);
    lose_access(&fake, &dropbox);

    add_group(&mut workspace, vault, "From the laptop");
    workspace.save_vault(vault).unwrap();

    let open = workspace.vault(vault).unwrap();
    assert_eq!(open.sync_status(), Some(SyncStatus::Offline));
    assert!(!open.save_failed());
    assert!(!open.has_unsaved_changes());
    assert!(!cache.cached(&personal()).unwrap().uploaded);
    assert!(!fake_has_group(&fake, "From the laptop"));
    // Checking again while still out changes nothing.
    sync(&mut workspace);
    assert_eq!(
        workspace.vault(vault).unwrap().sync_status(),
        Some(SyncStatus::Offline)
    );
    assert!(!workspace.vault(vault).unwrap().save_failed());

    add_group_elsewhere(&fake, "From the phone");
    connect(&dropbox);
    sync(&mut workspace);

    assert_eq!(
        workspace.vault(vault).unwrap().sync_status(),
        Some(SyncStatus::Synced)
    );
    assert!(cache.cached(&personal()).unwrap().uploaded);
    assert!(fake_has_group(&fake, "From the laptop"));
    assert!(fake_has_group(&fake, "From the phone"));
    assert!(group_names(&workspace, vault).contains(&"From the phone".to_owned()));
}

fn fake_has_group(fake: &FakeDropbox, name: &str) -> bool {
    let key = keefast_core::CompositeKey::password(PASSWORD);
    let vault =
        keefast_core::Vault::open(&fake.file("/Vaults/Personal.kdbx").unwrap(), &key).unwrap();
    vault
        .root()
        .groups()
        .iter()
        .any(|group| group.name() == name)
}

#[test]
fn a_save_after_dropbox_revokes_keefast_waits_in_the_cache_until_connected_again() {
    saves_wait_in_the_cache_until_connected_again(|fake, _| fake.revoke_access());
}

#[test]
fn a_save_after_disconnecting_dropbox_waits_in_the_cache_until_connected_again() {
    saves_wait_in_the_cache_until_connected_again(|_, dropbox| dropbox.disconnect().unwrap());
}

fn new_vault() -> NewVault {
    NewVault {
        name: "Personal".into(),
        secrets: UnlockSecrets::password(PASSWORD),
        encryption: Encryption {
            cipher: Cipher::ChaCha20,
            kdf: Kdf::Argon2id(Argon2 {
                memory: 1024 * 1024,
                iterations: 2,
                parallelism: 2,
            }),
        },
    }
}

#[test]
fn a_vault_created_in_dropbox_is_added_never_overwriting() {
    let fake = FakeDropbox::start();
    fake.add_folder("/Vaults");
    let (dropbox, _) = connected(&fake);
    let mut workspace =
        Workspace::new(Arc::new(FakeStorage::default())).with_dropbox(dropbox.clone());

    let pending = workspace.begin_create(personal(), new_vault()).unwrap();
    let vault = workspace.add_unlocked(pending.create().unwrap()).unwrap();

    assert!(workspace.vault(vault).is_some());
    let written = fake.file("/Vaults/Personal.kdbx").unwrap();
    assert!(matches!(
        dropbox.create(&personal(), b"another"),
        Err(StorageError::Exists(_))
    ));
    assert_eq!(fake.file("/Vaults/Personal.kdbx").unwrap(), written);
}

#[test]
fn a_file_appearing_in_dropbox_while_a_vault_is_created_is_never_overwritten() {
    let fake = Arc::new(FakeDropbox::start());
    fake.add_folder("/Vaults");
    let (dropbox, _) = connected(&fake);
    // Another device uploads a file there just after keefast checked that none was.
    let elsewhere = fake.clone();
    let storage = AppearsAfterCheck::new(dropbox, move || {
        elsewhere.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    });
    let workspace =
        Workspace::new(Arc::new(FakeStorage::default())).with_dropbox_storage(Arc::new(storage));
    let pending = workspace.begin_create(personal(), new_vault()).unwrap();

    let result = pending.create();

    assert!(matches!(result, Err(CreateVaultError::FileExists)));
    assert_eq!(fake.file("/Vaults/Personal.kdbx").unwrap(), KEEPASSXC_KDBX4);
}

#[test]
fn disconnecting_revokes_and_removes_the_tokens() {
    let fake = FakeDropbox::start();
    fake.add_folder("/Vaults");
    let (dropbox, secrets) = connected(&fake);

    dropbox.disconnect().unwrap();

    assert_eq!(fake.revocations(), 1);
    assert!(secrets.is_empty());
    assert_eq!(dropbox.account().unwrap(), None);
    assert!(matches!(
        dropbox.list_folder(""),
        Err(DropboxError::NotConnected)
    ));
}

#[test]
fn disconnecting_offline_still_removes_the_tokens() {
    let fake = FakeDropbox::start();
    let (dropbox, secrets) = connected(&fake);
    let mut offline = config(&fake);
    offline.api_url = "http://127.0.0.1:9".into();
    let offline = Dropbox::new(offline, secrets.clone());

    offline.disconnect().unwrap();

    assert!(secrets.is_empty());
    drop(dropbox);
}

#[test]
fn a_secret_store_that_fails_is_reported() {
    let fake = FakeDropbox::start();
    let secrets = Arc::new(FakeSecretStore::default());
    secrets.fail(true);
    let dropbox = dropbox(&fake, &secrets);

    assert!(matches!(
        dropbox.account(),
        Err(DropboxError::SecretStore(_))
    ));
}

#[test]
fn without_dropbox_a_dropbox_vault_file_is_unavailable() {
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default()));

    let result = workspace.open_vault(&personal(), &UnlockSecrets::password(PASSWORD));

    assert!(matches!(
        result,
        Err(OpenVaultError::Storage(StorageError::Unavailable(_)))
    ));
}

#[test]
fn a_revision_is_fetched_without_downloading_the_vault_file() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    fake.add_folder("/Vaults/Work");
    let (dropbox, _) = connected(&fake);

    let revision = dropbox.revision(&personal()).unwrap();

    assert_eq!(revision, dropbox.read(&personal()).unwrap().revision);
    assert_eq!(fake.downloads(), 1, "only the read downloaded it");
    assert!(matches!(
        dropbox.revision(&VaultLocation::Dropbox("/Vaults/Work".into())),
        Err(StorageError::NotFound(_))
    ));
    assert!(matches!(
        dropbox.revision(&VaultLocation::Dropbox("/Vaults/Gone.kdbx".into())),
        Err(StorageError::NotFound(_))
    ));
}

#[test]
fn checking_a_dropbox_vault_downloads_it_only_once_changed() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, _) = connected(&fake);
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default())).with_dropbox(dropbox);
    let vault = workspace
        .open_vault(&personal(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    let sync = |workspace: &mut Workspace| {
        for sync in workspace.sync_now() {
            workspace.finish_save(sync.run()).unwrap();
        }
    };

    sync(&mut workspace);
    assert_eq!(fake.downloads(), 1);

    let mut theirs = keefast_core::Vault::open(
        KEEPASSXC_KDBX4,
        &keefast_core::CompositeKey::password(PASSWORD),
    )
    .unwrap();
    let root = theirs.root().id();
    theirs
        .create_group(
            root,
            "From the phone",
            keefast_core::Timestamp::from_unix_seconds(2_000_000_000),
        )
        .unwrap();
    fake.add_file("/Vaults/Personal.kdbx", &theirs.save().unwrap());
    sync(&mut workspace);

    assert_eq!(fake.downloads(), 2);
    assert!(group_names(&workspace, vault).contains(&"From the phone".to_owned()));
    assert_eq!(
        workspace.vault(vault).unwrap().sync_status(),
        Some(SyncStatus::Synced)
    );
}

#[test]
fn dropbox_out_of_reach_opens_the_cached_vault_file_offline() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", KEEPASSXC_KDBX4);
    let (dropbox, secrets) = connected(&fake);
    let cache = Arc::new(FakeVaultCache::default());
    let mut online = Workspace::new(Arc::new(FakeStorage::default()))
        .with_dropbox(dropbox)
        .with_vault_cache(cache.clone());
    online
        .open_vault(&personal(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    // keefast starts again without a network.
    let mut unreachable = config(&fake);
    unreachable.api_url = "http://127.0.0.1:9".into();
    unreachable.content_url = "http://127.0.0.1:9".into();
    let offline = Arc::new(Dropbox::new(unreachable, secrets));
    assert!(matches!(
        offline.read(&personal()),
        Err(StorageError::Offline(_))
    ));
    let mut workspace = Workspace::new(Arc::new(FakeStorage::default()))
        .with_dropbox(offline)
        .with_vault_cache(cache);
    let vault = workspace
        .open_vault(&personal(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    assert_eq!(workspace.visible_entries(EntryScope::Vault(vault)).len(), 4);
    assert_eq!(
        workspace.vault(vault).unwrap().sync_status(),
        Some(SyncStatus::Offline)
    );
}
