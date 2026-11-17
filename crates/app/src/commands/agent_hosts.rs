//! Settings → Agent hosts, and the approval prompt (ADR 0012 "Approvals"): the Headless hosts
//! this desktop keefast is an Approver for, and its answers to their prompts. A host's token is
//! given once, when it is added, and kept in the OS secret store; no command returns it.

use serde::Serialize;
use specta::Type;

use super::{Core, blocking};
use crate::agent_hosts::{self, Connection, HostView, WaitingConsent, WaitingPrompt};
use crate::error::ApiResult;

/// How an Agent host's prompt stream stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AgentHostState {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentHostView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub state: AgentHostState,
    /// Why it is disconnected; keefast connects again by itself.
    pub problem: Option<String>,
}

impl From<HostView> for AgentHostView {
    fn from(host: HostView) -> Self {
        let (state, problem) = match host.connection {
            Connection::Connecting => (AgentHostState::Connecting, None),
            Connection::Connected => (AgentHostState::Connected, None),
            Connection::Disconnected(why) => (AgentHostState::Disconnected, Some(why)),
        };
        Self {
            id: host.id,
            name: host.name,
            url: host.url,
            state,
            problem,
        }
    }
}

/// An Agent's use of a secret that a Headless host asks this Approver about: the oldest one
/// waiting.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingAgentPrompt {
    pub id: String,
    /// The Agent host asking, by its id and the name the user gave it.
    pub host: String,
    pub host_name: String,
    /// The Agent pairing whose Agent asks.
    pub pairing: String,
    pub entry_title: String,
    /// The Secret reference the Agent named the Entry by.
    pub reference: String,
    /// The program a remembered answer is remembered for.
    pub program: String,
    /// The program and its arguments, whole.
    pub command: Vec<String>,
    /// Whether the answer may be remembered: never for shells and interpreters.
    pub can_remember: bool,
    /// When the host denies it unanswered, in seconds since 1970 UTC.
    pub expires: f64,
    /// How many prompts wait besides this one.
    pub more: u32,
}

impl PendingAgentPrompt {
    pub fn of(waiting: &WaitingPrompt, more: usize) -> Self {
        let prompt = &waiting.prompt;
        Self {
            id: waiting.id.clone(),
            host: waiting.host.clone(),
            host_name: waiting.host_name.clone(),
            pairing: prompt.pairing.clone(),
            entry_title: prompt.entry_title.clone(),
            reference: prompt.reference.clone(),
            program: prompt.program.clone(),
            command: prompt.command.clone(),
            can_remember: prompt.can_remember,
            expires: waiting
                .expires
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |since| since.as_secs_f64()),
            more: u32::try_from(more).unwrap_or(u32::MAX),
        }
    }
}

/// An MCP client pairing with a Headless host by OAuth, which this Approver may let in by
/// typing the code its consent page shows: the oldest one waiting, once no prompt does.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingAgentConsent {
    pub id: String,
    /// The Agent host asking, by its id and the name the user gave it.
    pub host: String,
    pub host_name: String,
    /// The name the client registered with: its own claim.
    pub client: String,
    /// Where the browser goes back to once approved, by its host.
    pub redirect: String,
    /// The Agent group's subgroups the new pairing may also read.
    pub subgroups: Vec<String>,
    /// The Agent pairing the client has already, which approving lets in again.
    pub paired_as: Option<String>,
    /// When the host forgets it, in seconds since 1970 UTC.
    pub expires: f64,
    /// How many consents wait besides this one.
    pub more: u32,
}

impl PendingAgentConsent {
    pub fn of(waiting: &WaitingConsent, more: usize) -> Self {
        let consent = &waiting.consent;
        Self {
            id: waiting.id.clone(),
            host: waiting.host.clone(),
            host_name: waiting.host_name.clone(),
            client: consent.client.clone(),
            redirect: consent.redirect.clone(),
            subgroups: consent.subgroups.clone(),
            paired_as: consent.paired_as.clone(),
            expires: waiting
                .expires
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |since| since.as_secs_f64()),
            more: u32::try_from(more).unwrap_or(u32::MAX),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_agent_hosts(core: Core<'_>) -> ApiResult<Vec<AgentHostView>> {
    Ok(core
        .agent_hosts()
        .views()
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Becomes an Approver for the Agent host at `url`, with the token `keefast-agent pairing
/// approver` printed there. An empty name names it by its address.
#[tauri::command]
#[specta::specta]
pub async fn add_agent_host(
    core: Core<'_>,
    name: String,
    url: String,
    token: String,
) -> ApiResult<AgentHostView> {
    agent_hosts::add(&core, &name, &url, &token).map(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_agent_host(core: Core<'_>, host: String) -> ApiResult<()> {
    agent_hosts::remove(&core, &host)
}

/// Answers the prompt `id` from `host`: allow or deny, and with `remember`, as an Agent access
/// decision saved in the Vault.
#[tauri::command]
#[specta::specta]
pub async fn answer_agent_prompt(
    core: Core<'_>,
    host: String,
    id: String,
    allow: bool,
    remember: bool,
) -> ApiResult<()> {
    let core = std::sync::Arc::clone(&core);
    blocking(move || agent_hosts::answer(&core, &host, &id, allow, remember)).await?
}

/// Answers the OAuth consent `id` from `host`. Approving needs the `code` its consent page
/// shows, and makes the Agent pairing `name` (or as the client calls itself, if empty), reading
/// the subgroups `reads` too and creating Entries if `can_create`. Denying sends the client
/// away.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn answer_agent_consent(
    core: Core<'_>,
    host: String,
    id: String,
    approve: bool,
    code: String,
    name: String,
    reads: Vec<String>,
    can_create: bool,
) -> ApiResult<()> {
    let core = std::sync::Arc::clone(&core);
    let answer = keefast_agent::ConsentAnswer {
        approve,
        code,
        name,
        reads,
        can_create,
    };
    blocking(move || agent_hosts::answer_agent_consent(&core, &host, &id, &answer)).await?
}
