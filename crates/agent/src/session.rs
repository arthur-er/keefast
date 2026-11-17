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

use std::sync::Arc;

use keefast_core::{Entry, EntryId, Group, GroupId, Vault};
use keefast_workspace::{VaultId, Workspace};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::approver::{AgentPrompt, Approval, Approver, NoApprover};
use crate::audit::{AuditEntry, AuditOutcome, AuditRecord, AuditSink};
use crate::programs::{is_shell_or_interpreter, program_name};
use crate::{
    AgentDecision, AgentPairing, AgentPairingId, AgentSettings, AgentSettingsExt, AgentUse,
    DefaultAgentPolicy,
};

/// A password at least this long gets a hint of its last four characters; a shorter one would
/// give too much of itself away.
const HINT_FROM_LENGTH: usize = 12;
const HINT_LENGTH: usize = 4;

/// What an Agent asks of keefast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRequest {
    /// The secrets the pairing can read, without their values.
    ListSecrets,
    /// The values of secrets, for `keefast-agent run` to put in a program's environment.
    UseSecrets(UseSecrets),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentReply {
    Secrets(Vec<SecretListing>),
    /// One value per reference asked for, in the same order.
    Values(Vec<SecretValue>),
}

impl AgentReply {
    /// The secrets listed in reply to [`AgentRequest::ListSecrets`]; empty for another reply.
    pub fn into_secrets(self) -> Vec<SecretListing> {
        match self {
            Self::Secrets(secrets) => secrets,
            Self::Values(_) => Vec::new(),
        }
    }

    /// The values in reply to [`AgentRequest::UseSecrets`]; empty for another reply.
    pub fn into_values(self) -> Vec<SecretValue> {
        match self {
            Self::Values(values) => values,
            Self::Secrets(_) => Vec::new(),
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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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
}

/// A secret as an Agent sees it in a list: what helps it pick the right one, and how to name
/// it, but never a value and never the Entry's notes. All of it is text the Agent didn't
/// write, which hosts mark as untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The two ways an Agent can name an Entry in a Secret reference, before `#<field>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

/// One Agent pairing's requests.
#[derive(Clone)]
pub struct AgentSession {
    pairing: AgentPairingId,
    audit: Arc<dyn AuditSink>,
    approver: Arc<dyn Approver>,
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
        }
    }

