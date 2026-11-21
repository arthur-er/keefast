//! `AgentSession`: one Agent pairing's requests, handled against a Workspace (ADR 0012, spec
//! "Implementation Decisions").
//!
//! It is the single entry point for Agents, shaped like the workspace's `BrowserSession`:
//! every host, whether the Headless host's MCP endpoint, the desktop app's in-app relay or the
//! endpoints `keefast-agent run` calls, translates its transport into [`AgentRequest`]s and
//! [`AgentReply`]s and back, so what a pairing may see and do is decided here, once.
//!
//! An Agent sees only Entries in the Vault's Agent group, and of those only the ones in its
//! pairing's own subgroup and the subgroups it may read, with their subgroups. It never sees
//! an Entry's notes: they are where prompt injection would hide, and they often hold secrets
//! themselves. Each request reads the pairing afresh from the Vault, so revoking one takes
//! effect at once.
//!
//! A use (`keefast-agent run`) names its secrets by Secret reference, and is checked against
//! the Agent access decisions for the pairing, each Entry and the program, then the Default
//! agent policy. When neither answers, or the program is a shell or interpreter, the session's
//! [`Approver`] is asked, without holding the Workspace, for up to a minute; with nobody to ask,
//! the use is refused at once, saying how to allow it ahead of time in the desktop app. Every
//! list and use goes to the audit log, never with a value.
//!
//! A pairing with the create right also writes, only directly in its own subgroup: see
//! [`writes`].

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use keefast_core::{Entry, EntryId, Group, GroupId, Vault};
use keefast_workspace::{VaultId, Workspace};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::approver::{AgentPrompt, Approval, Approver, NoApprover};
use crate::audit::{AuditAction, AuditEntry, AuditOutcome, AuditRecord, AuditSink};
use crate::hosts::{Allowed, allows, is_private_address, url_host};
use crate::http::{
    Hop, HttpRequest, HttpResponse, HttpTransport, MAX_REDIRECTS, MAX_REQUEST_BODY,
    NetworkTransport, Template, basic_auth, body_text, expand, is_redirect, percent_encode,
};
use crate::programs::{is_shell_or_interpreter, program_name};
use crate::run::Redactor;
use crate::scope::{ReferenceError, ScopeError};
pub(crate) use writes::own_path;
pub use writes::{CreateEntry, StoreSecret, WrittenEntry};

mod writes;

use crate::{
    AgentDecision, AgentPairing, AgentPairingId, AgentSettings, AgentSettingsExt, AgentUse,
    DefaultAgentPolicy, PairingKind,
};

/// A password at least this long gets a hint of its last four characters; a shorter one would
/// give too much of itself away.
const HINT_FROM_LENGTH: usize = 12;
const HINT_LENGTH: usize = 4;

/// What an Agent asks of keefast. It travels as JSON between `keefast-agent` and the desktop
/// app (see [`crate::local`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRequest {
    /// The secrets the pairing can read, without their values.
    ListSecrets,
    /// The values of secrets, for `keefast-agent run` to put in a program's environment.
    UseSecrets(UseSecrets),
    /// An HTTP request keefast sends with secrets put in, to their Entries' allowed hosts.
    Http(HttpRequest),
    /// A new Entry in the pairing's own subgroup, perhaps with a password keefast generates.
    CreateEntry(CreateEntry),
    /// A value for one field of an Entry in the pairing's own subgroup, for `keefast-agent
    /// store`, which creates the Entry if needed.
    Store(StoreSecret),
    /// The value a Secret reference names, handed to the Agent and so to the model behind it:
    /// only for Entries marked "Agents may reveal", and only when the user allows it, each time.
    Reveal(String),
}

/// What `keefast-agent run` asks for: the values `references` name, to run `command` with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseSecrets {
    /// Secret references, such as `GitHub/token#password`.
    pub references: Vec<String>,
    /// The program and its arguments, whole, as the Agent will run them.
    pub command: Vec<String>,
}

/// keefast's answer to an [`AgentRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentReply {
    Secrets(Vec<SecretListing>),
    /// One value per reference asked for, in the same order.
    Values(Vec<SecretValue>),
    /// What an `http_request` got back, its secrets redacted.
    Http(HttpResponse),
    /// The Entry a `create_entry` or `store` wrote.
    Written(WrittenEntry),
}

impl AgentReply {
    /// The secrets listed in reply to [`AgentRequest::ListSecrets`]; empty for another reply.
    pub fn into_secrets(self) -> Vec<SecretListing> {
        match self {
            Self::Secrets(secrets) => secrets,
            _ => Vec::new(),
        }
    }

    /// The values in reply to [`AgentRequest::UseSecrets`]; empty for another reply.
    pub fn into_values(self) -> Vec<SecretValue> {
        match self {
            Self::Values(values) => values,
            _ => Vec::new(),
        }
    }

    /// The Entry written in reply to [`AgentRequest::CreateEntry`] or [`AgentRequest::Store`].
    pub fn into_written(self) -> Option<WrittenEntry> {
        match self {
            Self::Written(written) => Some(written),
            _ => None,
        }
    }

    /// The response in reply to [`AgentRequest::Http`].
    pub fn into_http(self) -> Option<HttpResponse> {
        match self {
            Self::Http(response) => Some(response),
            _ => None,
        }
    }
}

/// A secret's value, and the Secret reference it was asked for by. Its `Debug` hides the value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretValue {
    pub reference: String,
    value: Zeroizing<String>,
}

