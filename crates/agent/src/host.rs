//! The Headless host (`keefast-agent serve`): keefast with no window, serving one Vault to
//! Agents (ADR 0012).
//!
//! It starts locked, and stays so until someone unlocks it with the Master key over the admin
//! socket (`keefast-agent unlock`), so that a reboot or a stolen disk doesn't hand the Vault's
//! secrets to anyone. Only an operator who opts in to unattended unlock gives that up, from a
//! Key file or the OS secret store, and is warned at startup.
//!
//! Like the desktop app, it Merges changes made to the Vault file elsewhere: a local Vault file
//! is watched, and a Dropbox one checked on the Workspace's schedule. It never calls
//! `note_activity`, and never idle-locks: nobody is at its keyboard.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::Duration;

use keefast_core::OpenError;
use keefast_workspace::{
    Clipboard, OpenVaultError, PendingSave, SecretStore, SecretStoreError, UnlockSecrets,
    UnsavedLock, VaultId, VaultLocation, Workspace, settled_vault_file_changes,
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::approver::{Approver, ApproverCredentials, ApproverHub};
use crate::audit::{AuditLog, AuditRecord, AuditSink};
use crate::{AgentError, AgentReply, AgentRequest, AgentSession, AgentSettings, VaultAccess};

/// How a Headless host is set up.
pub struct HostConfig {
    /// The Vault file it serves.
    pub location: VaultLocation,
    /// Unlocking without anyone typing the Master key, if the operator opted in.
    pub unattended: Option<UnattendedUnlock>,
    /// The OS secret store, where an unattended unlock's password is kept.
    pub secret_store: Arc<dyn SecretStore>,
    /// Where every Agent's list and use is recorded, in the host's state directory.
    pub audit_log: Arc<AuditLog>,
    /// The desktop keefasts asked about uses no decision answers.
    pub approvers: Arc<ApproverHub>,
    /// Who may connect as an Approver, in the host's state directory.
    pub approver_credentials: ApproverCredentials,
}

/// Where an unattended unlock finds the Master key at startup. Anyone who can read these can
/// open the Vault without its Master key, so the Vault loses its protection at rest.
#[derive(Debug, Clone, Default)]
pub struct UnattendedUnlock {
    /// The Master key's Key file.
    pub key_file: Option<PathBuf>,
    /// Whether the Master key has a password, kept in the OS secret store under
    /// [`HeadlessHost::UNATTENDED_PASSWORD`] by `keefast-agent unlock --remember`.
    pub password: bool,
}

/// Whether the Vault is unlocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostState {
    Locked,
    Unlocked,
    /// Asked to lock, but its unsaved changes couldn't be saved yet. Agents are shut out
    /// already; it locks once they are saved.
    Locking,
}

/// What `keefast-agent status` shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostStatus {
    pub state: HostState,
    /// The Vault file served.
    pub vault: String,
    /// Whether unattended unlock is on.
    pub unattended: bool,
}

/// Why the Vault couldn't be unlocked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnlockError {
    #[error("the Master key is wrong: check the password and the Key file")]
    WrongKey,
    #[error("the Vault file is damaged or not a Vault file: {0}")]
    Damaged(String),
    #[error("the Key file can't be used: {0}")]
    KeyFile(String),
    #[error("the Vault file can't be read: {0}")]
    Storage(String),
}

impl From<OpenVaultError> for UnlockError {
    fn from(error: OpenVaultError) -> Self {
        match error {
            OpenVaultError::Vault(OpenError::WrongKey) => Self::WrongKey,
            OpenVaultError::Vault(error) => Self::Damaged(error.to_string()),
            OpenVaultError::KeyFileNotFound(_)
            | OpenVaultError::KeyFileUnreadable(_)
            | OpenVaultError::KeyFile(_) => Self::KeyFile(error.to_string()),
            OpenVaultError::Storage(error) => Self::Storage(error.to_string()),
            // Handled before: the Vault is unlocked already.
            OpenVaultError::AlreadyOpen(_) => Self::Storage(error.to_string()),
        }
    }
}

