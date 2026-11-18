//! A Vault's Agent settings (ADR 0012), kept in its Vault file: the Agent group, and the Agent
//! pairings that reach its subgroups. A pairing's token is shown once, in the reply to the
//! command that creates it; the Vault keeps only its hash, and no other command returns it.
//!
//! The Entry editor's Agents section shows and changes the Agent access decisions remembered
//! for an Entry, by program (`keefast-agent run`) or host (`http_request`), whether allowed
//! ahead of time here or remembered at an Approver's prompt, and the hosts `http_request` may
//! send the Entry's secrets to. Both reach every host serving the Vault through the Vault file.
//! They live in the Vault's custom data, not in the Entry, so changing them makes no Entry
//! history.

use keefast_agent::{
    AgentDecision, AgentPairing, AgentSettings, AgentSettingsExt, AgentUse, PairingKind,
    pairings_reading,
};
use keefast_core::Vault;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Core;
use crate::dto::{EntryId, GroupId, VaultId};
use crate::error::{ApiError, ApiResult};

/// A subgroup of the Agent group: one pairing's own, and any others may read it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentSubgroup {
    pub id: GroupId,
    pub name: String,
}

/// What a pairing is for (ADR 0015): an Agent, reaching the Vault through MCP, or a Gateway,
/// another program's vault resolving values through `/gateway/v1/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AgentPairingKind {
    Agent,
    Gateway,
}

impl From<PairingKind> for AgentPairingKind {
    fn from(kind: PairingKind) -> Self {
        match kind {
            PairingKind::Agent => Self::Agent,
            PairingKind::Gateway => Self::Gateway,
        }
    }
}

/// An Agent or Gateway pairing, without its token.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentPairingView {
    pub id: String,
    pub name: String,
    /// Its own subgroup of the Agent group, which it always reads.
    pub group: GroupId,
    /// The other subgroups of the Agent group it reads.
    pub reads: Vec<GroupId>,
    /// Whether it creates Entries in its own subgroup, and sets their fields: for a Gateway
    /// pairing, whether it creates and changes logins there.
    pub can_create: bool,
}