impl SecretValue {
    pub fn new(reference: String, value: Zeroizing<String>) -> Self {
        Self { reference, value }
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretValue")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}

/// Why an Agent's request was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentError {
    #[error("keefast is locked: ask the user to unlock it, then try again. Agents can't unlock it")]
    Locked,
    #[error(
        "this Agent pairing was revoked or doesn't exist: ask the user to pair this Agent again"
    )]
    NotPaired,
    #[error(
        "no secret this Agent pairing can read matches {0}: list_secrets shows the ones it can"
    )]
    UnknownReference(String),
    #[error("{0} matches more than one Entry: name it by the Entry's UUID instead")]
    AmbiguousReference(String),
    #[error("the Entry {0} names has no such field: list_secrets shows its fields")]
    UnknownField(String),
    #[error("{0} names no field: end it with # and a field, such as #password")]
    NoField(String),
    #[error("no program to run was given")]
    NoProgram,
    #[error(
        "the user hasn't allowed this Agent pairing to use {reference} with {program}, and there \
         is no Approver to ask them now: ask them to open keefast on their desktop, paired with \
         this host as an Approver, or to allow it in keefast's Entry editor, under Agents, then \
         try again"
    )]
    NotAllowed { reference: String, program: String },
    #[error(
        "the user didn't answer within 60 seconds whether this Agent pairing may use {reference} \
         with {program}, so it was denied: ask them to watch for keefast's prompt, then try again"
    )]
    Unanswered { reference: String, program: String },
    #[error("the user doesn't allow this Agent pairing to use {reference} with {program}")]
    Denied { reference: String, program: String },
    #[error(
        "http_request sends a secret: put {{{{secret:<Secret reference>}}}} in its URL's path or \
         query, a header, the body or basic auth. For a request without one, use another tool"
    )]
    NoSecret,
    #[error("keefast won't send this request: {0}")]
    BadHttpRequest(String),
    #[error(
        "{host} isn't one of the hosts the Entry {reference} names may be sent to: ask the user \
         to add it in keefast's Entry editor, under Agents, if it should be"
    )]
    HostNotAllowed { reference: String, host: String },
    #[error(
        "the server redirected the request to {host}, which isn't one of the hosts the Entry \
         {reference} names may be sent to, so keefast stopped before sending it there"
    )]
    RedirectNotAllowed { reference: String, host: String },
    #[error(
        "{host} is at {address}, a private, loopback or link-local address, which keefast sends \
         secrets to only for a host the Entry's allowed hosts name exactly"
    )]
    PrivateAddress { host: String, address: String },
    #[error("the request was redirected more than {0} times")]
    TooManyRedirects(usize),
    #[error("the request failed: {0}")]
    HttpFailed(String),
    #[error(
        "this Agent pairing may not create or change Entries: ask the user to allow it to create \
         Entries in keefast's Vault settings, under Agents"
    )]
    NoCreateRight,
    #[error(
        "{0} is outside this Agent pairing's own subgroup: Agents create and change Entries only \
         directly in their own subgroup, and list_secrets shows its path"
    )]
    NotOwnEntry(String),
    #[error(
        "{0} is already an Entry: set a field on it with keefast-agent store, or choose another \
         title"
    )]
    EntryExists(String),
    #[error("keefast won't write this Entry: {0}")]
    BadEntry(String),
    #[error(
        "the Entry {0} names isn't marked \"Agents may reveal\", so its values never reach the \
         model: use the secret without seeing it, with keefast-agent run or http_request"
    )]
    NotRevealable(String),
    #[error(
        "revealing {0} needs the user's say each time, and there is no Approver to ask them now: \
         ask them to open keefast on their desktop, paired with this host as an Approver, then \
         try again"
    )]
    NoApproverToReveal(String),
    #[error(
        "keefast-agent and the keefast desktop app don't understand each other ({0}): update \
         both to the same version"
    )]
    Mismatch(String),
}

impl From<ScopeError> for AgentError {
    fn from(error: ScopeError) -> Self {
        match error {
            ScopeError::Locked => Self::Locked,
            ScopeError::NotPaired => Self::NotPaired,
        }
    }
}

impl From<ReferenceError> for AgentError {
    fn from(error: ReferenceError) -> Self {
        match error {
            ReferenceError::NoField(reference) => Self::NoField(reference),
            ReferenceError::Unknown(reference) => Self::UnknownReference(reference),
            ReferenceError::Ambiguous(reference) => Self::AmbiguousReference(reference),
            ReferenceError::UnknownField(reference) => Self::UnknownField(reference),
        }
    }
}

/// A secret as an Agent sees it in a list: what helps it pick the right one, and how to name
/// it, but never a value and never the Entry's notes. All of it is text the Agent didn't
/// write, which hosts mark as untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretListing {
    pub title: String,
    pub username: String,
    pub url: String,
    pub tags: Vec<String>,
    /// The names of the Entry's fields a Secret reference can end in: `password`, `username`
    /// and `url` when they aren't empty, then its custom fields.
    pub fields: Vec<String>,
    pub reference: SecretReference,
    /// The password's last four characters, when it is long enough that they give little away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The two ways an Agent can name an Entry in a Secret reference, before `#<field>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretReference {
    /// The Entry's UUID in hex: never ambiguous, and stable across renames.
    pub uuid: String,
    /// Its subgroups below the Agent group and its title, joined by `/`: readable, but it may
    /// match more than one Entry.
    pub path: String,
}

/// Where a session reaches the Vault it serves. Each call holds the Workspace only while it
/// runs, so that the session can let go of it while an Approver is asked: a host keeps
/// serving, and its Vault keeps syncing, while the user decides.
pub trait VaultAccess {
    /// Runs `f` on the Workspace and the Vault, if it is unlocked; `None` if it is locked.
    fn read<T>(&self, f: impl FnOnce(&Workspace, VaultId) -> T) -> Option<T>;
    /// As [`Self::read`], for a change to the Vault, which the host saves after.
    fn edit<T>(&self, f: impl FnOnce(&mut Workspace, VaultId) -> T) -> Option<T>;
}

/// Whatever answers Agents' requests by their Agent pairing's token: a Headless host, the
/// desktop app, or `keefast-agent`'s connection to the desktop app, for the MCP tools to call.
/// It blocks on the Workspace and on prompts, so async callers run it on a blocking thread.
pub trait ServesAgents: Send + Sync + 'static {
    fn handle_agent(&self, token: &str, request: AgentRequest) -> Result<AgentReply, AgentError>;
}

