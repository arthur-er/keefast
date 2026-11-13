//! A Vault's Groups: the tree, and creating, renaming, moving and deleting them.

use keefast_core::{Group, Vault};
use keefast_workspace as ws;
use serde::Serialize;
use specta::Type;

use super::Core;
use crate::dto::{GroupId, IconView, VaultId, group_icon_view};
use crate::error::{ApiError, ApiResult};

/// A Group and the Groups in it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupNode {
    pub id: GroupId,
    /// The top Group's name is the Vault's.
    pub name: String,
    /// Its standard icon (a folder unless given another) or custom icon, drawn as an Entry's.
    pub icon: IconView,
    /// The Entries right in this Group.
    pub entry_count: u32,
    /// The Entries in this Group and every Group below it.
    pub total_entries: u32,
    pub is_recycle_bin: bool,
    /// Whether it is the Recycle Bin or in it.
    pub in_recycle_bin: bool,
    /// Whether captured logins are saved here.
    pub is_browser_group: bool,
    pub children: Vec<GroupNode>,
}

fn node(id: ws::VaultId, vault: &Vault, group: &Group, in_bin: bool) -> GroupNode {
    let is_bin = vault.recycle_bin() == Some(group.id());
    let in_bin = in_bin || is_bin;
    GroupNode {
        id: group.id().into(),
        name: group.name().to_owned(),
        icon: group_icon_view(id, group.icon(), vault),
        entry_count: group.entries().len() as u32,
        total_entries: group.all_entries().count() as u32,
        is_recycle_bin: is_bin,
        in_recycle_bin: in_bin,
        is_browser_group: vault.browser_group() == Some(group.id()),
        children: group
            .groups()
            .iter()
            .map(|child| node(id, vault, child, in_bin))
            .collect(),
    }
}

/// The Vault's Groups, from its top Group down.
#[tauri::command]
#[specta::specta]
pub async fn list_groups(core: Core<'_>, vault: VaultId) -> ApiResult<GroupNode> {
    core.read(|workspace| {
        let id = vault.resolve_open(workspace)?;
        let open = workspace
            .readable_vault(id)
            .ok_or_else(ApiError::vault_not_found)?;
        Ok(node(id, open.vault(), open.vault().root(), false))
    })
}

/// Makes a new Group in `parent`, and returns it.
#[tauri::command]
#[specta::specta]
pub async fn create_group(
    core: Core<'_>,
    vault: VaultId,
    parent: GroupId,
    name: String,
) -> ApiResult<GroupId> {
    let (vault, parent) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, parent.resolve()?)))?;
    core.edit(|workspace| workspace.create_group(vault, parent, &name))
        .map(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_group(
    core: Core<'_>,
    vault: VaultId,
    group: GroupId,
    name: String,
) -> ApiResult<()> {
    let (vault, group) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, group.resolve()?)))?;
    core.edit(|workspace| workspace.rename_group(vault, group, &name))
}

/// Moves a Group, and everything in it, into another Group of its Vault.
#[tauri::command]
#[specta::specta]
pub async fn move_group(
    core: Core<'_>,
    vault: VaultId,
    group: GroupId,
    to: GroupId,
) -> ApiResult<()> {
    let (vault, group, to) = core.read(|workspace| {
        Ok::<_, ApiError>((
            vault.resolve_open(workspace)?,
            group.resolve()?,
            to.resolve()?,
        ))
    })?;
    core.edit(|workspace| workspace.move_group(vault, group, to))
}

/// Moves a Group to the Recycle Bin, or deletes it for good when the Vault has the Recycle Bin
/// disabled or the Group is in it already. Ask first when `deletesGroupForGood` says so.
#[tauri::command]
#[specta::specta]
pub async fn delete_group(core: Core<'_>, vault: VaultId, group: GroupId) -> ApiResult<()> {
    let (vault, group) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, group.resolve()?)))?;
    core.edit(|workspace| workspace.delete_group(vault, group))
}

/// Whether deleting the Group deletes it for good rather than moving it to the Recycle Bin.
#[tauri::command]
#[specta::specta]
pub async fn deletes_group_for_good(
    core: Core<'_>,
    vault: VaultId,
    group: GroupId,
) -> ApiResult<bool> {
    core.read(|workspace| {
        let id = vault.resolve_open(workspace)?;
        let open = workspace
            .readable_vault(id)
            .ok_or_else(ApiError::vault_not_found)?;
        open.vault()
            .deletes_group_for_good(group.resolve()?)
            .map_err(|error| keefast_workspace::EditVaultError::Edit(error).into())
    })
}

/// Makes the Group the Vault's Browser group, where captured logins are saved.
#[tauri::command]
#[specta::specta]
pub async fn set_browser_group(core: Core<'_>, vault: VaultId, group: GroupId) -> ApiResult<()> {
    let (vault, group) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, group.resolve()?)))?;
    core.edit(|workspace| workspace.set_browser_group(vault, group))
}
