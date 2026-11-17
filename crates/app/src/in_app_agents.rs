//! The in-app host (ADR 0012 "Where it runs", update "In-app host"): Agents on this machine
//! reach the Vaults unlocked in this desktop keefast through `keefast-agent mcp`, `run` and
//! `store`, which connect to an endpoint of the app's own (`keefast_agent::local`).
//!
//! Each call names its Agent pairing by token. The Vault whose pairing has it answers, through
//! an [`AgentSession`] like a Headless host's, so what an Agent sees and may do is decided in
//! one place. When no unlocked Vault has the token and a Vault the user opened lately is
//! locked, the call is answered "keefast is locked": the token may be that Vault's, and the
//! Agent can't unlock it. Calls never count as the user's activity, so a busy Agent can't keep
//! a Vault from locking when idle.
//!
//! Prompts are put to the user here, as Agent hosts' prompts are, from a host called "this
//! computer"; the first answer, or a minute without one, decides. Each Vault keeps its own
//! audit log, in keefast's data directory, shown as that Vault's Agent Activity.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use keefast_agent::approver::{ApproverConnection, ApproverEvent};
use keefast_agent::approver_client::ApproverClientEvent;
use keefast_agent::{
    AgentError, AgentReply, AgentRequest, AgentSession, AgentSettings, ApproverHub, AuditLog,
    AuditRecord, ServesAgents, VaultAccess,
};
use keefast_native_host::ipc::Server;
use keefast_workspace::{VaultId, VaultLocation, Workspace};
use sha2::{Digest, Sha256};

use crate::agent_hosts;
use crate::core::AppCore;
use crate::error::{ApiError, ApiResult, ErrorKind};

/// The id the prompts of this computer's own Agents wait under, among Agent hosts' prompts.
pub const LOCAL_HOST: &str = "local";
/// What the approval prompt calls this computer.
pub const LOCAL_HOST_NAME: &str = "this computer";

/// The in-app host's state on [`AppCore`].
pub struct InAppAgents {
    approvers: Arc<ApproverHub>,
    /// Where each Vault's audit log goes; `None` keeps no log, until [`start`] says.
    logs: Mutex<Option<PathBuf>>,
    open_logs: Mutex<HashMap<PathBuf, Arc<AuditLog>>>,
    /// The prompts' way to the user, while connected.
    prompts: Mutex<Option<ApproverConnection>>,
    server: Mutex<Option<Server>>,
}

impl InAppAgents {
    pub fn new(approvers: Arc<ApproverHub>) -> Self {
        Self {
            approvers,
            logs: Mutex::default(),
            open_logs: Mutex::default(),
            prompts: Mutex::default(),
            server: Mutex::default(),
        }
    }

    pub fn approvers(&self) -> &Arc<ApproverHub> {
        &self.approvers
    }

    /// The audit log of the Vault file at `location`: one file per Vault file, named by a hash
    /// of where it is.
    fn log(&self, location: &VaultLocation) -> Option<Arc<AuditLog>> {
        let dir = self
            .logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        let hash = Sha256::digest(keefast_agent::host::describe(location).as_bytes());
        let name: String = hash[..12]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let path = dir.join(format!("{name}.log"));
        let mut open = self
            .open_logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Some(Arc::clone(
            open.entry(path.clone())
                .or_insert_with(|| Arc::new(AuditLog::new(path))),
        ))
    }
}

/// `agent-logs` in keefast's data directory, if the OS has one.
pub fn logs_in_os_data_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("keefast").join("agent-logs"))
}

/// Keeps each Vault's audit log in `logs`, puts prompts to the user, and serves `keefast-agent`
/// at `endpoint`. Fails, saying why on stderr, while another keefast serves there.
pub fn start(core: &Arc<AppCore>, logs: Option<PathBuf>, endpoint: io::Result<PathBuf>) {
    *core
        .in_app_agents()
        .logs
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = logs;
    connect_prompts(core);
    let served = endpoint.and_then(|endpoint| {
        keefast_agent::local::serve(endpoint, Arc::new(Agents(Arc::downgrade(core))))
    });
    match served {
        Ok(server) => {
            *core
                .in_app_agents()
                .server
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(server);
        }
        Err(error) => eprintln!("keefast: can't serve Agents on this computer: {error}"),
    }
}

/// Puts the in-app host's prompts to the user, among the Agent hosts'.
pub fn connect_prompts(core: &Arc<AppCore>) {
    let weak = Arc::downgrade(core);
    let connection = core.in_app_agents().approvers.connect(move |event| {
        let Some(core) = weak.upgrade() else {
            return;
        };
        let event = match event.clone() {
            ApproverEvent::Prompt {
                id,
                seconds_left,
                prompt,
            } => ApproverClientEvent::Prompt {
                id,
                seconds_left,
                prompt,
            },
            ApproverEvent::Dismissed { id } => ApproverClientEvent::Dismissed { id },
            // The in-app host has no OAuth: its Agents are on this machine, with a token.
            ApproverEvent::Consent { .. } => return,
        };
        agent_hosts::heard(&core, LOCAL_HOST, event);
    });
    *core
        .in_app_agents()
        .prompts
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(connection);
}

