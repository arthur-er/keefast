//! A Vault's Agent settings (ADR 0012), kept in its Vault file: the Agent group, and the Agent
//! pairings that reach its subgroups. A pairing's token is shown once, in the reply to the
//! command that creates it; the Vault keeps only its hash, and no other command returns it.

use keefast_agent::{AgentPairing, AgentSettings, AgentSettingsExt};
use keefast_core::Vault;
use serde::Serialize;
use specta::Type;

use super::Core;
use crate::dto::{GroupId, VaultId};
use crate::error::{ApiError, ApiResult};

/// A subgroup of the Agent group: one pairing's own, and any others may read it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentSubgroup {
    pub id: GroupId,
    pub name: String,
}

/// An Agent pairing, without its token.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentPairingView {
    pub id: String,
    pub name: String,
    /// Its own subgroup of the Agent group, which it always reads.
    pub group: GroupId,
    /// The other subgroups of the Agent group it reads.
    pub reads: Vec<GroupId>,
}

impl AgentPairingView {
    fn of(pairing: &AgentPairing) -> Self {
        Self {
            id: pairing.id().as_str().to_owned(),
            name: pairing.name().to_owned(),
            group: pairing.group().into(),
            reads: pairing.readable().iter().copied().map(Into::into).collect(),
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
        pairings: settings
            .pairings()
            .iter()
            .map(AgentPairingView::of)
            .collect(),
    }
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
/// also reads the subgroups `reads`. The reply has its token, which no command returns again.
#[tauri::command]
#[specta::specta]
pub async fn create_agent_pairing(
    core: Core<'_>,
    vault: VaultId,
    name: String,
    reads: Vec<GroupId>,
) -> ApiResult<CreatedAgentPairing> {
    let reads = groups(&reads)?;
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let created =
        core.update(|workspace| workspace.create_agent_pairing(vault, &name, reads, false))?;
    core.autosave();
    Ok(CreatedAgentPairing {
        pairing: AgentPairingView::of(&created.pairing),
        token: created.token.as_str().to_owned(),
    })
}

/// Sets which other subgroups of the Agent group a pairing reads.
#[tauri::command]
#[specta::specta]
pub async fn set_agent_pairing_reads(
    core: Core<'_>,
    vault: VaultId,
    pairing: String,
    reads: Vec<GroupId>,
) -> ApiResult<()> {
    let reads = groups(&reads)?;
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.update(|workspace| {
        let settings = workspace
            .agent_settings(vault)
            .ok_or_else(ApiError::vault_not_found)?;
        let pairing = self::pairing(&settings, &pairing)?;
        workspace
            .set_agent_pairing_access(vault, pairing.id(), reads, pairing.can_create())
            .map_err(ApiError::from)
    })?;
    core.autosave();
    Ok(())
}

/// Revokes an Agent pairing: its token is refused from now on. Its subgroup and Entries stay.
/// Ask the user first.
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
