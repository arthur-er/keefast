//! The KeePassXC-Browser protocol, driven with real NaCl-encrypted messages from a fake
//! extension through a Workspace with faked storage: key exchange, Pairing and the locked
//! error.

mod support;

use std::sync::Arc;

use keefast_core::EditError;
use keefast_workspace::{
    BrowserReply, BrowserSession, EditVaultError, EntryScope, KEEPASSXC_VERSION, PairingRequest,
    UnlockSecrets, VaultId, View, Workspace,
};
use serde_json::{Value, json};
use support::extension::{FakeExtension, Sent};
use support::{FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault, work_vault};

/// The fixture Vault's browser hash: SHA-256 of its root Group's UUID in hex.
const FIXTURE_HASH: &str = "e664e0d1a663bc5bf6293d1955671b5b1c87d5d0cef01daf1fdbe87e477ed419";

fn storage() -> Arc<FakeStorage> {
    Arc::new(
        FakeStorage::default()
            .with_file(&personal_vault(), KEEPASSXC_KDBX4)
            .with_file(&work_vault(), KEEPASSXC_KDBX4),
    )
}

fn workspace_with_personal_vault() -> (Workspace, VaultId) {
    let mut workspace = Workspace::new(storage());
    let vault = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    (workspace, vault)
}

fn send(session: &mut BrowserSession, workspace: &Workspace, sent: &Sent) -> String {
    match session.handle(workspace, &sent.json) {
        BrowserReply::Send(reply) => reply,
        other => panic!("unexpected request to the user: {other:?}"),
    }
}

fn ask_pairing(session: &mut BrowserSession, workspace: &Workspace, sent: &Sent) -> PairingRequest {
    match session.handle(workspace, &sent.json) {
        BrowserReply::AskPairing(request) => request,
        other => panic!("expected a Pairing request, got {other:?}"),
    }
}

fn parse(reply: &str) -> Value {
    serde_json::from_str(reply).unwrap()
}

/// An extension that has exchanged keys with a session.
fn connected(workspace: &Workspace) -> (FakeExtension, BrowserSession) {
    let mut extension = FakeExtension::new();
    let mut session = BrowserSession::new();
    let reply = parse(&send(
        &mut session,
        workspace,
        &extension.change_public_keys(),
    ));
    extension.accept_keys(&reply);
    (extension, session)
}

/// Pairs the extension with the Vault the browser deals with, under `name`.
fn pair(
    workspace: &mut Workspace,
    extension: &FakeExtension,
    session: &mut BrowserSession,
    name: &str,
) {
    let associate = extension.associate();
    let request = ask_pairing(session, workspace, &associate);
    let reply = session.approve_pairing(workspace, &request, name).unwrap();
    extension.open(&associate, &reply);
}

#[test]
fn the_key_exchange_answers_with_a_host_key_and_the_incremented_nonce() {
    let (workspace, _) = workspace_with_personal_vault();
    let extension = FakeExtension::new();
    let mut session = BrowserSession::new();
    let sent = extension.change_public_keys();

    let reply = parse(&send(&mut session, &workspace, &sent));

    assert_eq!(reply["action"], "change-public-keys");
    assert_eq!(reply["success"], "true");
    assert_eq!(reply["version"], KEEPASSXC_VERSION);
    assert_eq!(reply["nonce"], sent.expected_nonce);
    assert_eq!(reply["requestID"], "r1");
    assert_ne!(reply["publicKey"], extension.public_key());
}

#[test]
fn the_database_hash_identifies_the_vault() {
    let (workspace, _) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    let sent = extension.get_database_hash(&[]);

    let reply = send(&mut session, &workspace, &sent);

    let message = extension.open(&sent, &reply);
    assert_eq!(message["hash"], FIXTURE_HASH);
    assert_eq!(message["version"], KEEPASSXC_VERSION);
    assert_eq!(parse(&reply)["action"], "get-databasehash");
}

#[test]
fn associating_asks_the_user_and_stores_an_approved_pairing_in_the_vault() {
    let (mut workspace, vault) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    let associate = extension.associate();

    let request = ask_pairing(&mut session, &workspace, &associate);
    assert_eq!(request.vault(), vault);
    let reply = session
        .approve_pairing(&mut workspace, &request, " Work laptop ")
        .unwrap();

    let message = extension.open(&associate, &reply);
    assert_eq!(message["id"], "Work laptop");
    assert_eq!(message["hash"], FIXTURE_HASH);
    let open = workspace.vault(vault).unwrap();
    assert!(
        open.vault()
            .is_paired_with("Work laptop", &extension.id_key())
    );
    assert!(open.has_unsaved_changes());
}