/// A Headless host serving one Vault file.
pub struct HeadlessHost {
    inner: Mutex<Inner>,
    location: VaultLocation,
    unattended: bool,
    secret_store: Arc<dyn SecretStore>,
    audit_log: Arc<AuditLog>,
    approvers: Arc<ApproverHub>,
    approver_credentials: ApproverCredentials,
}

struct Inner {
    workspace: Workspace,
    /// The served Vault's id, once it was first unlocked; it keeps it when locked.
    vault: Option<VaultId>,
}

impl HeadlessHost {
    /// The name the password for unattended unlock is kept under in the OS secret store.
    pub const UNATTENDED_PASSWORD: &str = "agent-unattended-unlock";

    /// Starts serving the Vault file through `workspace`, locked unless unattended unlock is on
    /// and works. Returns warnings for the operator: that unattended unlock is on, and why it
    /// failed if it did.
    pub fn start(workspace: Workspace, config: HostConfig) -> (Arc<Self>, Vec<String>) {
        let host = Arc::new(Self {
            inner: Mutex::new(Inner {
                workspace,
                vault: None,
            }),
            location: config.location,
            unattended: config.unattended.is_some(),
            secret_store: config.secret_store,
            audit_log: config.audit_log,
            approvers: config.approvers,
            approver_credentials: config.approver_credentials,
        });
        host.watch_vault_file();
        host.sync_online_vault();
        let mut warnings = Vec::new();
        if let Some(unattended) = config.unattended {
            warnings.push(
                "unattended unlock is on: the Vault opens without anyone typing its Master key, \
                 so whoever can read this machine's disk or OS secret store can open it too"
                    .to_owned(),
            );
            if let Err(why) = host.unlock_unattended(unattended) {
                warnings.push(format!(
                    "unattended unlock failed, so the Vault stays locked: {why}"
                ));
            }
        }
        (host, warnings)
    }

    fn unlock_unattended(&self, unattended: UnattendedUnlock) -> Result<(), String> {
        let password = if unattended.password {
            match self.secret_store.get(Self::UNATTENDED_PASSWORD) {
                Ok(Some(password)) => password,
                Ok(None) => {
                    return Err("no password is remembered for it yet: \
                                unlock once with `keefast-agent unlock --remember`"
                        .to_owned());
                }
                Err(error) => return Err(error.to_string()),
            }
        } else {
            SecretString::from(String::new())
        };
        let secrets = UnlockSecrets::new(password, unattended.key_file);
        self.unlock(secrets).map_err(|error| error.to_string())
    }

