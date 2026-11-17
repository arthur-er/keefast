//! The Headless host, `keefast-agent serve`, run in-process on a Vault file on disk, and driven
//! over its admin socket as `keefast-agent unlock`, `lock` and `status` drive it: it starts
//! locked, unlocks with the Master key, tells a wrong key from a damaged Vault file, unlocks
//! unattended only when asked to, and Merges changes made to the Vault file elsewhere.

#![cfg(unix)]

mod support;

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use keefast_agent::admin::{AdminErrorKind, AdminGroup, AdminReply, AdminRequest, AdminSocket};
use keefast_agent::host::{HeadlessHost, HostConfig, HostState, UnattendedUnlock};
use keefast_agent::{
    AgentCredential, AgentDecision, AgentSettingsExt, AgentUse, DefaultAgentPolicy, NewAgentPairing,
};
use keefast_core::EntryFields;
use keefast_workspace::{LocalStorage, SecretStore, Workspace};
use support::{
    FakeClock, FakeSecretStore, KEEPASSXC_KDBX4, KEY_FILE_KDBX, PASSWORD, VaultFile,
    XML_V2_KEY_FILE,
};
use tempfile::TempDir;

/// A Headless host serving a Vault file, with its admin socket.
struct Served {
    host: Arc<HeadlessHost>,
    socket: PathBuf,
    warnings: Vec<String>,
    _dir: TempDir,
}

impl Served {
    fn ask(&self, request: &AdminRequest) -> AdminReply {
        keefast_agent::admin::send(&self.socket, request).unwrap()
    }

    fn state(&self) -> HostState {
        match self.ask(&AdminRequest::Status) {
            AdminReply::Status(status) => status.state,
            other => panic!("expected the host's status, got {other:?}"),
        }
    }

    fn unlock(&self, password: &str, key_file: Option<&[u8]>) -> AdminReply {
        self.ask(&AdminRequest::unlock(password, key_file, false))
    }
}

fn serve_with(
    file: &VaultFile,
    unattended: Option<UnattendedUnlock>,
    secret_store: Arc<dyn SecretStore>,
) -> Served {
    let workspace = Workspace::new(Arc::new(LocalStorage));
    let (host, warnings) = HeadlessHost::start(
        workspace,
        HostConfig {
            location: file.location(),
            unattended,
            secret_store,
            audit_log: file.audit_log(),
            approvers: keefast_agent::ApproverHub::new(),
            approver_credentials: keefast_agent::ApproverCredentials::in_memory(),
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("admin.sock");
    AdminSocket::bind(&socket, None)
        .unwrap()
        .serve(Arc::clone(&host));
    Served {
        host,
        socket,
        warnings,
        _dir: dir,
    }
}

fn serve(file: &VaultFile) -> Served {
    serve_with(file, None, Arc::new(FakeSecretStore::default()))
}

fn error_kind(reply: AdminReply) -> AdminErrorKind {
    match reply {
        AdminReply::Error { kind, .. } => kind,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// Waits up to ten seconds for `done`, as for a change the host picks up on a thread of its own.
fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    done()
}

#[test]
fn serve_starts_locked_and_without_warnings() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);

    let served = serve(&file);

    assert_eq!(served.state(), HostState::Locked);
    assert!(served.warnings.is_empty());
    assert!(served.host.agent_settings().is_none());
}

#[test]
fn the_status_names_the_vault_file_served() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);

    let AdminReply::Status(status) = served.ask(&AdminRequest::Status) else {
        panic!("expected the host's status");
    };

    assert_eq!(status.vault, file.path.display().to_string());
    assert!(!status.unattended);
}

#[test]
fn unlocking_with_the_master_key_unlocks_the_vault() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);

    let reply = served.unlock(PASSWORD, None);

    assert!(matches!(reply, AdminReply::Status(ref status) if status.state == HostState::Unlocked));
    assert_eq!(served.state(), HostState::Unlocked);
    assert!(served.host.agent_settings().is_some());
}

#[test]
fn a_wrong_master_key_says_so_and_the_vault_stays_locked() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);

    let reply = served.unlock("not-the-password", None);

    assert_eq!(error_kind(reply), AdminErrorKind::WrongKey);
    assert_eq!(served.state(), HostState::Locked);
}

#[test]
fn a_damaged_vault_file_is_told_apart_from_a_wrong_master_key() {
    let mut damaged = KEEPASSXC_KDBX4.to_vec();
    let middle = damaged.len() / 2;
    damaged.truncate(middle);
    let file = VaultFile::from_fixture(&damaged);
    let served = serve(&file);

    let reply = served.unlock(PASSWORD, None);

    assert_eq!(error_kind(reply), AdminErrorKind::Damaged);
    assert_eq!(served.state(), HostState::Locked);
}

#[test]
fn a_master_key_with_a_key_file_unlocks_with_the_key_files_contents() {
    let file = VaultFile::from_fixture(KEY_FILE_KDBX);
    let served = serve(&file);

    assert_eq!(
        error_kind(served.unlock(PASSWORD, None)),
        AdminErrorKind::WrongKey
    );
    let reply = served.unlock(PASSWORD, Some(XML_V2_KEY_FILE));

    assert!(matches!(reply, AdminReply::Status(_)), "{reply:?}");
    assert_eq!(served.state(), HostState::Unlocked);
}

