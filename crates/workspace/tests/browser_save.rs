//! Saving captured logins over the KeePassXC-Browser protocol: `set-login` driven with real
//! NaCl-encrypted messages from a fake extension through a Workspace. keefast asks the user
//! where the login goes; the extension never decides.
//!
//! The Vaults are KeePassXC's browser fixture, which has no Browser group yet. For
//! https://example.com it holds "Example" (alice / example-secret) among others.

mod support;

use std::sync::Arc;

use keefast_core::{DEFAULT_BROWSER_GROUP, EditError, EntryId, GroupId};
use keefast_workspace::{
    AppConfig, ConnectionId, EditVaultError, LoginTarget, Outgoing, PendingLogin, UnlockSecrets,
    VaultId, VaultLocation, Workspace,
};
use secrecy::ExposeSecret;
use serde_json::Value;
use support::extension::{FakeExtension, Sent};
use support::{BROWSER_KDBX, FakeConfigStore, FakeStorage, PASSWORD, personal_vault, work_vault};

const FIRST: ConnectionId = ConnectionId(1);
const SHOP: &str = "https://shop.test/sign-in";
const EXAMPLE: &str = "https://example.com/login";

fn parse(reply: &str) -> Value {
    serde_json::from_str(reply).unwrap()
}

/// An extension connected over `FIRST` to a Workspace with browser integration on.
struct Browser {
    workspace: Workspace,
    extension: FakeExtension,
    config: Arc<FakeConfigStore>,
}

impl Browser {
    /// Opens the personal and work Vaults, pairs an extension as "Home" with those in
    /// `paired_with`, and has it prove the Pairing, as the stock extension does on connecting.
    fn new(paired_with: &[VaultLocation]) -> (Self, Vec<VaultId>) {
        Self::with_config(paired_with, AppConfig::default())
    }

    fn with_config(paired_with: &[VaultLocation], config: AppConfig) -> (Self, Vec<VaultId>) {
        let storage = Arc::new(
            FakeStorage::default()
                .with_file(&personal_vault(), BROWSER_KDBX)
                .with_file(&work_vault(), BROWSER_KDBX),
        );
        let config = Arc::new(FakeConfigStore::holding(config));
        let mut workspace = Workspace::new(storage).with_config(config.clone());
        let key = UnlockSecrets::password(PASSWORD);
        let vaults = vec![
            workspace.open_vault(&personal_vault(), &key).unwrap(),
            workspace.open_vault(&work_vault(), &key).unwrap(),
        ];
        workspace.set_browser_integration(true);
        let mut extension = FakeExtension::new();
        let keys = workspace
            .handle_browser_request(FIRST, &extension.change_public_keys().json)
            .unwrap();
        extension.accept_keys(&parse(&keys));
        for location in paired_with {
            let vault = workspace.open_vault_at(location).unwrap();
            workspace
                .pair_browser(vault, "Home", &extension.id_key())
                .unwrap();
        }
        let mut browser = Self {
            workspace,
            extension,
            config,
        };
        if !paired_with.is_empty() {
            let sent = browser
                .extension
                .test_associate("Home", &browser.extension.id_key());
            let reply = browser.request(&sent).unwrap();
            browser.extension.open(&sent, &reply);
        }
        (browser, vaults)
    }

    fn request(&mut self, sent: &Sent) -> Option<String> {
        self.workspace.handle_browser_request(FIRST, &sent.json)
    }

    /// Sends the request and expects keefast to ask the user where the login goes.
    #[track_caller]
    fn ask(&mut self, sent: &Sent) -> PendingLogin {
        assert_eq!(self.request(sent), None, "answered without asking");
        self.workspace.pending_login().expect("asks the user")
    }

    /// Saves the pending login to `target`, and returns the reply sent for it.
    #[track_caller]
    fn save(&mut self, sent: &Sent, target: LoginTarget) -> Value {
        let pending = self.workspace.pending_login().unwrap();
        let reply = self.workspace.save_login(pending.id, target).unwrap();
        let Outgoing {
            connection,
            message,
        } = reply.expect("a reply for the extension");
        assert_eq!(connection, FIRST);
        self.extension.open(sent, &message)
    }

    fn entry_titled(&self, vault: VaultId, title: &str) -> EntryId {
        let open = self.workspace.vault(vault).unwrap().vault();
        open.root()
            .all_entries()
            .find(|entry| entry.title() == title)
            .unwrap_or_else(|| panic!("no Entry {title:?}"))
            .id()
    }

