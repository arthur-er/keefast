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

use keefast_core::{Group, GroupId, Vault};
use keefast_workspace::{VaultId, Workspace};
use secrecy::ExposeSecret;
use serde::Serialize;

use crate::{AgentPairingId, AgentSettings};

/// A password at least this long gets a hint of its last four characters; a shorter one would
/// give too much of itself away.
const HINT_FROM_LENGTH: usize = 12;
const HINT_LENGTH: usize = 4;

/// What an Agent asks of keefast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRequest {
    /// The secrets the pairing can read, without their values.
    ListSecrets,
}

/// keefast's answer to an [`AgentRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentReply {
    Secrets(Vec<SecretListing>),
}

impl AgentReply {
    /// The secrets listed in reply to [`AgentRequest::ListSecrets`].
    pub fn into_secrets(self) -> Vec<SecretListing> {
        match self {
            Self::Secrets(secrets) => secrets,
        }
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

/// One Agent pairing's requests.
#[derive(Debug, Clone)]
pub struct AgentSession {
    pairing: AgentPairingId,
}

impl AgentSession {
    /// A session for the Agent pairing `pairing`.
    pub fn new(pairing: AgentPairingId) -> Self {
        Self { pairing }
    }

    pub fn pairing(&self) -> &AgentPairingId {
        &self.pairing
    }

    /// Handles `request` against `vault` in `workspace`. It never notes activity: Agents don't
    /// keep a Vault from locking when its user is idle.
    pub fn handle(
        &self,
        workspace: &Workspace,
        vault: VaultId,
        request: AgentRequest,
    ) -> Result<AgentReply, AgentError> {
        let vault = workspace
            .readable_vault(vault)
            .ok_or(AgentError::Locked)?
            .vault();
        let settings = AgentSettings::of(vault);
        let pairing = settings
            .pairing(&self.pairing)
            .ok_or(AgentError::NotPaired)?;
        match request {
            AgentRequest::ListSecrets => {
                let mut reads = vec![pairing.group()];
                reads.extend_from_slice(pairing.readable());
                Ok(AgentReply::Secrets(list_secrets(vault, &settings, &reads)))
            }
        }
    }
}

/// The Entries in the subgroups of the Agent group listed in `reads`, and in their subgroups.
fn list_secrets(vault: &Vault, settings: &AgentSettings, reads: &[GroupId]) -> Vec<SecretListing> {
    let Some(agent_group) = settings.agent_group().and_then(|id| vault.group(id)) else {
        return Vec::new();
    };
    let mut listed = Vec::new();
    // The Agent group's own Entries belong to no pairing, so the walk starts below it.
    for subgroup in agent_group.groups() {
        walk(vault, subgroup, "", false, reads, &mut listed);
    }
    listed
}

fn walk(
    vault: &Vault,
    group: &Group,
    parent_path: &str,
    parent_read: bool,
    reads: &[GroupId],
    listed: &mut Vec<SecretListing>,
) {
    if vault.recycle_bin() == Some(group.id()) {
        return;
    }
    let path = format!("{parent_path}{}/", group.name());
    let read = parent_read || reads.contains(&group.id());
    if read {
        listed.extend(group.entries().iter().map(|entry| {
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
                    path: format!("{path}{}", entry.title()),
                },
                hint: (length >= HINT_FROM_LENGTH)
                    .then(|| password.chars().skip(length - HINT_LENGTH).collect()),
            }
        }));
    }
    for subgroup in group.groups() {
        walk(vault, subgroup, &path, read, reads, listed);
    }
}