impl AgentPairingView {
    fn of(pairing: &AgentPairing) -> Self {
        Self {
            id: pairing.id().as_str().to_owned(),
            name: pairing.name().to_owned(),
            group: pairing.group().into(),
            reads: pairing.readable().iter().copied().map(Into::into).collect(),
            can_create: pairing.can_create(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingsView {
    pub agent_group: Option<GroupId>,
    pub agent_group_name: Option<String>,
    /// The Agent group's subgroups, in Vault file order.
    pub subgroups: Vec<AgentSubgroup>,
    /// The Agent pairings, without revoked ones, in the order they were made.
    pub pairings: Vec<AgentPairingView>,
    /// The Gateway pairings, listed apart: without revoked ones, in the order they were made.
    pub gateways: Vec<AgentPairingView>,
}

/// A pairing just created, with its bearer token: the only time anyone sees it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAgentPairing {
    pub pairing: AgentPairingView,
    pub token: String,
}

fn view(vault: &Vault) -> AgentSettingsView {
    let settings = AgentSettings::of(vault);
    let agent_group = settings.agent_group().and_then(|id| vault.group(id));
    AgentSettingsView {
        agent_group: agent_group.map(|group| group.id().into()),
        agent_group_name: agent_group.map(|group| group.name().to_owned()),
        subgroups: agent_group
            .map(|group| {
                group
                    .groups()
                    .iter()
                    .map(|child| AgentSubgroup {
                        id: child.id().into(),
                        name: child.name().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        pairings: of_kind(&settings, PairingKind::Agent),
        gateways: of_kind(&settings, PairingKind::Gateway),
    }
}

fn of_kind(settings: &AgentSettings, kind: PairingKind) -> Vec<AgentPairingView> {
    settings
        .pairings()
        .iter()
        .filter(|pairing| pairing.kind() == kind)
        .map(AgentPairingView::of)
        .collect()
}

/// The pairing whose id is `id`, among the Vault's.
fn pairing(settings: &AgentSettings, id: &str) -> ApiResult<AgentPairing> {
    settings
        .pairings()
        .iter()
        .find(|pairing| pairing.id().as_str() == id)
        .cloned()
        .ok_or_else(|| ApiError::invalid("This Agent pairing was revoked."))
}

fn groups(ids: &[GroupId]) -> ApiResult<Vec<keefast_core::GroupId>> {
    ids.iter().map(GroupId::resolve).collect()
}

#[tauri::command]
#[specta::specta]
pub async fn get_agent_settings(core: Core<'_>, vault: VaultId) -> ApiResult<AgentSettingsView> {
    core.read(|workspace| {
        let id = vault.resolve_open(workspace)?;
        let open = workspace
            .readable_vault(id)
            .ok_or_else(ApiError::vault_not_found)?;
        Ok(view(open.vault()))
    })
}

/// Makes the Group the Vault's Agent group: Agents reach only Entries in its subgroups.
#[tauri::command]
#[specta::specta]
pub async fn set_agent_group(core: Core<'_>, vault: VaultId, group: GroupId) -> ApiResult<()> {
    let (vault, group) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, group.resolve()?)))?;
    core.update(|workspace| workspace.set_agent_group(vault, group))?;
    core.autosave();
    Ok(())
}

/// Pairs an Agent called `name`, with its own subgroup of the Agent group by that name, which
/// also reads the subgroups `reads`, and creates Entries in its subgroup if `can_create`. The
/// reply has its token, which no command returns again.
#[tauri::command]
#[specta::specta]
pub async fn create_agent_pairing(
    core: Core<'_>,
    vault: VaultId,
    name: String,
    reads: Vec<GroupId>,
    can_create: bool,
) -> ApiResult<CreatedAgentPairing> {
    create_pairing(core, vault, PairingKind::Agent, name, reads, can_create)
}

/// Pairs a Gateway called `name` (ADR 0015), such as agentes-chat, with its own subgroup of the
/// Agent group by that name, which also reads the subgroups `reads`, and creates and changes
/// logins in its own subgroup if `can_write`. The reply has its token, which no command
/// returns again. Only this makes a Gateway pairing: OAuth never does.
#[tauri::command]
#[specta::specta]
pub async fn create_gateway_pairing(
    core: Core<'_>,
    vault: VaultId,
    name: String,
    reads: Vec<GroupId>,
    can_write: bool,
) -> ApiResult<CreatedAgentPairing> {
    create_pairing(core, vault, PairingKind::Gateway, name, reads, can_write)
}

/// Pairs a pairing of `kind`, for [`create_agent_pairing`] and [`create_gateway_pairing`].
fn create_pairing(
    core: Core<'_>,
    vault: VaultId,
    kind: PairingKind,
    name: String,
    reads: Vec<GroupId>,
    can_create: bool,
) -> ApiResult<CreatedAgentPairing> {
    let reads = groups(&reads)?;
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let created =
        core.update(|workspace| workspace.create_pairing(vault, kind, &name, reads, can_create))?;
    core.autosave();
    Ok(CreatedAgentPairing {
        pairing: AgentPairingView::of(&created.pairing),
        token: created.token.as_str().to_owned(),
    })
}

/// Sets which other subgroups of the Agent group a pairing reads, and whether it creates
/// Entries in its own subgroup.
#[tauri::command]
#[specta::specta]
pub async fn set_agent_pairing_access(
    core: Core<'_>,
    vault: VaultId,
    pairing: String,
    reads: Vec<GroupId>,
    can_create: bool,
) -> ApiResult<()> {
    let reads = groups(&reads)?;
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.update(|workspace| {
        let settings = workspace
            .agent_settings(vault)
            .ok_or_else(ApiError::vault_not_found)?;
        let pairing = self::pairing(&settings, &pairing)?;
        workspace
            .set_agent_pairing_access(vault, pairing.id(), reads, can_create)
            .map_err(ApiError::from)
    })?;
    core.autosave();
    Ok(())
}

/// Revokes an Agent or Gateway pairing: its token is refused from now on. Its subgroup and
/// Entries stay. Ask the user first.
#[tauri::command]
#[specta::specta]
pub async fn revoke_agent_pairing(
    core: Core<'_>,
    vault: VaultId,
    pairing: String,
) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.update(|workspace| {
        let settings = workspace
            .agent_settings(vault)
            .ok_or_else(ApiError::vault_not_found)?;
        let pairing = self::pairing(&settings, &pairing)?;
        workspace
            .revoke_agent_pairing(vault, pairing.id())
            .map_err(ApiError::from)
    })?;
    core.autosave();
    Ok(())
}

/// What an Agent access decision is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AgentUseKind {
    /// A program `keefast-agent run` starts, by its executable's name.
    Program,
    /// A host an `http_request` goes to.
    Host,
}

impl AgentUseKind {
    fn agent_use(self, name: String) -> AgentUse {
        match self {
            Self::Program => AgentUse::Program(name),
            Self::Host => AgentUse::Host(name),
        }
    }
}

/// A remembered Agent access decision: an Agent pairing using an Entry with a program, or at
/// a host.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentDecisionView {
    pub pairing: String,
    pub pairing_name: String,
    pub kind: AgentUseKind,
    /// The program's executable name, such as `gh`, or the host, such as `api.github.com`.
    pub name: String,
    pub allow: bool,
}

/// What the Entry editor's Agents section shows for an Entry.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryAgentAccess {
    /// The Agent pairings that can use the Entry, from their own subgroup or one they read.
    /// Empty when the Entry is outside the Agent group.
    pub pairings: Vec<AgentPairingView>,
    /// Its remembered decisions, by pairing name, then programs before hosts, then name.
    pub decisions: Vec<AgentDecisionView>,
    /// The hosts `http_request` may send its secrets to, as kept: host names, IP addresses, or
    /// `*.` and a domain for its subdomains.
    pub allowed_hosts: Vec<String>,
    /// Whether it is marked "Agents may reveal": `reveal_secret` may hand its values to a
    /// model, when the user allows it at the prompt, every time.
    pub agents_may_reveal: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn get_entry_agent_access(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
) -> ApiResult<EntryAgentAccess> {
    let entry = entry.resolve()?;
    core.read(|workspace| {
        let id = vault.resolve_open(workspace)?;
        let open = workspace
            .readable_vault(id)
            .ok_or_else(ApiError::vault_not_found)?
            .vault();
        let settings = AgentSettings::of(open);
        let mut decisions: Vec<AgentDecisionView> = settings
            .decisions_on(entry)
            .filter_map(|decision| {
                let (kind, name) = match &decision.agent_use {
                    AgentUse::Program(program) => (AgentUseKind::Program, program),
                    AgentUse::Host(host) => (AgentUseKind::Host, host),
                };
                // A revoked pairing's decisions no longer matter.
                let pairing = settings.pairing(&decision.pairing)?;
                Some(AgentDecisionView {
                    pairing: pairing.id().as_str().to_owned(),
                    pairing_name: pairing.name().to_owned(),
                    kind,
                    name: name.clone(),
                    allow: decision.decision == AgentDecision::Allow,
                })
            })
            .collect();
        decisions.sort_by(|a, b| {
            (a.pairing_name.as_str(), a.kind, a.name.as_str()).cmp(&(
                b.pairing_name.as_str(),
                b.kind,
                b.name.as_str(),
            ))
        });
        Ok(EntryAgentAccess {
            pairings: pairings_reading(open, entry)
                .iter()
                .map(AgentPairingView::of)
                .collect(),
            decisions,
            allowed_hosts: settings.allowed_hosts(entry).to_vec(),
            agents_may_reveal: settings.agents_may_reveal(entry),
        })
    })
}

/// Remembers that the Agent pairing `pairing` may (`allow`) or may not use the Entry with a
/// program, by its executable's name, or at a host. Shells and interpreters are refused: they
/// ask every time.
#[tauri::command]
#[specta::specta]
pub async fn remember_agent_decision(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    pairing: String,
    kind: AgentUseKind,
    name: String,
    allow: bool,
) -> ApiResult<()> {
    let entry = entry.resolve()?;
    let name = match kind {
        AgentUseKind::Program => {
            let program = keefast_agent::programs::program_name(name.trim());
            if program.is_empty() {
                return Err(ApiError::invalid("Name the program, such as gh."));
            }
            program
        }
        AgentUseKind::Host => match keefast_agent::hosts::normalize_allowed_host(&name) {
            // A decision is for one host a request goes to, never a wildcard.
            Some(host) if !host.starts_with("*.") => host,
            _ => return Err(ApiError::invalid("Name the host, such as api.github.com.")),
        },
    };
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.update(|workspace| {
        let settings = workspace
            .agent_settings(vault)
            .ok_or_else(ApiError::vault_not_found)?;
        let pairing = self::pairing(&settings, &pairing)?;
        let decision = if allow {
            AgentDecision::Allow
        } else {
            AgentDecision::Deny
        };
        workspace
            .remember_agent_decision(vault, entry, pairing.id(), &kind.agent_use(name), decision)
            .map_err(ApiError::from)
    })?;
    core.autosave();
    Ok(())
}

/// Forgets a remembered decision, so the use follows the Default agent policy again.
#[tauri::command]
#[specta::specta]
pub async fn forget_agent_decision(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    pairing: String,
    kind: AgentUseKind,
    name: String,
) -> ApiResult<()> {
    let entry = entry.resolve()?;
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.update(|workspace| {
        let settings = workspace
            .agent_settings(vault)
            .ok_or_else(ApiError::vault_not_found)?;
        let pairing = self::pairing(&settings, &pairing)?;
        workspace
            .forget_agent_decision(vault, entry, pairing.id(), &kind.agent_use(name))
            .map_err(ApiError::from)
    })?;
    core.autosave();
    Ok(())
}

/// Sets the hosts `http_request` may send the Entry's secrets to, and returns them as kept:
/// lower case, without schemes or paths, without repeats. Refused whole if one isn't a host.
#[tauri::command]
#[specta::specta]
pub async fn set_entry_allowed_hosts(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    hosts: Vec<String>,
) -> ApiResult<Vec<String>> {
    let entry = entry.resolve()?;
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let kept = core.update(|workspace| {
        workspace
            .set_agent_allowed_hosts(vault, entry, &hosts)
            .map_err(ApiError::from)
    })?;
    core.autosave();
    Ok(kept)
}

/// Marks the Entry "Agents may reveal", or takes the mark off. Each reveal still asks.
#[tauri::command]
#[specta::specta]
pub async fn set_entry_agents_may_reveal(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    may_reveal: bool,
) -> ApiResult<()> {
    let entry = entry.resolve()?;
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.update(|workspace| {
        workspace
            .set_agents_may_reveal(vault, entry, may_reveal)
            .map_err(ApiError::from)
    })?;
    core.autosave();
    Ok(())
}

/// What Agents on this computer did with the Vault's secrets, newest first: the Vault's own
/// audit log, which this desktop keeps as its in-app host (ADR 0012 "Audit").
#[tauri::command]
#[specta::specta]
pub async fn get_vault_agent_activity(
    core: Core<'_>,
    vault: VaultId,
) -> ApiResult<Vec<super::agent_hosts::AgentActivityRecord>> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let core = std::sync::Arc::clone(&core);
    let records = super::blocking(move || {
        crate::in_app_agents::activity(&core, vault, keefast_agent::approver::ACTIVITY_LINES)
    })
    .await??;
    Ok(records.into_iter().rev().map(Into::into).collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use keefast_agent::AgentSettingsExt;
    use keefast_workspace::{LocalStorage, UnlockSecrets, VaultLocation, Workspace};

    use super::view;

    /// A Vault whose Agent group `Agents` has the subgroup `shared`, with the Agent pairing
    /// `claude` and the Gateway pairing `agentes`, which reads `shared` and writes.
    fn vault_with_a_gateway() -> (Workspace, keefast_workspace::VaultId, tempfile::TempDir) {
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
        let mut workspace = Workspace::new(Arc::new(LocalStorage));
        let vault = workspace
            .open_vault(
                &VaultLocation::Local(path),
                &UnlockSecrets::password("fixture-password"),
            )
            .unwrap();
        let root = workspace.vault(vault).unwrap().vault().root().id();
        let agents = workspace.create_group(vault, root, "Agents").unwrap();
        let shared = workspace.create_group(vault, agents, "shared").unwrap();
        workspace.set_agent_group(vault, agents).unwrap();
        workspace
            .create_agent_pairing(vault, "claude", vec![], false)
            .unwrap();
        workspace
            .create_gateway_pairing(vault, "agentes", vec![shared], true)
            .unwrap();
        (workspace, vault, dir)
    }

    #[test]
    fn gateway_pairings_are_listed_apart_with_what_they_read_and_whether_they_write() {
        let (workspace, vault, _dir) = vault_with_a_gateway();

        let shown = view(workspace.readable_vault(vault).unwrap().vault());

        let agents: Vec<&str> = shown.pairings.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(agents, ["claude"]);
        let [gateway] = shown.gateways.as_slice() else {
            panic!("expected one Gateway pairing, got {:?}", shown.gateways);
        };
        assert_eq!(gateway.name, "agentes");
        let id_of = |name: &str| {
            shown
                .subgroups
                .iter()
                .find(|group| group.name == name)
                .map(|group| group.id.clone())
                .unwrap()
        };
        assert_eq!(gateway.group, id_of("agentes"));
        assert_eq!(gateway.reads, [id_of("shared")]);
        assert!(gateway.can_create);
    }
}