/// One Agent pairing's requests.
#[derive(Clone)]
pub struct AgentSession {
    pairing: AgentPairingId,
    audit: Arc<dyn AuditSink>,
    approver: Arc<dyn Approver>,
    transport: Arc<dyn HttpTransport>,
}

/// What a use is for: the program `keefast-agent run` starts, the host an `http_request`
/// goes to, with its method, or a reveal of one field to the Agent itself.
enum UseFor<'a> {
    Program(&'a str),
    Http { method: &'a str, host: &'a str },
    Reveal { field: &'a str },
}

/// What a reveal is called where a program or host would be named: in its prompt, and in what
/// the Agent is told when it is refused.
const REVEAL: &str = "reveal_secret";

impl UseFor<'_> {
    /// The program or host, as the Agent is told it.
    fn name(&self) -> &str {
        match self {
            Self::Program(program) => program,
            Self::Http { host, .. } => host,
            Self::Reveal { .. } => REVEAL,
        }
    }

    /// What an answer to this use is remembered for; a reveal's never is.
    fn agent_use(&self) -> Option<AgentUse> {
        match self {
            Self::Program(program) => Some(AgentUse::Program((*program).to_owned())),
            Self::Http { host, .. } => Some(AgentUse::Host((*host).to_owned())),
            Self::Reveal { .. } => None,
        }
    }

    fn action(&self) -> AuditAction {
        match self {
            Self::Program(_) => AuditAction::Use,
            Self::Http { .. } => AuditAction::HttpRequest,
            Self::Reveal { .. } => AuditAction::Reveal,
        }
    }

    /// `record`, as a record of this use.
    fn audited(&self, mut record: AuditRecord) -> AuditRecord {
        record.action = self.action();
        match self {
            Self::Program(program) => {
                record.program = Some((*program).to_owned());
            }
            Self::Http { method, host } => {
                record.method = Some((*method).to_owned());
                record.host = Some((*host).to_owned());
            }
            Self::Reveal { field } => {
                record.field = Some((*field).to_owned());
            }
        }
        record
    }
}

/// An Entry an `http_request` uses a secret of.
struct UsedEntry {
    id: EntryId,
    uuid: String,
    title: String,
    /// The first Secret reference the request names it by.
    reference: String,
    allowed_hosts: Vec<String>,
}

/// An `http_request` with its placeholders filled in, from one look at the Vault.
struct Prepared {
    url: url::Url,
    host: String,
    headers: Vec<(String, Zeroizing<String>)>,
    body: Option<Zeroizing<Vec<u8>>>,
    /// Everything to redact from what comes back: the values, and the forms they were sent in.
    secrets: Vec<Zeroizing<Vec<u8>>>,
    used: Vec<UsedEntry>,
}

/// An Entry a use needs the Approver's answer for, as the first look at the Vault found it.
struct Asking {
    prompt: AgentPrompt,
    entry: EntryId,
}

impl AgentSession {
    /// A session for the Agent pairing `pairing`, recording what it does in `audit`, with
    /// nobody to ask: a use that would ask is denied until [`Self::with_approver`] gives one.
    pub fn new(pairing: AgentPairingId, audit: Arc<dyn AuditSink>) -> Self {
        Self {
            pairing,
            audit,
            approver: Arc::new(NoApprover),
            transport: Arc::new(NetworkTransport),
        }
    }

    /// Sends `http_request`s through `transport` rather than the network.
    pub fn with_transport(mut self, transport: Arc<dyn HttpTransport>) -> Self {
        self.transport = transport;
        self
    }

    /// Asks `approver` about uses no decision or policy answers.
    pub fn with_approver(mut self, approver: Arc<dyn Approver>) -> Self {
        self.approver = approver;
        self
    }