    /// Unlocks the Vault. Unlocking a Vault already unlocked changes nothing.
    pub fn unlock(&self, secrets: UnlockSecrets) -> Result<(), UnlockError> {
        let pending = {
            let inner = self.lock_inner();
            if inner.unlocked().is_some() {
                return Ok(());
            }
            match inner.workspace.begin_open(self.location.clone(), secrets) {
                Ok(pending) => pending,
                Err(OpenVaultError::AlreadyOpen(_)) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        };
        // Deriving the key is slow: status requests are answered meanwhile.
        let unlocked = pending.unlock()?;
        let mut inner = self.lock_inner();
        // A Headless host never sees the OS lock, so the unlock is always added.
        if let Some(id) = inner.workspace.add_unlocked(unlocked) {
            inner.vault = Some(id);
        }
        Ok(())
    }

    /// Keeps `password` in the OS secret store for unattended unlock.
    pub fn remember_password(&self, password: &SecretString) -> Result<(), SecretStoreError> {
        self.secret_store.set(Self::UNATTENDED_PASSWORD, password)
    }

    /// Locks the Vault, saving its unsaved changes first. Agents are shut out at once; if the
    /// save fails, the Vault stays [`HostState::Locking`] until a later save succeeds.
    pub fn lock(&self) -> Result<(), String> {
        let saves = {
            let mut inner = self.lock_inner();
            let Some(id) = inner.vault else {
                return Ok(());
            };
            let workspace = &mut inner.workspace;
            let mut saves = workspace.lock_all(&mut NoClipboard);
            if workspace.vault(id).is_some_and(|open| open.is_locking()) {
                saves.extend(workspace.answer_lock(id, UnsavedLock::Save));
            }
            saves
        };
        self.run_saves(saves)
    }

    pub fn status(&self) -> HostStatus {
        let inner = self.lock_inner();
        let state = match inner.vault.and_then(|id| inner.workspace.vault(id)) {
            Some(open) if open.is_locking() => HostState::Locking,
            Some(_) => HostState::Unlocked,
            None => HostState::Locked,
        };
        HostStatus {
            state,
            vault: describe(&self.location),
            unattended: self.unattended,
        }
    }

    /// Runs `f` on the Workspace and the served Vault, if it is unlocked; `None` if it is
    /// locked or locking.
    pub fn with_vault<T>(&self, f: impl FnOnce(&mut Workspace, VaultId) -> T) -> Option<T> {
        let mut inner = self.lock_inner();
        let id = inner.unlocked()?;
        Some(f(&mut inner.workspace, id))
    }

    /// Runs `f` on the Workspace and the served Vault, if it is unlocked, then saves the Vault,
    /// so that what an operator changed is in the Vault file before they hear it is done.
    /// `None` if it is locked or locking; else `f`'s answer, and whether the save worked.
    pub fn edit<T>(
        &self,
        f: impl FnOnce(&mut Workspace, VaultId) -> T,
    ) -> Option<(T, Result<(), String>)> {
        let (answer, saves) = {
            let mut inner = self.lock_inner();
            let id = inner.unlocked()?;
            let answer = f(&mut inner.workspace, id);
            let saves = if inner
                .workspace
                .vault(id)
                .is_some_and(|open| open.has_unsaved_changes())
            {
                inner.workspace.begin_save(id).ok().into_iter().collect()
            } else {
                Vec::new()
            };
            (answer, saves)
        };
        Some((answer, self.run_saves(saves)))
    }

    /// Handles an Agent's `request`, from the Agent pairing whose bearer token is `token`,
    /// through an [`AgentSession`] that records it in the audit log and asks the connected
    /// Approvers. It blocks on the Workspace, and on an Approver for up to a minute, so async
    /// callers run it on a blocking thread.
    pub fn handle_agent(
        &self,
        token: &str,
        request: AgentRequest,
    ) -> Result<AgentReply, AgentError> {
        let pairing = self
            .with_vault(|workspace, vault| {
                let settings = AgentSettings::of(workspace.readable_vault(vault)?.vault());
                Some(settings.pairing_with_token(token)?.id().clone())
            })
            .ok_or(AgentError::Locked)?
            .ok_or(AgentError::NotPaired)?;
        let audit: Arc<dyn AuditSink> = self.audit_log.clone();
        let approver: Arc<dyn Approver> = self.approvers.clone();
        AgentSession::new(pairing, audit)
            .with_approver(approver)
            .handle(self, request)
    }

    /// The Approvers connected to this host, and the prompts waiting for them.
    pub fn approvers(&self) -> &Arc<ApproverHub> {
        &self.approvers
    }

    /// Who may connect as an Approver.
    pub fn approver_credentials(&self) -> &ApproverCredentials {
        &self.approver_credentials
    }

    /// The audit log's latest `limit` records, oldest first.
    pub fn audit_records(&self, limit: usize) -> std::io::Result<Vec<AuditRecord>> {
        self.audit_log.read(limit)
    }

    /// The served Vault's Agent settings, if it is unlocked.
    pub fn agent_settings(&self) -> Option<AgentSettings> {
        self.with_vault(|workspace, id| {
            AgentSettings::of(workspace.readable_vault(id).expect("unlocked").vault())
        })
    }

    fn lock_inner(&self) -> MutexGuard<'_, Inner> {
        // A thread that panicked holding the Workspace leaves it as consistent as any edit
        // does: every Workspace call either completes or changes nothing.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs saves off the Workspace lock, one after the other, then those asked for meanwhile.
    fn run_saves(&self, mut saves: Vec<PendingSave>) -> Result<(), String> {
        let mut outcome = Ok(());
        while !saves.is_empty() {
            for save in saves {
                let finished = save.run();
                if let Err(error) = self.lock_inner().workspace.finish_save(finished) {
                    outcome = Err(format!("the Vault couldn't be saved: {error}"));
                }
            }
            saves = if outcome.is_ok() {
                self.lock_inner().workspace.begin_autosaves()
            } else {
                Vec::new()
            };
        }
        outcome
    }

    /// Merges changes made to a local Vault file elsewhere, once they settle.
    fn watch_vault_file(self: &Arc<Self>) {
        let (changes, changed) = mpsc::channel();
        self.lock_inner().workspace.watch_vault_files(move |vault| {
            let _ = changes.send(vault);
        });
        let host = Arc::downgrade(self);
        thread::spawn(move || {
            // Ends once the host, and with it the Workspace holding `changes`, is gone.
            while let Some(vaults) = settled_vault_file_changes(&changed) {
                let Some(host) = host.upgrade() else { return };
                let saves = {
                    let mut inner = host.lock_inner();
                    for vault in vaults {
                        inner.workspace.vault_file_changed(vault);
                    }
                    inner.workspace.begin_autosaves()
                };
                report(host.run_saves(saves));
            }
        });
    }

    /// Checks a Dropbox Vault file for changes made elsewhere whenever one is due.
    fn sync_online_vault(self: &Arc<Self>) {
        let host: Weak<Self> = Arc::downgrade(self);
        thread::spawn(move || {
            loop {
                let Some(wait) = host
                    .upgrade()
                    .map(|host| host.lock_inner().workspace.sync_wait())
                else {
                    return;
                };
                thread::sleep(wait);
                let Some(host) = host.upgrade() else { return };
                let syncs = {
                    let mut inner = host.lock_inner();
                    if inner.workspace.sync_in() != Some(Duration::ZERO) {
                        continue;
                    }
                    inner.workspace.begin_syncs()
                };
                report(host.run_saves(syncs));
            }
        });
    }
}

/// An [`AgentSession`] reaches the served Vault through the host's lock, and its changes are
/// saved at once.
impl VaultAccess for HeadlessHost {
    fn read<T>(&self, f: impl FnOnce(&Workspace, VaultId) -> T) -> Option<T> {
        self.with_vault(|workspace, vault| f(workspace, vault))
    }

