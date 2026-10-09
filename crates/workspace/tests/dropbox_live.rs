//! The real Dropbox, against a sandbox account. Ignored by default: it needs a Dropbox app key, a
//! person to click "Allow" in their browser, and the network. Run it with
//!
//! ```sh
//! KEEFAST_DROPBOX_TEST_APP_KEY=<app key> \
//!   cargo test -p keefast-workspace --test dropbox_live -- --ignored --nocapture
//! ```
//!
//! and open the printed link. `the_os_secret_store_keeps_and_removes_a_secret` checks the real OS
//! secret store on its own. The Dropbox app must have `http://localhost:47826/dropbox` as a
//! redirect URI. The test uploads a throwaway Vault file to `/keefast-live-test/` in the account
//! and leaves it there; delete that folder afterwards.

mod support;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use keefast_workspace::{
    Dropbox, DropboxConfig, OsSecretStore, SecretStore, StorageProvider, UnlockSecrets,
    VaultLocation, Workspace,
};
use secrecy::{ExposeSecret, SecretString};
use support::{FakeSecretStore, FakeStorage, KEEPASSXC_KDBX4, PASSWORD};

#[test]
#[ignore = "needs a Dropbox sandbox account and a person to click Allow"]
fn connect_browse_open_save_and_disconnect_with_the_real_dropbox() {
    let app_key = std::env::var("KEEFAST_DROPBOX_TEST_APP_KEY")
        .expect("set KEEFAST_DROPBOX_TEST_APP_KEY to the Dropbox app key");
    let secrets = Arc::new(FakeSecretStore::default());
    let dropbox = Arc::new(Dropbox::new(DropboxConfig::new(app_key), secrets.clone()));

    let authorization = dropbox.begin_connect().unwrap();
    println!("Open this link and click Allow:\n{}", authorization.url());
    let account = dropbox.finish_connect(authorization).unwrap();
    println!("Connected as {} <{}>", account.name, account.email);
    assert!(secrets.secret("dropbox").is_some());

    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let path = format!("/keefast-live-test/live-{seconds}.kdbx");
    let location = VaultLocation::Dropbox(path.clone());
    dropbox.write(&location, KEEPASSXC_KDBX4, None).unwrap();
    let listed = dropbox.list_folder("/keefast-live-test").unwrap();
    assert!(listed.iter().any(|item| item.path == path), "{listed:?}");

    let mut workspace =
        Workspace::new(Arc::new(FakeStorage::default())).with_dropbox(dropbox.clone());
    let vault = workspace
        .open_vault(&location, &UnlockSecrets::password(PASSWORD))
        .unwrap();
    let root = workspace.vault(vault).unwrap().vault().root().id();
    workspace.create_group(vault, root, "Saved live").unwrap();
    workspace.save_vault(vault).unwrap();
    // A background check asks for the revision only, which matches what the save uploaded.
    assert_eq!(
        dropbox.revision(&location).unwrap(),
        dropbox.read(&location).unwrap().revision
    );
    for sync in workspace.sync_now() {
        workspace.finish_save(sync.run()).unwrap();
    }
    assert_eq!(
        workspace.vault(vault).unwrap().sync_status(),
        Some(keefast_workspace::SyncStatus::Synced)
    );

    dropbox.disconnect().unwrap();
    assert!(secrets.is_empty());
    println!("Left {path} in the Dropbox; delete /keefast-live-test when done.");
}

/// The OS secret store keeps, returns and removes a secret. Ignored by default: it needs a
/// Secret Service (or Windows Credential Manager) and writes to the user's keyring, under the
/// name `keefast-live-test` so keefast's own Dropbox tokens are left alone.
#[test]
#[ignore = "writes to the OS secret store"]
fn the_os_secret_store_keeps_and_removes_a_secret() {
    let store = OsSecretStore;
    let name = "keefast-live-test";

    store.set(name, &SecretString::from("a token")).unwrap();
    let kept = store.get(name).unwrap().expect("the secret is kept");
    assert_eq!(kept.expose_secret(), "a token");

    store.delete(name).unwrap();
    assert!(store.get(name).unwrap().is_none());
    store
        .delete(name)
        .expect("removing what isn't there succeeds");
}