    /// This session's pairing in `settings`: `NotPaired` once it is gone, or if it is a
    /// Gateway pairing.
    pub(crate) fn own_pairing<'s>(
        &self,
        settings: &'s AgentSettings,
    ) -> Result<&'s AgentPairing, AgentError> {
        settings
            .pairing_of_kind(&self.pairing, PairingKind::Agent)
            .ok_or(AgentError::NotPaired)
    }

    pub fn pairing(&self) -> &AgentPairingId {
        &self.pairing
    }

    /// Handles `request` against the Vault `vault` reaches. It never notes activity: Agents
    /// don't keep a Vault from locking when its user is idle. A use that needs the user's say
    /// blocks until the Approver answers.
    pub fn handle(
        &self,
        vault: &impl VaultAccess,
        request: AgentRequest,
    ) -> Result<AgentReply, AgentError> {
        match request {
            AgentRequest::ListSecrets => vault
                .read(|workspace, id| self.list_secrets(workspace, id))
                .unwrap_or(Err(AgentError::Locked)),
            AgentRequest::UseSecrets(request) => self.use_secrets(vault, request),
            AgentRequest::Http(request) => self.http_request(vault, request),
            AgentRequest::CreateEntry(request) => self.create_entry(vault, request),
            AgentRequest::Store(request) => self.store(vault, request),
            AgentRequest::Reveal(reference) => self.reveal(vault, reference),
        }
    }

    fn list_secrets(&self, workspace: &Workspace, id: VaultId) -> Result<AgentReply, AgentError> {
        let vault = workspace
            .readable_vault(id)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let pairing = self.own_pairing(&settings)?;
        self.audit.record(self.record(
            workspace,
            pairing,
            AuditAction::List,
            AuditOutcome::Allowed,
        ));
        Ok(AgentReply::Secrets(
            readable_entries(vault, &settings, pairing)
                .iter()
                .map(|(entry, path)| listing(entry, path))
                .collect(),
        ))
    }

    /// A use, in three steps: decide what the Vault can, holding the Workspace; ask the
    /// Approver about the rest, without it; then, holding it again, remember what was to be
    /// remembered and read the values afresh, since the Vault may have changed meanwhile.
    fn use_secrets(
        &self,
        vault: &impl VaultAccess,
        request: UseSecrets,
    ) -> Result<AgentReply, AgentError> {
        let program = program_name(request.command.first().ok_or(AgentError::NoProgram)?);
        if program.is_empty() {
            return Err(AgentError::NoProgram);
        }
        let (asking, first_time) = vault
            .read(|workspace, id| self.decide_use(workspace, id, &request, &program))
            .unwrap_or(Err(AgentError::Locked))?;
        self.ask(vault, asking, first_time, &UseFor::Program(&program))?;
        vault
            .read(|workspace, id| self.values(workspace, id, request))
            .unwrap_or(Err(AgentError::Locked))
    }

    /// Checks each Entry a use names against its Agent access decisions and the Default agent
    /// policy, auditing what they answer, and returns those the Approver must be asked about,
    /// with the time it was checked.
    fn decide_use(
        &self,
        workspace: &Workspace,
        id: VaultId,
        request: &UseSecrets,
        program: &str,
    ) -> Result<(Vec<Asking>, i64), AgentError> {
        let vault = workspace
            .readable_vault(id)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let pairing = self.own_pairing(&settings)?;
        let readable = readable_entries(vault, &settings, pairing);
        let mut resolved = Vec::with_capacity(request.references.len());
        for reference in &request.references {
            resolved.push(resolve(&readable, reference)?.0);
        }
        // Each Entry is checked, and audited, once, however many of its fields are used.
        let mut checked: Vec<EntryId> = Vec::new();
        let mut asking = Vec::new();
        for (entry, reference) in resolved.iter().zip(&request.references) {
            if checked.contains(&entry.id()) {
                continue;
            }
            checked.push(entry.id());
            let agent_use = AgentUse::Program(program.to_owned());
            let refused = match decide(&settings, entry.id(), pairing.id(), &agent_use) {
                Outcome::Allow => false,
                Outcome::Deny => true,
                Outcome::Ask => {
                    asking.push(Asking {
                        prompt: AgentPrompt {
                            pairing: pairing.name().to_owned(),
                            entry_uuid: entry.id().to_hex(),
                            entry_title: entry.title().to_owned(),
                            reference: reference.clone(),
                            program: program.to_owned(),
                            command: request.command.clone(),
                            can_remember: !is_shell_or_interpreter(program),
                            host: None,
                            reveal: false,
                        },
                        entry: entry.id(),
                    });
                    continue;
                }
            };
            let outcome = if refused {
                AuditOutcome::Denied
            } else {
                AuditOutcome::Allowed
            };
            let mut record = UseFor::Program(program).audited(self.record(
                workspace,
                pairing,
                AuditAction::Use,
                outcome,
            ));
            record.entry = Some(AuditEntry {
                uuid: entry.id().to_hex(),
                title: entry.title().to_owned(),
            });
            record.reason = refused.then(|| "denied".to_owned());
            self.audit.record(record);
            if refused {
                return Err(AgentError::Denied {
                    reference: reference.clone(),
                    program: program.to_owned(),
                });
            }
        }
        Ok((asking, workspace.now().unix_seconds()))
    }

    /// Saves answers the user asked to remember as Agent access decisions, so they reach every
    /// host serving the Vault. A Vault locked meanwhile keeps none of them: the use is refused
    /// as locked anyway.
    fn remember(
        &self,
        vault: &impl VaultAccess,
        agent_use: &AgentUse,
        answers: &[(EntryId, AgentDecision)],
    ) {
        vault.edit(|workspace, id| {
            for (entry, decision) in answers {
                // Fails only for a revoked pairing or a deleted Entry, which the use then hears.
                let _ = workspace.remember_agent_decision(
                    id,
                    *entry,
                    &self.pairing,
                    agent_use,
                    *decision,
                );
            }
        });
    }

    /// The values a use names, read now.
    fn values(
        &self,
        workspace: &Workspace,
        id: VaultId,
        request: UseSecrets,
    ) -> Result<AgentReply, AgentError> {
        let vault = workspace
            .readable_vault(id)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let pairing = self.own_pairing(&settings)?;
        let readable = readable_entries(vault, &settings, pairing);
        let mut values = Vec::with_capacity(request.references.len());
        for reference in request.references {
            let (_, value) = resolve(&readable, &reference)?;
            values.push(SecretValue::new(reference, value));
        }
        Ok(AgentReply::Values(values))
    }

    fn record(
        &self,
        workspace: &Workspace,
        pairing: &AgentPairing,
        action: AuditAction,
        outcome: AuditOutcome,
    ) -> AuditRecord {
        AuditRecord::of(workspace.now().unix_seconds(), pairing, action, outcome)
    }

    /// Asks the Approver about each Entry in `asking`, auditing each answer, and remembers the
    /// answers the user asked to. The first refusal ends it.
    fn ask(
        &self,
        vault: &impl VaultAccess,
        asking: Vec<Asking>,
        first_time: i64,
        used_for: &UseFor<'_>,
    ) -> Result<(), AgentError> {
        let name = used_for.name().to_owned();
        let mut remember = Vec::new();
        let mut refused = None;
        for asking in asking {
            let reference = asking.prompt.reference.clone();
            let can_remember = asking.prompt.can_remember;
            let (reason, refusal) = match self.approver.ask(&asking.prompt) {
                Approval::Allow { remember: wants } => {
                    if wants && can_remember {
                        remember.push((asking.entry, AgentDecision::Allow));
                        ("allowed and remembered at a prompt", None)
                    } else {
                        ("allowed once at a prompt", None)
                    }
                }
                Approval::Deny { remember: wants } => {
                    let reason = if wants && can_remember {
                        remember.push((asking.entry, AgentDecision::Deny));
                        "denied and remembered at a prompt"
                    } else {
                        "denied once at a prompt"
                    };
                    let program = name.clone();
                    (reason, Some(AgentError::Denied { reference, program }))
                }
                Approval::Unanswered => (
                    "nobody answered the prompt in time",
                    Some(AgentError::Unanswered {
                        reference,
                        program: name.clone(),
                    }),
                ),
                Approval::NoApprover => (
                    "no Approver to ask",
                    Some(match used_for {
                        // Nothing allows a reveal ahead of time, so the user can only be asked.
                        UseFor::Reveal { .. } => AgentError::NoApproverToReveal(reference),
                        _ => AgentError::NotAllowed {
                            reference,
                            program: name.clone(),
                        },
                    }),
                ),
            };
            let time = vault
                .read(|workspace, _| workspace.now().unix_seconds())
                .unwrap_or(first_time);
            self.audit.record(used_for.audited(AuditRecord {
                time,
                pairing: asking.prompt.pairing.clone(),
                pairing_id: self.pairing.as_str().to_owned(),
                kind: PairingKind::Agent,
                action: used_for.action(),
                entry: Some(AuditEntry {
                    uuid: asking.prompt.entry_uuid.clone(),
                    title: asking.prompt.entry_title.clone(),
                }),
                program: None,
                method: None,
                host: None,
                field: None,
                outcome: if refusal.is_some() {
                    AuditOutcome::Denied
                } else {
                    AuditOutcome::Allowed
                },
                reason: Some(reason.to_owned()),
                claimed_for: None,
            }));
            if refusal.is_some() {
                refused = refusal;
                break;
            }
        }
        if let Some(agent_use) = used_for.agent_use()
            && !remember.is_empty()
        {
            self.remember(vault, &agent_use, &remember);
        }
        refused.map_or(Ok(()), Err)
    }
}

