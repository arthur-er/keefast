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

use keefast_core::{GroupId, Vault};

use crate::approver::{Approver, ApproverCredentials, ApproverHub, ConsentAnswer, ConsentPrompt};
use crate::audit::{AuditLog, AuditRecord, AuditSink};
use crate::oauth::{
    ACCESS_TOKEN_PREFIX, AuthorizeError, AuthorizeRequest, BegunConsent, CONSENT_LIFETIME,
    ConsentError, IssuedTokens, OAuthServer, PendingConsent, TokenError, TokenRequest,
};
use crate::{
    AgentError, AgentPairingId, AgentReply, AgentRequest, AgentSession, AgentSettings,
    AgentSettingsError, AgentSettingsExt, VaultAccess,
};

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
    /// The MCP clients paired by OAuth and their tokens, in the host's state directory.
    pub oauth: OAuthServer,
}

/// Whose a bearer token is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenOwner {
    /// An Agent pairing's, by its own token or an OAuth access token.
    Pairing(AgentPairingId),
    /// Nobody's: refuse it.
    Unknown,
    /// Perhaps an Agent pairing's, but its hash is in the Vault, which is locked.
    Locked,
}

/// What the user chose when approving an OAuth client's consent: the new Agent pairing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingChoice {
    pub name: String,
    /// The other subgroups of the Agent group it reads, by name.
    #[serde(default)]
    pub reads: Vec<String>,
    #[serde(default)]
    pub can_create: bool,
}

/// An OAuth client's consent approved: the Agent pairing it pairs as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedConsent {
    pub client: String,
    pub pairing: String,
    /// Whether the client had that pairing already, so nothing new was made.
    pub again: bool,
}