    fn top_group(&self, vault: VaultId, name: &str) -> Option<GroupId> {
        let open = self.workspace.vault(vault).unwrap().vault();
        let mut groups = open.root().groups().iter();
        groups
            .find(|group| group.name() == name)
            .map(|group| group.id())
    }
}

#[test]
fn a_new_login_asks_the_user_and_preselects_the_browser_group_of_the_vault() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);

    let pending = browser.ask(&sent);

    assert_eq!(pending.site, "shop.test");
    assert_eq!(pending.username, "carol");
    // Only the paired Vault is offered, and nothing to update.
    assert_eq!(pending.vaults, [vaults[0]]);
    assert_eq!(pending.entries, []);
    assert_eq!(
        pending.suggested,
        LoginTarget::NewEntry {
            vault: vaults[0],
            group: None,
        }
    );
    assert_eq!(browser.workspace.browser_group(vaults[0]), None);
}

#[test]
fn a_saved_new_login_goes_to_the_browser_group_made_for_it() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    let pending = browser.ask(&sent);

    let message = browser.save(&sent, pending.suggested);

    assert_eq!(message["action"], "set-login");
    assert_eq!(message["error"], "success");
    assert_eq!(message["count"], Value::Null);
    assert_eq!(message["entries"], Value::Null);
    let open = browser.workspace.vault(vaults[0]).unwrap();
    assert_eq!(message["hash"], open.vault().browser_hash());
    assert!(open.has_unsaved_changes());
    let group = browser.top_group(vaults[0], DEFAULT_BROWSER_GROUP).unwrap();
    assert_eq!(browser.workspace.browser_group(vaults[0]), Some(group));
    let saved = &browser.workspace.group(vaults[0], group).unwrap().entries()[0];
    assert_eq!(saved.title(), "shop.test");
    assert_eq!(saved.url(), SHOP);
    assert_eq!(saved.username(), "carol");
    assert_eq!(saved.password().expose_secret(), "s3cret");
    assert!(browser.workspace.pending_login().is_none());
}

#[test]
fn the_group_the_extension_asks_for_is_ignored() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let other = browser.top_group(vaults[0], "Hidden group").unwrap();
    let wanted = format!("{other:?}");
    let sent = browser.extension.set_login(
        SHOP,
        "carol",
        "s3cret",
        None,
        Some(("Hidden group", &wanted)),
    );

    let pending = browser.ask(&sent);

    assert_eq!(
        pending.suggested,
        LoginTarget::NewEntry {
            vault: vaults[0],
            group: None,
        }
    );
}

#[test]
fn the_browser_group_chosen_for_a_vault_is_preselected() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let chosen = browser.top_group(vaults[0], "Hidden group").unwrap();
    browser
        .workspace
        .set_browser_group(vaults[0], chosen)
        .unwrap();
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);

    let pending = browser.ask(&sent);
    assert_eq!(
        pending.suggested,
        LoginTarget::NewEntry {
            vault: vaults[0],
            group: Some(chosen),
        }
    );
    browser.save(&sent, pending.suggested);

    let entries = browser
        .workspace
        .group(vaults[0], chosen)
        .unwrap()
        .entries();
    assert!(entries.iter().any(|entry| entry.username() == "carol"));
    assert_eq!(browser.top_group(vaults[0], DEFAULT_BROWSER_GROUP), None);
}

#[test]
fn the_user_can_save_a_new_login_to_another_group() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    browser.ask(&sent);
    let root = browser
        .workspace
        .vault(vaults[0])
        .unwrap()
        .vault()
        .root()
        .id();

    browser.save(
        &sent,
        LoginTarget::NewEntry {
            vault: vaults[0],
            group: Some(root),
        },
    );

    let root = browser.workspace.group(vaults[0], root).unwrap();
    assert!(
        root.entries()
            .iter()
            .any(|entry| entry.username() == "carol")
    );
    assert_eq!(browser.top_group(vaults[0], DEFAULT_BROWSER_GROUP), None);
}

#[test]
fn a_changed_password_suggests_updating_the_entry_with_that_username() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let example = browser.entry_titled(vaults[0], "Example");
    let sent = browser
        .extension
        .set_login(EXAMPLE, "alice", "new-secret", None, None);

    let pending = browser.ask(&sent);

    assert_eq!(pending.entries, [(vaults[0], example)]);
    assert_eq!(
        pending.suggested,
        LoginTarget::Update {
            vault: vaults[0],
            entry: example,
        }
    );
}