/// What answers `keefast-agent`: the app, while it runs.
pub struct Agents(pub Weak<AppCore>);

impl ServesAgents for Agents {
    fn handle_agent(&self, token: &str, request: AgentRequest) -> Result<AgentReply, AgentError> {
        let core = self.0.upgrade().ok_or(AgentError::Locked)?;
        handle(&core, token, request)
    }
}

/// Handles an Agent's `request` from the Agent pairing whose token is `token`, in the unlocked
/// Vault that has it. It blocks on prompts, for up to a minute.
pub fn handle(
    core: &Arc<AppCore>,
    token: &str,
    request: AgentRequest,
) -> Result<AgentReply, AgentError> {
    let (vault, pairing, location) = core.read(|workspace| paired(workspace, token))?;
    let agents = core.in_app_agents();
    let mut session = AgentSession::new(
        pairing,
        match agents.log(&location) {
            Some(log) => log,
            None => Arc::new(NoLog),
        },
    );
    session = session.with_approver(agents.approvers.clone());
    session.handle(
        &AppVault {
            core: Arc::clone(core),
            vault,
        },
        request,
    )
}

/// The unlocked Vault with an Agent pairing whose token is `token`, and the pairing.
fn paired(
    workspace: &Workspace,
    token: &str,
) -> Result<(VaultId, keefast_agent::AgentPairingId, VaultLocation), AgentError> {
    for open in workspace.readable_vaults() {
        if let Some(pairing) = AgentSettings::of(open.vault()).pairing_with_token(token) {
            return Ok((open.id(), pairing.id().clone(), open.location().clone()));
        }
    }
    let some_locked = workspace.readable_vaults().next().is_none()
        || workspace
            .recent_vaults()
            .iter()
            .any(|location| workspace.open_vault_at(location).is_none());
    Err(if some_locked {
        AgentError::Locked
    } else {
        AgentError::NotPaired
    })
}

/// A Vault the app has open, as an [`AgentSession`] reaches it. Neither reading nor changing it
/// is the user's activity, so neither postpones locking when idle; changes are autosaved as
/// the user's are.
struct AppVault {
    core: Arc<AppCore>,
    vault: VaultId,
}

impl VaultAccess for AppVault {
    fn read<T>(&self, f: impl FnOnce(&Workspace, VaultId) -> T) -> Option<T> {
        self.core.read(|workspace| {
            workspace.readable_vault(self.vault)?;
            Some(f(workspace, self.vault))
        })
    }

    fn edit<T>(&self, f: impl FnOnce(&mut Workspace, VaultId) -> T) -> Option<T> {
        let changed = self.core.update(|workspace| {
            workspace.readable_vault(self.vault)?;
            Some(f(workspace, self.vault))
        });
        if changed.is_some() {
            self.core.autosave();
        }
        changed
    }
}

/// No log, without a data directory to keep one in.
struct NoLog;

impl keefast_agent::AuditSink for NoLog {
    fn record(&self, _: AuditRecord) {}
}