#[test]
fn lock_locks_the_vault_again() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);
    served.unlock(PASSWORD, None);

    let reply = served.ask(&AdminRequest::Lock);

    assert!(matches!(reply, AdminReply::Status(ref status) if status.state == HostState::Locked));
    assert_eq!(served.state(), HostState::Locked);
    assert!(served.host.agent_settings().is_none());
}

#[test]
fn the_admin_socket_is_open_only_to_its_owner() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);

    let mode = std::fs::metadata(&served.socket)
        .unwrap()
        .permissions()
        .mode();

    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn the_admin_socket_can_be_opened_to_an_operator_group() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("admin.sock");
    // SAFETY: getegid has no preconditions.
    let group = unsafe { libc::getegid() };

    let _bound = AdminSocket::bind(&socket, Some(AdminGroup::with_id(group))).unwrap();

    let metadata = std::fs::metadata(&socket).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o660);
    assert_eq!(metadata.gid(), group);
}

#[test]
fn unattended_unlock_from_a_key_file_starts_unlocked_and_warns() {
    let file = VaultFile::from_fixture(KEY_FILE_KDBX);
    let key_file = file.path.with_file_name("agents.keyx");
    std::fs::write(&key_file, XML_V2_KEY_FILE).unwrap();
    let store = FakeSecretStore::holding(HeadlessHost::UNATTENDED_PASSWORD, PASSWORD);

    let served = serve_with(
        &file,
        Some(UnattendedUnlock {
            key_file: Some(key_file),
            password: true,
        }),
        store,
    );

    assert_eq!(served.state(), HostState::Unlocked);
    assert!(
        served
            .warnings
            .iter()
            .any(|warning| warning.contains("unattended unlock")),
        "{:?}",
        served.warnings
    );
}

#[test]
fn unattended_unlock_uses_the_password_unlock_remembered() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let store = Arc::new(FakeSecretStore::default());
    let unattended = || {
        Some(UnattendedUnlock {
            key_file: None,
            password: true,
        })
    };
    let first = serve_with(&file, unattended(), store.clone());
    assert_eq!(first.state(), HostState::Locked);
    assert!(first.warnings.len() >= 2, "{:?}", first.warnings);

    first.ask(&AdminRequest::unlock(PASSWORD, None, true));
    drop(first);
    let second = serve_with(&file, unattended(), store.clone());

    assert_eq!(second.state(), HostState::Unlocked);
    assert_eq!(
        store.secret(HeadlessHost::UNATTENDED_PASSWORD).as_deref(),
        Some(PASSWORD)
    );
}

#[test]
fn a_change_made_to_the_vault_file_elsewhere_is_merged_in() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);
    served.unlock(PASSWORD, None);
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (mut desktop, vault) = file.open(&clock);

    desktop
        .set_default_agent_policy(vault, DefaultAgentPolicy::Deny)
        .unwrap();
    desktop.save_vault(vault).unwrap();

    let merged = eventually(|| {
        served.host.agent_settings().unwrap().default_agent_policy() == DefaultAgentPolicy::Deny
    });
    assert!(merged, "the host never Merged the change in");
}

#[test]
fn a_vault_with_agent_pairings_and_decisions_round_trips_through_serve_unchanged() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (mut desktop, vault) = file.open(&clock);
    let root = desktop.readable_vault(vault).unwrap().vault().root().id();
    let agents = desktop.create_group(vault, root, "Agents").unwrap();
    let claude = desktop.create_group(vault, agents, "claude").unwrap();
    desktop.set_agent_group(vault, agents).unwrap();
    let pairing = desktop
        .add_agent_pairing(
            vault,
            NewAgentPairing {
                name: "claude".to_owned(),
                group: claude,
                readable: vec![],
                can_create: true,
                credential: AgentCredential::token("kfa_claudes-token"),
            },
        )
        .unwrap();
    let entry = desktop
        .create_entry(vault, claude, &EntryFields::default())
        .unwrap();
    let gh = AgentUse::Program("gh".to_owned());
    desktop
        .remember_agent_decision(vault, entry, pairing.id(), &gh, AgentDecision::Allow)
        .unwrap();
    desktop.save_vault(vault).unwrap();
    let before = std::fs::read(&file.path).unwrap();

    let served = serve(&file);
    served.unlock(PASSWORD, None);
    let settings = served.host.agent_settings().unwrap();
    served.ask(&AdminRequest::Lock);

    assert_eq!(settings.agent_group(), Some(agents));
    assert_eq!(settings.pairings().len(), 1);
    assert_eq!(settings.pairings()[0].name(), "claude");
    assert_eq!(
        settings.decision(entry, pairing.id(), &gh),
        Some(AgentDecision::Allow)
    );
    assert_eq!(std::fs::read(&file.path).unwrap(), before);
}