#[test]
fn updating_an_entry_keeps_the_old_version_in_its_history() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let example = browser.entry_titled(vaults[0], "Example");
    let history = browser
        .workspace
        .entry(vaults[0], example)
        .unwrap()
        .history()
        .len();
    let sent = browser
        .extension
        .set_login(EXAMPLE, "alice", "new-secret", None, None);
    let pending = browser.ask(&sent);

    let message = browser.save(&sent, pending.suggested);

    assert_eq!(message["error"], "success");
    let entry = browser.workspace.entry(vaults[0], example).unwrap();
    assert_eq!(entry.title(), "Example");
    assert_eq!(entry.password().expose_secret(), "new-secret");
    assert_eq!(entry.history().len(), history + 1);
    let old = entry.history().last().unwrap();
    assert_eq!(old.password().expose_secret(), "example-secret");
    // No duplicate: the Browser group was never needed.
    assert_eq!(browser.top_group(vaults[0], DEFAULT_BROWSER_GROUP), None);
}

#[test]
fn the_entry_the_extension_picked_to_update_is_suggested() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let example = browser.entry_titled(vaults[0], "Example");
    let uuid = example.to_hex();
    // A new username for the Entry: no Entry has it, but the extension picked "Example".
    let sent = browser
        .extension
        .set_login(EXAMPLE, "alice2", "new-secret", Some(&uuid), None);

    let pending = browser.ask(&sent);

    assert_eq!(pending.entries, [(vaults[0], example)]);
    assert_eq!(
        pending.suggested,
        LoginTarget::Update {
            vault: vaults[0],
            entry: example,
        }
    );
    browser.save(&sent, pending.suggested);
    let entry = browser.workspace.entry(vaults[0], example).unwrap();
    assert_eq!(entry.username(), "alice2");
}

#[test]
fn the_user_can_save_a_changed_login_as_a_new_entry_instead() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let example = browser.entry_titled(vaults[0], "Example");
    let sent = browser
        .extension
        .set_login(EXAMPLE, "alice", "new-secret", None, None);
    browser.ask(&sent);

    browser.save(
        &sent,
        LoginTarget::NewEntry {
            vault: vaults[0],
            group: None,
        },
    );

    let entry = browser.workspace.entry(vaults[0], example).unwrap();
    assert_eq!(entry.password().expose_secret(), "example-secret");
    let group = browser.top_group(vaults[0], DEFAULT_BROWSER_GROUP).unwrap();
    assert_eq!(
        browser
            .workspace
            .group(vaults[0], group)
            .unwrap()
            .entries()
            .len(),
        1
    );
}

#[test]
fn cancelling_answers_that_the_action_was_cancelled_and_changes_nothing() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    let pending = browser.ask(&sent);

    let reply = browser.workspace.cancel_login(pending.id).unwrap();

    assert_eq!(reply.connection, FIRST);
    let reply = parse(&reply.message);
    assert_eq!(reply["action"], "set-login");
    assert_eq!(reply["errorCode"], "6");
    assert!(browser.workspace.pending_login().is_none());
    assert_eq!(browser.top_group(vaults[0], DEFAULT_BROWSER_GROUP), None);
}