    fn edit<T>(&self, f: impl FnOnce(&mut Workspace, VaultId) -> T) -> Option<T> {
        let (answer, saved) = HeadlessHost::edit(self, f)?;
        report(saved);
        Some(answer)
    }
}

impl Inner {
    /// The served Vault, if it is unlocked and not waiting to lock.
    fn unlocked(&self) -> Option<VaultId> {
        let id = self.vault?;
        self.workspace.readable_vault(id).map(|open| open.id())
    }
}

/// A Headless host has no clipboard: locking has nothing to clear from one.
struct NoClipboard;

impl Clipboard for NoClipboard {
    fn read_text(&self) -> Option<String> {
        None
    }

    fn write_text(&mut self, _text: &str) {}
}

/// The Vault file as the operator named it: a path, or `dropbox:` and a Dropbox path.
pub fn describe(location: &VaultLocation) -> String {
    match location {
        VaultLocation::Local(path) => path.display().to_string(),
        VaultLocation::Dropbox(path) => format!("dropbox:{path}"),
    }
}

/// The Vault file the operator named: `dropbox:` and a path in their Dropbox, or else a path
/// on this machine.
pub fn parse_location(vault: &str) -> VaultLocation {
    match vault.strip_prefix("dropbox:") {
        Some(path) => VaultLocation::Dropbox(path.to_owned()),
        None => VaultLocation::Local(PathBuf::from(vault)),
    }
}

/// Background saves and Merges have nobody to tell but the service log.
fn report(outcome: Result<(), String>) {
    if let Err(error) = outcome {
        eprintln!("keefast-agent: {error}");
    }
}