/// `http_request`: decided like a use, keyed on the host, then sent hop by hop, each hop's host
/// checked against every used Entry's allowed hosts and its addresses against private ranges.
impl AgentSession {
    fn http_request(
        &self,
        vault: &impl VaultAccess,
        request: HttpRequest,
    ) -> Result<AgentReply, AgentError> {
        let template = Template::check(request)?;
        let (asking, first_time, host) = vault
            .read(|workspace, id| self.decide_http(workspace, id, &template))
            .unwrap_or(Err(AgentError::Locked))?;
        let used_for = UseFor::Http {
            method: &template.method,
            host: &host,
        };
        self.ask(vault, asking, first_time, &used_for)?;
        // The values are read afresh: the Vault may have changed while the user was asked.
        let prepared = vault
            .read(|workspace, id| {
                let (prepared, pairing) = self.prepare(workspace, id, &template)?;
                self.check_first_host(workspace, &pairing, &prepared, &template.method)?;
                Ok::<_, AgentError>(prepared)
            })
            .unwrap_or(Err(AgentError::Locked))?;
        self.send(vault, &template.method, prepared)
            .map(AgentReply::Http)
    }

    /// Checks the request's host against each used Entry's allowed hosts, then its decisions
    /// and the Default agent policy, auditing what they answer, and returns the Entries the
    /// Approver must be asked about, the time, and the host.
    fn decide_http(
        &self,
        workspace: &Workspace,
        id: VaultId,
        template: &Template,
    ) -> Result<(Vec<Asking>, i64, String), AgentError> {
        let (prepared, pairing) = self.prepare(workspace, id, template)?;
        self.check_first_host(workspace, &pairing, &prepared, &template.method)?;
        let vault = workspace
            .readable_vault(id)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let host = prepared.host.clone();
        let used_for = UseFor::Http {
            method: &template.method,
            host: &host,
        };
        let agent_use = used_for
            .agent_use()
            .expect("an http_request is remembered by its host");
        let mut asking = Vec::new();
        for used in &prepared.used {
            let refused = match decide(&settings, used.id, pairing.id(), &agent_use) {
                Outcome::Allow => false,
                Outcome::Deny => true,
                Outcome::Ask => {
                    asking.push(Asking {
                        prompt: AgentPrompt {
                            pairing: pairing.name().to_owned(),
                            entry_uuid: used.uuid.clone(),
                            entry_title: used.title.clone(),
                            reference: used.reference.clone(),
                            program: host.clone(),
                            command: vec![template.method.clone(), template.request.url.clone()],
                            can_remember: true,
                            host: Some(host.clone()),
                            reveal: false,
                        },
                        entry: used.id,
                    });
                    continue;
                }
            };
            let outcome = if refused {
                AuditOutcome::Denied
            } else {
                AuditOutcome::Allowed
            };
            let mut record =
                used_for.audited(self.record(workspace, &pairing, used_for.action(), outcome));
            record.entry = Some(AuditEntry {
                uuid: used.uuid.clone(),
                title: used.title.clone(),
            });
            record.reason = refused.then(|| "denied".to_owned());
            self.audit.record(record);
            if refused {
                return Err(AgentError::Denied {
                    reference: used.reference.clone(),
                    program: host,
                });
            }
        }
        Ok((asking, workspace.now().unix_seconds(), host))
    }

    /// Refuses the request if a used Entry doesn't allow its first host, auditing it.
    fn check_first_host(
        &self,
        workspace: &Workspace,
        pairing: &AgentPairing,
        prepared: &Prepared,
        method: &str,
    ) -> Result<(), AgentError> {
        let Some(used) = prepared
            .used
            .iter()
            .find(|used| allows(&used.allowed_hosts, &prepared.host).is_none())
        else {
            return Ok(());
        };
        let host = redact(&prepared.secrets, &prepared.host);
        let used_for = UseFor::Http {
            method,
            host: &host,
        };
        let mut record = used_for.audited(self.record(
            workspace,
            pairing,
            used_for.action(),
            AuditOutcome::Denied,
        ));
        record.entry = Some(AuditEntry {
            uuid: used.uuid.clone(),
            title: used.title.clone(),
        });
        record.reason = Some("not one of the Entry's allowed hosts".to_owned());
        self.audit.record(record);
        Err(AgentError::HostNotAllowed {
            reference: used.reference.clone(),
            host,
        })
    }