/// Why a consent couldn't be approved or denied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConsentApprovalError {
    #[error(transparent)]
    Consent(#[from] ConsentError),
    #[error("the Vault is locked, and Agent pairings are kept in it: unlock it first")]
    Locked,
    #[error(transparent)]
    Pairing(#[from] AgentSettingsError),
    #[error("the Agent group has no subgroup called {0}")]
    NoSubgroup(String),
    #[error("{0}")]
    Save(String),
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
    oauth: OAuthServer,
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
            oauth: config.oauth,
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
        let pairing = match self.token_owner(token) {
            TokenOwner::Pairing(pairing) => pairing,
            TokenOwner::Unknown => return Err(AgentError::NotPaired),
            TokenOwner::Locked => return Err(AgentError::Locked),
        };
        let audit: Arc<dyn AuditSink> = self.audit_log.clone();
        let approver: Arc<dyn Approver> = self.approvers.clone();
        AgentSession::new(pairing, audit)
            .with_approver(approver)
            .handle(self, request)
    }

    /// Whose `token` is: an Agent pairing's own, or an OAuth access token of a client that has
    /// an Agent pairing. An OAuth client whose pairing was revoked in the Vault loses its
    /// tokens here, once the host sees the change. An OAuth token can be told while the Vault
    /// is locked, as its hash is in the host's state; an expired one is refused even then, so
    /// its client refreshes it.
    pub fn token_owner(&self, token: &str) -> TokenOwner {
        if token.starts_with(ACCESS_TOKEN_PREFIX) {
            let Some(client) = self.oauth.client_with_access_token(token) else {
                return TokenOwner::Unknown;
            };
            return match self.agent_settings() {
                None => TokenOwner::Locked,
                Some(settings) => match settings.pairing_with_oauth_client(&client) {
                    Some(pairing) => TokenOwner::Pairing(pairing.id().clone()),
                    None => {
                        self.oauth.forget_client(&client);
                        TokenOwner::Unknown
                    }
                },
            };
        }
        match self.agent_settings() {
            None => TokenOwner::Locked,
            Some(settings) => settings
                .pairing_with_token(token)
                .map_or(TokenOwner::Unknown, |pairing| {
                    TokenOwner::Pairing(pairing.id().clone())
                }),
        }
    }

    /// The MCP clients paired by OAuth, and their tokens.
    pub fn oauth(&self) -> &OAuthServer {
        &self.oauth
    }

    /// Begins an OAuth client's consent, and shows it to every Approver.
    pub fn begin_consent(
        &self,
        request: AuthorizeRequest,
        issuer: &str,
        resource: &str,
    ) -> Result<BegunConsent, AuthorizeError> {
        self.withdraw_expired_consents();
        let begun = self.oauth.begin(request, issuer, resource)?;
        let (subgroups, paired_as) = self
            .with_vault(|workspace, vault| {
                let open = workspace.readable_vault(vault)?;
                let settings = AgentSettings::of(open.vault());
                let subgroups = settings
                    .agent_group()
                    .and_then(|group| open.vault().group(group))
                    .map(|group| {
                        group
                            .groups()
                            .iter()
                            .map(|child| child.name().to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                let paired_as = settings
                    .pairing_with_oauth_client(&begun.client_id)
                    .map(|pairing| pairing.name().to_owned());
                Some((subgroups, paired_as))
            })
            .flatten()
            .unwrap_or_default();
        self.approvers.offer_consent(
            &begun.prompt,
            ConsentPrompt {
                client: begun.client_name.clone(),
                redirect: begun.redirect_host.clone(),
                subgroups,
                paired_as,
            },
            CONSENT_LIFETIME,
        );
        Ok(begun)
    }

    /// Answers the consent an Approver was shown as `prompt`: approving needs the code its
    /// page shows, and creates the Agent pairing the user chose.
    pub fn answer_consent(
        &self,
        prompt: &str,
        answer: ConsentAnswer,
    ) -> Result<Option<ApprovedConsent>, ConsentApprovalError> {
        if !answer.approve {
            self.oauth.deny(prompt)?;
            self.approvers.withdraw_consent(prompt);
            return Ok(None);
        }
        let consent = self.oauth.consent_for_prompt(prompt, &answer.code);
        if consent.is_err() && !self.oauth.is_waiting(prompt) {
            // Denied for too many wrong codes, or gone.
            self.approvers.withdraw_consent(prompt);
        }
        let choice = PairingChoice {
            name: answer.name,
            reads: answer.reads,
            can_create: answer.can_create,
        };
        self.approve_consent(&consent?, choice).map(Some)
    }

    /// Approves the consent whose page shows `code`, as `keefast-agent pairing approve` does.
    pub fn approve_consent_code(
        &self,
        code: &str,
        choice: PairingChoice,
    ) -> Result<ApprovedConsent, ConsentApprovalError> {
        self.withdraw_expired_consents();
        let consent = self.oauth.consent_with_code(code)?;
        self.approve_consent(&consent, choice)
    }

    /// Creates the Agent pairing for `consent`'s client, unless it has one, and approves it.
    fn approve_consent(
        &self,
        consent: &PendingConsent,
        choice: PairingChoice,
    ) -> Result<ApprovedConsent, ConsentApprovalError> {
        let (made, saved) = self
            .edit(|workspace, vault| {
                let settings = workspace
                    .agent_settings(vault)
                    .ok_or(ConsentApprovalError::Locked)?;
                if let Some(pairing) = settings.pairing_with_oauth_client(&consent.client_id) {
                    return Ok::<_, ConsentApprovalError>((pairing.name().to_owned(), true));
                }
                let agent_group = settings
                    .agent_group()
                    .ok_or(AgentSettingsError::NoAgentGroup)?;
                let open = workspace
                    .readable_vault(vault)
                    .ok_or(ConsentApprovalError::Locked)?;
                let reads = subgroups_named(open.vault(), agent_group, &choice.reads)
                    .map_err(ConsentApprovalError::NoSubgroup)?;
                // Unnamed, it is named as the client calls itself.
                let name = match choice.name.trim() {
                    "" => consent.client_name.replace('/', "-"),
                    name => name.to_owned(),
                };
                let pairing = workspace.create_oauth_agent_pairing(
                    vault,
                    &name,
                    reads,
                    choice.can_create,
                    &consent.client_id,
                )?;
                Ok((pairing.name().to_owned(), false))
            })
            .ok_or(ConsentApprovalError::Locked)?;
        let (pairing, again) = made?;
        saved.map_err(ConsentApprovalError::Save)?;
        self.oauth.approve(&consent.prompt)?;
        self.approvers.withdraw_consent(&consent.prompt);
        Ok(ApprovedConsent {
            client: consent.client_name.clone(),
            pairing,
            again,
        })
    }

    /// Cancels the consent whose page is `page`, from the page; where to send the browser.
    pub fn cancel_consent(&self, page: &str) -> Option<String> {
        let (prompt, redirect) = self.oauth.cancel(page)?;
        self.approvers.withdraw_consent(&prompt);
        Some(redirect)
    }

    /// Answers a token request; a refresh is refused once the client's Agent pairing is gone
    /// from the Vault, which can't be told while it is locked.
    pub fn oauth_token(
        &self,
        request: TokenRequest,
        resource: &str,
    ) -> Result<IssuedTokens, TokenError> {
        let still_paired = |client: &str| {
            self.agent_settings()
                .map(|settings| settings.pairing_with_oauth_client(client).is_some())
        };
        self.oauth.token(request, resource, &still_paired)
    }

    /// Tells Approvers to dismiss consents nobody approved in time.
    pub fn withdraw_expired_consents(&self) {
        for prompt in self.oauth.expired_prompts() {
            self.approvers.withdraw_consent(&prompt);
        }
    }

    /// The Approvers connected to this host, and the prompts waiting for them.
    pub fn approvers(&self) -> &Arc<ApproverHub> {
        &self.approvers
    }

    /// Who may connect as an Approver.
    pub fn approver_credentials(&self) -> &ApproverCredentials {
        &self.approver_credentials
    }

    /// Unpairs the Approver called `name` and cuts off its open prompt streams at once, so it
    /// is neither shown nor counted on for another prompt.
    pub fn revoke_approver(&self, name: &str) -> Result<(), crate::approver::ApproverPairingError> {
        self.approver_credentials.unpair(name)?;
        self.approvers.disconnect(name);
        Ok(())
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

impl crate::ServesAgents for HeadlessHost {
    fn handle_agent(&self, token: &str, request: AgentRequest) -> Result<AgentReply, AgentError> {
        HeadlessHost::handle_agent(self, token, request)
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

/// The subgroups of `agent_group` called `names`, or the first name none is called.
pub fn subgroups_named(
    vault: &Vault,
    agent_group: GroupId,
    names: &[String],
) -> Result<Vec<GroupId>, String> {
    let subgroups = vault
        .group(agent_group)
        .map(|group| group.groups().to_vec())
        .unwrap_or_default();
    names
        .iter()
        .map(|name| {
            subgroups
                .iter()
                .find(|group| group.name() == name)
                .map(|group| group.id())
                .ok_or_else(|| name.clone())
        })
        .collect()
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