    /// Asks `approver` about uses no decision or policy answers.
    pub fn with_approver(mut self, approver: Arc<dyn Approver>) -> Self {
        self.approver = approver;
        self
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
        }
    }

    fn list_secrets(&self, workspace: &Workspace, id: VaultId) -> Result<AgentReply, AgentError> {
        let vault = workspace
            .readable_vault(id)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let pairing = settings
            .pairing(&self.pairing)
            .ok_or(AgentError::NotPaired)?;
        self.audit
            .record(self.record(workspace, pairing, "list", AuditOutcome::Allowed));
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
        let mut remember = Vec::new();
        for asking in asking {
            let approval = self.approver.ask(&asking.prompt);
            let (reason, refusal) = match approval {
                Approval::Allow { remember: wants } => {
                    if wants && asking.prompt.can_remember {
                        remember.push((asking.entry, AgentDecision::Allow));
                        ("allowed and remembered at a prompt", None)
                    } else {
                        ("allowed once at a prompt", None)
                    }
                }
                Approval::Deny { remember: wants } => {
                    if wants && asking.prompt.can_remember {
                        remember.push((asking.entry, AgentDecision::Deny));
                    }
                    let reason = if wants && asking.prompt.can_remember {
                        "denied and remembered at a prompt"
                    } else {
                        "denied once at a prompt"
                    };
                    (
                        reason,
                        Some(AgentError::Denied {
                            reference: asking.prompt.reference.clone(),
                            program: program.clone(),
                        }),
                    )
                }
                Approval::Unanswered => (
                    "nobody answered the prompt in time",
                    Some(AgentError::Unanswered {
                        reference: asking.prompt.reference.clone(),
                        program: program.clone(),
                    }),
                ),
                Approval::NoApprover => (
                    "no Approver to ask",
                    Some(AgentError::NotAllowed {
                        reference: asking.prompt.reference.clone(),
                        program: program.clone(),
                    }),
                ),
            };
            let time = vault
                .read(|workspace, _| workspace.now().unix_seconds())
                .unwrap_or(first_time);
            self.audit.record(AuditRecord {
                time,
                pairing: asking.prompt.pairing.clone(),
                pairing_id: self.pairing.as_str().to_owned(),
                action: "use".to_owned(),
                entry: Some(AuditEntry {
                    uuid: asking.prompt.entry_uuid.clone(),
                    title: asking.prompt.entry_title.clone(),
                }),
                program: Some(program.clone()),
                outcome: if refusal.is_some() {
                    AuditOutcome::Denied
                } else {
                    AuditOutcome::Allowed
                },
                reason: Some(reason.to_owned()),
            });
            if let Some(refusal) = refusal {
                if !remember.is_empty() {
                    self.remember(vault, &program, &remember);
                }
                return Err(refusal);
            }
        }
        if !remember.is_empty() {
            self.remember(vault, &program, &remember);
        }
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
        let pairing = settings
            .pairing(&self.pairing)
            .ok_or(AgentError::NotPaired)?;
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
            let refused = match decide(&settings, entry.id(), pairing.id(), program) {
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
            let mut record = self.record(workspace, pairing, "use", outcome);
            record.entry = Some(AuditEntry {
                uuid: entry.id().to_hex(),
                title: entry.title().to_owned(),
            });
            record.program = Some(program.to_owned());
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
        program: &str,
        answers: &[(EntryId, AgentDecision)],
    ) {
        let agent_use = AgentUse::Program(program.to_owned());
        vault.edit(|workspace, id| {
            for (entry, decision) in answers {
                // Fails only for a revoked pairing or a deleted Entry, which the use then hears.
                let _ = workspace.remember_agent_decision(
                    id,
                    *entry,
                    &self.pairing,
                    &agent_use,
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
        let pairing = settings
            .pairing(&self.pairing)
            .ok_or(AgentError::NotPaired)?;
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
        action: &str,
        outcome: AuditOutcome,
    ) -> AuditRecord {
        AuditRecord {
            time: workspace.now().unix_seconds(),
            pairing: pairing.name().to_owned(),
            pairing_id: pairing.id().as_str().to_owned(),
            action: action.to_owned(),
            entry: None,
            program: None,
            outcome,
            reason: None,
        }
    }
}

enum Outcome {
    Allow,
    Deny,
    Ask,
}

/// What the Vault answers `pairing` using `entry` with `program`: its Agent access decision, or
/// else its Default agent policy. Shells and interpreters ask every time, whatever was
/// remembered or the policy allows, unless the policy denies everything.
fn decide(
    settings: &AgentSettings,
    entry: EntryId,
    pairing: &AgentPairingId,
    program: &str,
) -> Outcome {
    let policy = settings.default_agent_policy();
    if is_shell_or_interpreter(program) {
        return match policy {
            DefaultAgentPolicy::Deny => Outcome::Deny,
            _ => Outcome::Ask,
        };
    }
    match settings.decision(entry, pairing, &AgentUse::Program(program.to_owned())) {
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
/// holds it. The desktop app's Entry editor offers decisions for these.
pub fn pairings_reading(vault: &Vault, entry: EntryId) -> Vec<AgentPairing> {
    let settings = AgentSettings::of(vault);
    settings
        .pairings()
        .iter()
        .filter(|pairing| {
            readable_entries(vault, &settings, pairing)
                .iter()
                .any(|(readable, _)| readable.id() == entry)
        })
        .cloned()
        .collect()
}

/// The Entries `pairing` reads, with their paths below the Agent group: those in its own
/// subgroup and the subgroups it may read, and in their subgroups.
fn readable_entries<'a>(
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