    /// The request with its placeholders filled in from the Entries the pairing reads now.
    fn prepare(
        &self,
        workspace: &Workspace,
        id: VaultId,
        template: &Template,
    ) -> Result<(Prepared, AgentPairing), AgentError> {
        let bad = |message: &str| AgentError::BadHttpRequest(message.to_owned());
        let vault = workspace
            .readable_vault(id)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let pairing = self.own_pairing(&settings)?;
        let readable = readable_entries(vault, &settings, pairing);
        let mut values: HashMap<&str, Zeroizing<String>> = HashMap::new();
        let mut used: Vec<UsedEntry> = Vec::new();
        for reference in &template.references {
            let (entry, value) = resolve(&readable, reference)?;
            values.insert(reference, value);
            if !used.iter().any(|known| known.id == entry.id()) {
                used.push(UsedEntry {
                    id: entry.id(),
                    uuid: entry.id().to_hex(),
                    title: entry.title().to_owned(),
                    reference: reference.clone(),
                    allowed_hosts: settings.allowed_hosts(entry.id()).to_vec(),
                });
            }
        }
        let value = |reference: &str| values.get(reference).cloned();
        let mut secrets: Vec<Zeroizing<Vec<u8>>> = Vec::new();
        for value in values.values() {
            secrets.push(Zeroizing::new(value.as_bytes().to_vec()));
            let encoded = percent_encode(value);
            if encoded.as_str() != value.as_str() {
                secrets.push(Zeroizing::new(encoded.as_bytes().to_vec()));
            }
        }
        let request = &template.request;
        let url = expand(&request.url, &value, true);
        let url = url::Url::parse(&url).map_err(|_| bad("the URL isn't one"))?;
        if !matches!(url.scheme(), "https" | "http") {
            return Err(bad("keefast sends only https requests"));
        }
        let host = url_host(&url).ok_or_else(|| bad("the URL has no host"))?;
        let mut headers: Vec<(String, Zeroizing<String>)> = request
            .headers
            .iter()
            .map(|(name, text)| (name.clone(), expand(text, &value, false)))
            .collect();
        if let Some(auth) = &request.basic_auth {
            let header = basic_auth(
                &expand(&auth.username, &value, false),
                &expand(&auth.password, &value, false),
            );
            // What a server echoing the header back would show.
            if let Some(encoded) = header.strip_prefix("Basic ") {
                secrets.push(Zeroizing::new(encoded.as_bytes().to_vec()));
            }
            headers.push(("Authorization".to_owned(), header));
        }
        if headers
            .iter()
            .any(|(_, value)| value.contains(['\r', '\n']))
        {
            return Err(bad("a header's value can't have a line break in it"));
        }
        let body = match &request.body {
            Some(text) => {
                let body = expand(text, &value, false);
                if body.len() > MAX_REQUEST_BODY {
                    return Err(AgentError::BadHttpRequest(format!(
                        "the body may be at most {MAX_REQUEST_BODY} bytes, with its secrets"
                    )));
                }
                Some(Zeroizing::new(body.as_bytes().to_vec()))
            }
            None => None,
        };
        Ok((
            Prepared {
                url,
                host,
                headers,
                body,
                secrets,
                used,
            },
            pairing.clone(),
        ))
    }

    /// Sends the request, following redirects by hand so that each hop's host and addresses
    /// are checked before the secrets go there, and redacts the response.
    fn send(
        &self,
        vault: &impl VaultAccess,
        first_method: &str,
        prepared: Prepared,
    ) -> Result<HttpResponse, AgentError> {
        let Prepared {
            mut url,
            mut host,
            headers,
            mut body,
            secrets,
            used,
        } = prepared;
        let mut method = first_method.to_owned();
        let mut redirects = 0;
        let response = loop {
            let mut by_name = true;
            for entry in &used {
                match allows(&entry.allowed_hosts, &host) {
                    Some(Allowed::ByName) => {}
                    Some(Allowed::ByWildcard) => by_name = false,
                    None => {
                        let host = redact(&secrets, &host);
                        self.refused(
                            vault,
                            entry,
                            &method,
                            &host,
                            "redirected to a host the Entry doesn't allow",
                        );
                        return Err(AgentError::RedirectNotAllowed {
                            reference: entry.reference.clone(),
                            host,
                        });
                    }
                }
            }
            let addresses: Vec<IpAddr> = match url.host() {
                Some(url::Host::Ipv4(v4)) => vec![IpAddr::V4(v4)],
                Some(url::Host::Ipv6(v6)) => vec![IpAddr::V6(v6)],
                Some(url::Host::Domain(domain)) => {
                    let port = url.port_or_known_default().unwrap_or(443);
                    self.transport
                        .resolve(domain, port)
                        .map_err(|error| AgentError::HttpFailed(redact(&secrets, &error)))?
                }
                None => return Err(AgentError::BadHttpRequest("the URL has no host".to_owned())),
            };
            if addresses.is_empty() {
                return Err(AgentError::HttpFailed(format!("{host} has no address")));
            }
            let private = addresses
                .iter()
                .find(|address| is_private_address(**address));
            if let Some(address) = private
                && !by_name
            {
                let host = redact(&secrets, &host);
                for entry in &used {
                    self.refused(vault, entry, &method, &host, "at a private address");
                }
                return Err(AgentError::PrivateAddress {
                    host,
                    address: address.to_string(),
                });
            }
            if url.scheme() == "http" && !addresses.iter().all(|a| is_private_address(*a)) {
                let host = redact(&secrets, &host);
                for entry in &used {
                    self.refused(
                        vault,
                        entry,
                        &method,
                        &host,
                        "plain http to a public address",
                    );
                }
                return Err(AgentError::BadHttpRequest(format!(
                    "{host} is a public address, which keefast sends secrets to only over https"
                )));
            }
            let hop = Hop {
                method: &method,
                url: &url,
                addresses: &addresses,
                headers: &headers,
                body: body.as_deref().map(Vec::as_slice),
            };
            let response = self
                .transport
                .send(&hop)
                .map_err(|error| AgentError::HttpFailed(redact(&secrets, &error)))?;
            let location = response
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("location"))
                .map(|(_, value)| value.clone());
            let (true, Some(location)) = (is_redirect(response.status), location) else {
                break response;
            };
            redirects += 1;
            if redirects > MAX_REDIRECTS {
                return Err(AgentError::TooManyRedirects(MAX_REDIRECTS));
            }
            url = url.join(&location).map_err(|_| {
                AgentError::HttpFailed(
                    "the server redirected to something that isn't a URL".to_owned(),
                )
            })?;
            if !matches!(url.scheme(), "https" | "http") {
                return Err(AgentError::HttpFailed(
                    "the server redirected to something other than http or https".to_owned(),
                ));
            }
            host = url_host(&url).ok_or_else(|| {
                AgentError::HttpFailed("the server redirected to a URL without a host".to_owned())
            })?;
            // As browsers do: a 303, and a 301 or 302 after a POST, go on as a GET.
            if (response.status == 303 && method != "HEAD")
                || (matches!(response.status, 301 | 302) && method == "POST")
            {
                method = "GET".to_owned();
                body = None;
            }
        };
        let mut redactor = Redactor::new(secrets.clone());
        let mut redacted = redactor.push(&response.body);
        // A body cut off may end in the start of a secret, which must not be passed on.
        if !response.truncated {
            redacted.extend(redactor.finish());
        }
        let (body, body_encoding) = body_text(redacted);
        Ok(HttpResponse {
            status: response.status,
            url: redact(&secrets, url.as_str()),
            headers: response
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), redact(&secrets, value)))
                .collect(),
            body,
            body_encoding: body_encoding.to_owned(),
            truncated: response.truncated,
        })
    }

    /// Audits a hop refused after the use was allowed.
    fn refused(
        &self,
        vault: &impl VaultAccess,
        entry: &UsedEntry,
        method: &str,
        host: &str,
        reason: &str,
    ) {
        let Some(Some(pairing)) = vault.read(|workspace, id| {
            let settings = workspace.agent_settings(id)?;
            let pairing = self.own_pairing(&settings).ok()?.clone();
            Some(self.record(
                workspace,
                &pairing,
                AuditAction::HttpRequest,
                AuditOutcome::Denied,
            ))
        }) else {
            return;
        };
        let mut record = UseFor::Http { method, host }.audited(pairing);
        record.entry = Some(AuditEntry {
            uuid: entry.uuid.clone(),
            title: entry.title.clone(),
        });
        record.reason = Some(reason.to_owned());
        self.audit.record(record);
    }
}