/// The latest `lines` records of the open Vault `vault`'s own audit log, oldest first, for its
/// Agent Activity.
pub fn activity(core: &AppCore, vault: VaultId, lines: usize) -> ApiResult<Vec<AuditRecord>> {
    let location = core
        .read(|workspace| workspace.vault(vault).map(|open| open.location().clone()))
        .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "That Vault isn't open."))?;
    let Some(log) = core.in_app_agents().log(&location) else {
        return Ok(Vec::new());
    };
    log.read(lines).map_err(|error| {
        ApiError::new(
            ErrorKind::Io,
            "keefast couldn't read this Vault's Agent Activity.",
        )
        .with_detail(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    use keefast_agent::{AgentSettingsExt, UseSecrets};
    use keefast_core::EntryFields;
    use keefast_workspace::{Clock, LocalStorage, UnlockSecrets};
    use secrecy::SecretString;

    use super::*;
    use crate::core::tests::Recorder;
    use crate::core::{AppCore, Launch};

    struct FakeClock(Mutex<SystemTime>);

    impl Clock for FakeClock {
        fn now(&self) -> SystemTime {
            *self.0.lock().unwrap()
        }
    }

    impl FakeClock {
        fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }

    /// The desktop app with a Vault unlocked whose Agent group `Agents` holds the pairing
    /// `claude`, and its Entry `claude/GitHub`; and that pairing's token.
    struct App {
        core: Arc<AppCore>,
        clock: Arc<FakeClock>,
        vault: VaultId,
        token: String,
        _dir: tempfile::TempDir,
    }

    fn app() -> App {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.kdbx");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/keepassxc-kdbx4.kdbx"
            ),
            &path,
        )
        .unwrap();
        let clock = Arc::new(FakeClock(Mutex::new(
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_900_000_000),
        )));
        let workspace = Workspace::with_clock(Arc::new(LocalStorage), clock.clone());
        let core = AppCore::new(
            workspace,
            Box::new(Arc::new(Recorder::default())),
            Launch::default(),
        );
        let (vault, token) = core.update(|workspace| {
            let vault = workspace
                .open_vault(
                    &VaultLocation::Local(path.clone()),
                    &UnlockSecrets::password("fixture-password"),
                )
                .unwrap();
            let root = workspace.vault(vault).unwrap().vault().root().id();
            let agents = workspace.create_group(vault, root, "Agents").unwrap();
            workspace.set_agent_group(vault, agents).unwrap();
            let created = workspace
                .create_agent_pairing(vault, "claude", vec![], false)
                .unwrap();
            let fields = EntryFields {
                title: "GitHub".to_owned(),
                password: SecretString::from("ghp_s3cr3t-value-0123456789"),
                ..EntryFields::default()
            };
            workspace
                .create_entry(vault, created.pairing.group(), &fields)
                .unwrap();
            (vault, created.token.to_string())
        });
        *core.in_app_agents().logs.lock().unwrap() = Some(dir.path().join("agent-logs"));
        connect_prompts(&core);
        App {
            core,
            clock,
            vault,
            token,
            _dir: dir,
        }
    }

    #[test]
    fn an_agent_lists_the_secrets_of_the_unlocked_vault_its_token_pairs_it_with() {
        let app = app();

        let listed = handle(&app.core, &app.token, AgentRequest::ListSecrets).unwrap();

        let paths: Vec<String> = listed
            .into_secrets()
            .into_iter()
            .map(|secret| secret.reference.path)
            .collect();
        assert_eq!(paths, ["claude/GitHub"]);
        assert_eq!(
            handle(&app.core, "kfa_nobodys", AgentRequest::ListSecrets),
            Err(AgentError::NotPaired)
        );
    }

    #[test]
    fn a_locked_app_answers_locked_and_the_agent_cant_unlock_it() {
        let app = app();
        app.core
            .lock_with(|workspace, clipboard| workspace.lock_all(clipboard));

        assert_eq!(
            handle(&app.core, &app.token, AgentRequest::ListSecrets),
            Err(AgentError::Locked)
        );
        assert!(
            app.core
                .read(|workspace| workspace.readable_vaults().next().is_none())
        );
    }

    #[test]
    fn agent_calls_dont_keep_the_vault_from_locking_when_idle() {
        let app = app();
        app.core.update(|workspace| {
            workspace.set_idle_lock_after(Some(Duration::from_secs(300)));
            workspace.note_activity();
        });

        for _ in 0..6 {
            app.clock.advance(Duration::from_secs(60));
            handle(&app.core, &app.token, AgentRequest::ListSecrets).unwrap();
        }

        assert_eq!(
            app.core.read(Workspace::idle_lock_in),
            Some(Duration::ZERO),
            "six minutes of Agent calls are still five idle minutes"
        );
    }

    #[test]
    fn a_use_is_put_to_the_user_as_from_this_computer_and_their_answer_goes_back() {
        let app = app();
        let core = Arc::clone(&app.core);
        let token = app.token.clone();
        let using = std::thread::spawn(move || {
            handle(
                &core,
                &token,
                AgentRequest::UseSecrets(UseSecrets {
                    references: vec!["claude/GitHub#password".to_owned()],
                    command: vec!["gh".to_owned(), "auth".to_owned(), "status".to_owned()],
                }),
            )
        });
        let prompt = loop {
            if let Some(prompt) = app.core.agent_hosts().front().cloned() {
                break prompt;
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        assert_eq!(prompt.host, LOCAL_HOST);
        assert_eq!(prompt.host_name, "this computer");
        assert_eq!(prompt.prompt.command, ["gh", "auth", "status"]);
        agent_hosts::answer(&app.core, LOCAL_HOST, &prompt.id, true, false).unwrap();
        let values = using.join().unwrap().unwrap().into_values();
        assert_eq!(values[0].value(), "ghp_s3cr3t-value-0123456789");
        assert!(app.core.agent_hosts().front().is_none());
    }

    #[test]
    fn the_vaults_own_log_is_its_agent_activity() {
        let app = app();

        handle(&app.core, &app.token, AgentRequest::ListSecrets).unwrap();

        let records = activity(&app.core, app.vault, 10).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].pairing, "claude");
        assert_eq!(records[0].action, keefast_agent::AuditAction::List);
    }
}