#[test]
fn a_pairing_is_saved_with_the_vault_and_recognised_in_a_later_session() {
    let storage = storage();
    let mut workspace = Workspace::new(storage.clone());
    let vault = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    let (extension, mut session) = connected(&workspace);
    pair(&mut workspace, &extension, &mut session, "Home");
    workspace.save_vault(vault).unwrap();

    // keefast restarts; the extension reconnects with a new session key.
    let mut workspace = Workspace::new(storage);
    workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    let mut later = FakeExtension::new();
    let mut session = BrowserSession::new();
    let keys = parse(&send(&mut session, &workspace, &later.change_public_keys()));
    later.accept_keys(&keys);
    let sent = later.test_associate("Home", &extension.id_key());

    let message = later.open(&sent, &send(&mut session, &workspace, &sent));
    assert_eq!(message["id"], "Home");
    assert_eq!(message["hash"], FIXTURE_HASH);
}

#[test]
fn test_associate_fails_for_an_unknown_name_or_key() {
    let (mut workspace, _) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    pair(&mut workspace, &extension, &mut session, "Home");
    let other = FakeExtension::new();

    for (id, key) in [("Work", extension.id_key()), ("Home", other.id_key())] {
        let reply = parse(&send(
            &mut session,
            &workspace,
            &extension.test_associate(id, &key),
        ));
        assert_eq!(reply["errorCode"], "8", "{id:?}");
        assert_eq!(reply["action"], "test-associate");
    }
    // Without a name or key, KeePassXC answers as if no database were open.
    let empty = parse(&send(
        &mut session,
        &workspace,
        &extension.test_associate("", ""),
    ));
    assert_eq!(empty["errorCode"], "1");
}

#[test]
fn a_denied_pairing_is_not_stored() {
    let (workspace, vault) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    let request = ask_pairing(&mut session, &workspace, &extension.associate());

    let reply = parse(&session.deny_pairing(&request));

    assert_eq!(reply["action"], "associate");
    assert_eq!(reply["errorCode"], "6");
    let open = workspace.vault(vault).unwrap();
    assert_eq!(open.vault().pairings().len(), 1, "only the fixture's own");
    assert!(!open.has_unsaved_changes());
}

#[test]
fn a_taken_or_empty_name_is_refused_and_the_request_can_be_answered_again() {
    let (mut workspace, _) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    let associate = extension.associate();
    let request = ask_pairing(&mut session, &workspace, &associate);

    // The fixture Vault was paired with a browser called "fixture-browser" before.
    assert_eq!(
        session
            .approve_pairing(&mut workspace, &request, "fixture-browser")
            .unwrap_err(),
        EditVaultError::Edit(EditError::PairingNameTaken)
    );
    assert_eq!(
        session
            .approve_pairing(&mut workspace, &request, "  ")
            .unwrap_err(),
        EditVaultError::Edit(EditError::PairingNameEmpty)
    );
    let reply = session
        .approve_pairing(&mut workspace, &request, "Laptop")
        .unwrap();

    assert_eq!(extension.open(&associate, &reply)["id"], "Laptop");
}

#[test]
fn associate_must_prove_the_session_key() {
    let (workspace, _) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    let impostor = FakeExtension::new();
    let sent = extension.encrypted(
        "associate",
        json!({ "action": "associate", "key": impostor.public_key(), "idKey": extension.id_key() }),
    );

    let reply = parse(&send(&mut session, &workspace, &sent));

    assert_eq!(reply["errorCode"], "8");
}

#[test]
fn with_no_vault_open_keefast_answers_that_it_is_locked() {
    let workspace = Workspace::new(storage());
    let (extension, mut session) = connected(&workspace);

    for sent in [
        extension.get_database_hash(&[]),
        extension.associate(),
        extension.test_associate("Home", &extension.id_key()),
        extension.get_logins("https://example.com/", None, &[]),
    ] {
        let reply = parse(&send(&mut session, &workspace, &sent));
        assert_eq!(reply["errorCode"], "1", "{reply}");
        assert!(reply["error"].as_str().unwrap().contains("locked"));
        assert!(reply.get("message").is_none());
    }
}