#[test]
fn the_last_chosen_vault_is_preselected_next_time_and_remembered_between_runs() {
    let (mut browser, vaults) = Browser::new(&[personal_vault(), work_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    let pending = browser.ask(&sent);
    assert_eq!(pending.vaults, vaults);
    assert_eq!(
        pending.suggested,
        LoginTarget::NewEntry {
            vault: vaults[0],
            group: None,
        }
    );

    browser.save(
        &sent,
        LoginTarget::NewEntry {
            vault: vaults[1],
            group: None,
        },
    );
    let sent = browser
        .extension
        .set_login(SHOP, "dave", "other", None, None);
    let pending = browser.ask(&sent);
    assert_eq!(
        pending.suggested,
        LoginTarget::NewEntry {
            vault: vaults[1],
            group: browser.workspace.browser_group(vaults[1]),
        }
    );

    // A later run, with the same app config, starts from that Vault too.
    let saved = browser.config.saved().unwrap();
    assert_eq!(saved.last_login_vault, Some(work_vault()));
    let (mut browser, vaults) = Browser::with_config(&[personal_vault(), work_vault()], saved);
    let sent = browser
        .extension
        .set_login(SHOP, "erin", "third", None, None);
    let pending = browser.ask(&sent);
    assert_eq!(
        pending.suggested,
        LoginTarget::NewEntry {
            vault: vaults[1],
            group: None,
        }
    );
}

#[test]
fn a_login_cannot_go_to_a_vault_the_browser_is_not_paired_with() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    let pending = browser.ask(&sent);

    let refused = browser.workspace.save_login(
        pending.id,
        LoginTarget::NewEntry {
            vault: vaults[1],
            group: None,
        },
    );

    assert_eq!(refused, Err(EditVaultError::NotPaired));
    assert!(browser.workspace.pending_login().is_some());
    assert!(
        !browser
            .workspace
            .vault(vaults[1])
            .unwrap()
            .has_unsaved_changes()
    );
}

#[test]
fn an_extension_that_has_not_proven_a_pairing_cannot_save_logins() {
    let (mut browser, _) = Browser::new(&[]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);

    let reply = parse(&browser.request(&sent).unwrap());

    assert_eq!(reply["errorCode"], "8");
    assert!(browser.workspace.pending_login().is_none());
}

#[test]
fn a_login_without_a_url_is_refused() {
    let (mut browser, _) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login("", "carol", "s3cret", None, None);

    let reply = parse(&browser.request(&sent).unwrap());

    assert_eq!(reply["errorCode"], "14");
}

#[test]
fn an_extension_that_goes_away_takes_its_login_with_it() {
    let (mut browser, _) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    browser.ask(&sent);

    browser.workspace.disconnect_browser(FIRST);

    assert!(browser.workspace.pending_login().is_none());
}

#[test]
fn turning_integration_off_cancels_the_pending_login() {
    let (mut browser, _) = Browser::new(&[personal_vault()]);
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    browser.ask(&sent);

    let replies = browser.workspace.set_browser_integration(false);

    assert_eq!(replies.len(), 1);
    assert_eq!(parse(&replies[0].message)["errorCode"], "6");
    assert!(browser.workspace.pending_login().is_none());
}

#[test]
fn the_last_chosen_vault_survives_in_the_config_file() {
    use keefast_workspace::{ConfigStore, FileConfigStore};

    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("config.toml"));
    store
        .save(&AppConfig {
            last_login_vault: Some(work_vault()),
            ..AppConfig::default()
        })
        .unwrap();

    assert_eq!(store.load().unwrap().last_login_vault, Some(work_vault()));
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("[last_login_vault]"), "{text}");
}

#[test]
fn only_an_entry_offered_to_the_site_can_be_picked_for_update() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    // "Other site" is for other.test, and "Hidden" is hidden from browsers.
    for title in ["Other site", "Hidden"] {
        let uuid = browser.entry_titled(vaults[0], title).to_hex();
        let sent = browser
            .extension
            .set_login(EXAMPLE, "mallory", "x", Some(&uuid), None);

        let pending = browser.ask(&sent);

        assert_eq!(pending.entries, [], "{title} offered");
        browser.workspace.cancel_login(pending.id);
    }
}

#[test]
fn an_update_without_a_username_keeps_the_entrys() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let example = browser.entry_titled(vaults[0], "Example");
    let uuid = example.to_hex();
    let sent = browser
        .extension
        .set_login(EXAMPLE, "", "new-secret", Some(&uuid), None);
    let pending = browser.ask(&sent);

    browser.save(&sent, pending.suggested);

    let entry = browser.workspace.entry(vaults[0], example).unwrap();
    assert_eq!(entry.username(), "alice");
    assert_eq!(entry.password().expose_secret(), "new-secret");
}

#[test]
fn a_login_cannot_go_to_the_recycle_bin() {
    let (mut browser, vaults) = Browser::new(&[personal_vault()]);
    let bin = browser.top_group(vaults[0], "Recycle Bin").unwrap();
    let sent = browser
        .extension
        .set_login(SHOP, "carol", "s3cret", None, None);
    let pending = browser.ask(&sent);

    let refused = browser.workspace.save_login(
        pending.id,
        LoginTarget::NewEntry {
            vault: vaults[0],
            group: Some(bin),
        },
    );

    assert_eq!(refused, Err(EditVaultError::Edit(EditError::InRecycleBin)));
    assert_eq!(
        browser.workspace.set_browser_group(vaults[0], bin),
        Err(EditVaultError::Edit(EditError::InRecycleBin))
    );
    assert!(browser.workspace.pending_login().is_some());
}