/// `reveal_secret`: the one way a value reaches the model. Only an Entry marked "Agents may
/// reveal" is revealed, and only once the user allows it at a prompt that says so, every time:
/// no Agent access decision or Default agent policy allows it, though a policy of deny refuses
/// it, as it does a shell.
impl AgentSession {
    fn reveal(
        &self,
        vault: &impl VaultAccess,
        reference: String,
    ) -> Result<AgentReply, AgentError> {
        let (asking, first_time) = vault
            .read(|workspace, id| self.decide_reveal(workspace, id, &reference))
            .unwrap_or(Err(AgentError::Locked))?;
        let field = reference.rsplit_once('#').map_or("", |(_, field)| field);
        self.ask(vault, vec![asking], first_time, &UseFor::Reveal { field })?;
        // The value is read afresh, and the mark checked again: either may have changed while
        // the user was asked.
        vault
            .read(|workspace, id| {
                let vault = workspace
                    .readable_vault(id)
                    .ok_or(AgentError::Locked)?
                    .vault();
                let settings = AgentSettings::of(vault);
                let pairing = self.own_pairing(&settings)?;
                let readable = readable_entries(vault, &settings, pairing);
                let (entry, value) = resolve(&readable, &reference)?;
                if !settings.agents_may_reveal(entry.id()) {
                    return Err(AgentError::NotRevealable(reference.clone()));
                }
                Ok(AgentReply::Values(vec![SecretValue::new(
                    reference.clone(),
                    value,
                )]))
            })
            .unwrap_or(Err(AgentError::Locked))
    }

    /// Refuses, and audits, a reveal of an unmarked Entry, or one the Default agent policy
    /// denies; otherwise returns the prompt to ask, with the time it was checked.
    fn decide_reveal(
        &self,
        workspace: &Workspace,
        id: VaultId,
        reference: &str,
    ) -> Result<(Asking, i64), AgentError> {
        let vault = workspace
            .readable_vault(id)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let pairing = self.own_pairing(&settings)?;
        let readable = readable_entries(vault, &settings, pairing);
        let (entry, _) = resolve(&readable, reference)?;
        let refusal = if !settings.agents_may_reveal(entry.id()) {
            Some((
                "not marked Agents may reveal",
                AgentError::NotRevealable(reference.to_owned()),
            ))
        } else if settings.default_agent_policy() == DefaultAgentPolicy::Deny {
            Some((
                "denied",
                AgentError::Denied {
                    reference: reference.to_owned(),
                    program: REVEAL.to_owned(),
                },
            ))
        } else {
            None
        };
        if let Some((reason, error)) = refusal {
            let field = reference.rsplit_once('#').map_or("", |(_, field)| field);
            let mut record = UseFor::Reveal { field }.audited(self.record(
                workspace,
                pairing,
                AuditAction::Reveal,
                AuditOutcome::Denied,
            ));
            record.entry = Some(AuditEntry {
                uuid: entry.id().to_hex(),
                title: entry.title().to_owned(),
            });
            record.reason = Some(reason.to_owned());
            self.audit.record(record);
            return Err(error);
        }
        let asking = Asking {
            prompt: AgentPrompt {
                pairing: pairing.name().to_owned(),
                entry_uuid: entry.id().to_hex(),
                entry_title: entry.title().to_owned(),
                reference: reference.to_owned(),
                program: REVEAL.to_owned(),
                command: vec![REVEAL.to_owned(), reference.to_owned()],
                can_remember: false,
                host: None,
                reveal: true,
            },
            entry: entry.id(),
        };
        Ok((asking, workspace.now().unix_seconds()))
    }
}