#[test]
fn encrypted_requests_before_the_key_exchange_are_refused() {
    let (workspace, _) = workspace_with_personal_vault();
    let mut session = BrowserSession::new();
    let request = Sent {
        json: json!({
            "action": "get-databasehash",
            "message": "AAAA",
            "nonce": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        })
        .to_string(),
        expected_nonce: String::new(),
    };

    let reply = parse(&send(&mut session, &workspace, &request));
    assert_eq!(reply["errorCode"], "4");

    // Locked as well as without keys, the missing key exchange is what the extension hears.
    let locked = Workspace::new(storage());
    let reply = parse(&send(&mut session, &locked, &request));
    assert_eq!(reply["errorCode"], "3");
}

#[test]
fn a_tampered_message_cannot_be_decrypted() {
    let (workspace, _) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    let sent = extension.get_database_hash(&[]);
    let mut request = parse(&sent.json);
    let message = request["message"].as_str().unwrap();
    let tampered = format!("B{}", &message[1..]);
    request["message"] = tampered.into();

    let reply = parse(&send(
        &mut session,
        &workspace,
        &Sent {
            json: request.to_string(),
            expected_nonce: String::new(),
        },
    ));

    assert_eq!(reply["errorCode"], "4");
}

#[test]
fn unknown_actions_and_garbage_are_refused() {
    let (workspace, _) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);

    let unknown = parse(&send(
        &mut session,
        &workspace,
        &extension.encrypted("fly-to-the-moon", json!({ "action": "fly-to-the-moon" })),
    ));
    let garbage = parse(&send(
        &mut session,
        &workspace,
        &Sent {
            json: "not json".into(),
            expected_nonce: String::new(),
        },
    ));

    assert_eq!(unknown["errorCode"], "12");
    assert_eq!(unknown["action"], "fly-to-the-moon");
    assert_eq!(garbage["errorCode"], "13");
}

#[test]
fn the_browser_deals_with_the_focused_vault() {
    let mut workspace = Workspace::new(storage());
    let key = UnlockSecrets::password(PASSWORD);
    workspace.open_vault(&personal_vault(), &key).unwrap();
    let work = workspace.open_vault(&work_vault(), &key).unwrap();
    workspace.show(View::from(EntryScope::Vault(work)));
    let (extension, mut session) = connected(&workspace);

    let request = ask_pairing(&mut session, &workspace, &extension.associate());
    assert_eq!(request.vault(), work);
    session
        .approve_pairing(&mut workspace, &request, "Work browser")
        .unwrap();

    // Looking at all Vaults, the browser deals with the Vault it is paired with.
    workspace.show(View::from(EntryScope::AllVaults));
    let sent = extension.test_associate("Work browser", &extension.id_key());
    extension.open(&sent, &send(&mut session, &workspace, &sent));
    let personal = workspace.vaults().next().unwrap();
    assert!(
        !personal
            .vault()
            .is_paired_with("Work browser", &extension.id_key())
    );
}

#[test]
fn unfocused_the_browser_deals_with_a_vault_it_holds_a_key_for() {
    let storage = storage();
    let key = UnlockSecrets::password(PASSWORD);
    // Pair the work Vault alone, then open it after the personal one.
    let mut workspace = Workspace::new(storage.clone());
    let work = workspace.open_vault(&work_vault(), &key).unwrap();
    let (extension, mut session) = connected(&workspace);
    pair(&mut workspace, &extension, &mut session, "Work browser");
    workspace.save_vault(work).unwrap();

    let mut workspace = Workspace::new(storage);
    workspace.open_vault(&personal_vault(), &key).unwrap();
    let work = workspace.open_vault(&work_vault(), &key).unwrap();
    let (extension_again, mut session) = connected(&workspace);
    let sent = extension_again.get_database_hash(&["Work browser"]);
    let reply = send(&mut session, &workspace, &sent);

    // Both fixtures share a root Group, so the hash can't tell them apart; the Pairing can.
    assert_eq!(extension_again.open(&sent, &reply)["hash"], FIXTURE_HASH);
    let associate = extension_again.associate();
    let request = ask_pairing(&mut session, &workspace, &associate);
    assert_eq!(request.vault(), work);
}

#[test]
fn approving_for_a_vault_no_longer_open_answers_that_keefast_is_locked() {
    let (workspace, _) = workspace_with_personal_vault();
    let (extension, mut session) = connected(&workspace);
    let request = ask_pairing(&mut session, &workspace, &extension.associate());
    // The answer comes once the Vault is no longer open.
    let mut without_vault = Workspace::new(storage());

    let reply = session
        .approve_pairing(&mut without_vault, &request, "Home")
        .unwrap();

    assert_eq!(parse(&reply)["errorCode"], "1");
}

mod connections {
    //! The same protocol through the Workspace's connections: one session per native host,
    //! and Pairing requests put to the user one at a time.

    use keefast_workspace::{ConnectionId, Outgoing};

    use super::*;

    const FIRST: ConnectionId = ConnectionId(1);
    const SECOND: ConnectionId = ConnectionId(2);

    fn integrated_workspace() -> (Workspace, VaultId) {
        let (mut workspace, vault) = workspace_with_personal_vault();
        workspace.set_browser_integration(true);
        (workspace, vault)
    }

    fn request(workspace: &mut Workspace, connection: ConnectionId, sent: &Sent) -> Option<String> {
        workspace.handle_browser_request(connection, &sent.json)
    }

    /// An extension that has exchanged keys over `connection`.
    fn connect(workspace: &mut Workspace, connection: ConnectionId) -> FakeExtension {
        let mut extension = FakeExtension::new();
        let reply = request(workspace, connection, &extension.change_public_keys()).unwrap();
        extension.accept_keys(&parse(&reply));
        extension
    }

    #[test]
    fn pairing_requests_are_put_to_the_user_one_at_a_time_in_order() {
        let (mut workspace, vault) = integrated_workspace();
        let first = connect(&mut workspace, FIRST);
        let second = connect(&mut workspace, SECOND);
        let first_associate = first.associate();
        let second_associate = second.associate();

        assert_eq!(request(&mut workspace, FIRST, &first_associate), None);
        assert_eq!(request(&mut workspace, SECOND, &second_associate), None);
        let pending = workspace.pending_pairing().unwrap();
        assert_eq!(pending.vault, vault);

        let Outgoing {
            connection,
            message,
        } = workspace
            .approve_pairing(pending.id, "Laptop")
            .unwrap()
            .unwrap();
        assert_eq!(connection, FIRST);
        assert_eq!(first.open(&first_associate, &message)["id"], "Laptop");

        let next = workspace.pending_pairing().unwrap();
        assert_ne!(next.id, pending.id);
        let denied = workspace.deny_pairing(next.id).unwrap();
        assert_eq!(denied.connection, SECOND);
        assert_eq!(parse(&denied.message)["errorCode"], "6");
        assert_eq!(workspace.pending_pairing(), None);
    }

    #[test]
    fn a_refused_name_keeps_the_request_pending() {
        let (mut workspace, _) = integrated_workspace();
        let extension = connect(&mut workspace, FIRST);
        request(&mut workspace, FIRST, &extension.associate());
        let pending = workspace.pending_pairing().unwrap();

        assert!(
            workspace
                .approve_pairing(pending.id, "fixture-browser")
                .is_err()
        );

        assert_eq!(workspace.pending_pairing(), Some(pending));
    }

    #[test]
    fn an_extension_that_goes_away_takes_its_pairing_request_with_it() {
        let (mut workspace, _) = integrated_workspace();
        let extension = connect(&mut workspace, FIRST);
        request(&mut workspace, FIRST, &extension.associate());
        let pending = workspace.pending_pairing().unwrap();

        workspace.disconnect_browser(FIRST);

        assert_eq!(workspace.pending_pairing(), None);
        assert_eq!(
            workspace.approve_pairing(pending.id, "Laptop").unwrap(),
            None
        );
    }

    #[test]
    fn with_integration_off_requests_are_answered_that_keefast_is_not_running() {
        let (mut workspace, _) = workspace_with_personal_vault();
        let extension = FakeExtension::new();

        let reply = request(&mut workspace, FIRST, &extension.change_public_keys()).unwrap();

        let reply = parse(&reply);
        assert_eq!(reply["errorCode"], "5");
        assert_eq!(reply["action"], "change-public-keys");
        assert_eq!(reply["requestID"], "r1");
    }

    #[test]
    fn turning_integration_off_ends_sessions_and_turns_pending_pairings_down() {
        let (mut workspace, _) = integrated_workspace();
        let extension = connect(&mut workspace, FIRST);
        request(&mut workspace, FIRST, &extension.associate());

        let replies = workspace.set_browser_integration(false);

        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].connection, FIRST);
        assert_eq!(parse(&replies[0].message)["errorCode"], "6");
        assert_eq!(workspace.pending_pairing(), None);
        // Turned on again, the old session's keys are gone.
        workspace.set_browser_integration(true);
        let reply = request(&mut workspace, FIRST, &extension.get_database_hash(&[])).unwrap();
        assert_eq!(parse(&reply)["errorCode"], "4");
    }
}