/// `text` with every secret in `secrets` replaced.
fn redact(secrets: &[Zeroizing<Vec<u8>>], text: &str) -> String {
    let mut redactor = Redactor::new(secrets.to_vec());
    let mut out = redactor.push(text.as_bytes());
    out.extend(redactor.finish());
    String::from_utf8_lossy(&out).into_owned()
}

enum Outcome {
    Allow,
    Deny,
    Ask,
}

/// What the Vault answers `pairing` using `entry` with a program or host: its Agent access decision, or
/// else its Default agent policy. Shells and interpreters ask every time, whatever was
/// remembered or the policy allows, unless the policy denies everything.
fn decide(
    settings: &AgentSettings,
    entry: EntryId,
    pairing: &AgentPairingId,
    agent_use: &AgentUse,
) -> Outcome {
    let policy = settings.default_agent_policy();
    if let AgentUse::Program(program) = agent_use
        && is_shell_or_interpreter(program)
    {
        return match policy {
            DefaultAgentPolicy::Deny => Outcome::Deny,
            _ => Outcome::Ask,
        };
    }
    match settings.decision(entry, pairing, agent_use) {
        Some(AgentDecision::Allow) => Outcome::Allow,
        Some(AgentDecision::Deny) => Outcome::Deny,
        None => match policy {
            DefaultAgentPolicy::Allow => Outcome::Allow,
            DefaultAgentPolicy::Deny => Outcome::Deny,
            DefaultAgentPolicy::Ask => Outcome::Ask,
        },
    }
}

/// The Entry and value `reference` names among the Entries the pairing reads: by UUID, or by
/// path below the Agent group, then `#` and a field. The last `#` splits them, since titles may
/// have one in them and field names rarely do.
fn resolve<'a>(
    readable: &[(&'a Entry, String)],
    reference: &str,
) -> Result<(&'a Entry, Zeroizing<String>), AgentError> {
    let (name, field) = reference
        .rsplit_once('#')
        .ok_or_else(|| AgentError::NoField(reference.to_owned()))?;
    let by_uuid = EntryId::from_hex(name);
    let mut matches = readable.iter().filter(|(entry, path)| match by_uuid {
        Some(id) => entry.id() == id,
        None => path == name,
    });
    let (entry, _) = matches
        .next()
        .ok_or_else(|| AgentError::UnknownReference(reference.to_owned()))?;
    if matches.next().is_some() {
        return Err(AgentError::AmbiguousReference(reference.to_owned()));
    }
    let value = match field {
        "password" => Some(entry.password().expose_secret()),
        "username" => Some(entry.username()),
        "url" => Some(entry.url()),
        _ => entry
            .custom_fields()
            .iter()
            .find(|custom| custom.name == field)
            .map(|custom| custom.value.expose_secret()),
    }
    .filter(|value| !value.is_empty())
    .ok_or_else(|| AgentError::UnknownField(reference.to_owned()))?;
    Ok((entry, Zeroizing::new(value.to_owned())))
}

/// The Agent pairings that can use `entry`: those whose own subgroup, or a subgroup they read,
/// holds it. The desktop app's Entry editor offers decisions for these; Gateway pairings take
/// none, so they aren't among them.
pub fn pairings_reading(vault: &Vault, entry: EntryId) -> Vec<AgentPairing> {
    let settings = AgentSettings::of(vault);
    settings
        .pairings()
        .iter()
        .filter(|pairing| {
            pairing.kind() == PairingKind::Agent
                && readable_entries(vault, &settings, pairing)
                    .iter()
                    .any(|(readable, _)| readable.id() == entry)
        })
        .cloned()
        .collect()
}

/// The Entries `pairing` reads, with their paths below the Agent group: those in its own
/// subgroup and the subgroups it may read, and in their subgroups. Agent and Gateway pairings
/// alike see exactly these, and never a note: what they are shown of each is up to them.
pub(crate) fn readable_entries<'a>(
    vault: &'a Vault,
    settings: &AgentSettings,
    pairing: &AgentPairing,
) -> Vec<(&'a Entry, String)> {
    let Some(agent_group) = settings.agent_group().and_then(|id| vault.group(id)) else {
        return Vec::new();
    };
    let mut reads = vec![pairing.group()];
    reads.extend_from_slice(pairing.readable());
    let mut readable = Vec::new();
    // The Agent group's own Entries belong to no pairing, so the walk starts below it.
    for subgroup in agent_group.groups() {
        walk(vault, subgroup, "", false, &reads, &mut readable);
    }
    readable
}

fn walk<'a>(
    vault: &'a Vault,
    group: &'a Group,
    parent_path: &str,
    parent_read: bool,
    reads: &[GroupId],
    readable: &mut Vec<(&'a Entry, String)>,
) {
    if vault.recycle_bin() == Some(group.id()) {
        return;
    }
    let path = format!("{parent_path}{}/", group.name());
    let read = parent_read || reads.contains(&group.id());
    if read {
        readable.extend(
            group
                .entries()
                .iter()
                .map(|entry| (entry, format!("{path}{}", entry.title()))),
        );
    }
    for subgroup in group.groups() {
        walk(vault, subgroup, &path, read, reads, readable);
    }
}

fn listing(entry: &Entry, path: &str) -> SecretListing {
    let password = entry.password().expose_secret();
    let mut fields: Vec<String> = [
        ("password", password),
        ("username", entry.username()),
        ("url", entry.url()),
    ]
    .into_iter()
    .filter(|(_, value)| !value.is_empty())
    .map(|(name, _)| name.to_owned())
    .collect();
    fields.extend(entry.custom_fields().iter().map(|field| field.name.clone()));
    let length = password.chars().count();
    SecretListing {
        title: entry.title().to_owned(),
        username: entry.username().to_owned(),
        url: entry.url().to_owned(),
        tags: entry.tags().to_vec(),
        fields,
        reference: SecretReference {
            uuid: entry.id().to_hex(),
            path: path.to_owned(),
        },
        hint: (length >= HINT_FROM_LENGTH)
            .then(|| password.chars().skip(length - HINT_LENGTH).collect()),
    }
}
