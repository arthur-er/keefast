//! A Vault's custom icons, which its Entries can use.

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use super::{Core, pick_file};
use crate::dto::{IconId, VaultId};
use crate::error::{ApiError, ApiResult};
use crate::icon_protocol;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomIconView {
    pub id: IconId,
    /// Where to draw it from.
    pub url: String,
}

/// The Vault's custom icons.
#[tauri::command]
#[specta::specta]
pub async fn list_custom_icons(core: Core<'_>, vault: VaultId) -> ApiResult<Vec<CustomIconView>> {
    core.read(|workspace| {
        let id = vault.resolve_open(workspace)?;
        let open = workspace
            .readable_vault(id)
            .ok_or_else(ApiError::vault_not_found)?;
        Ok(open
            .vault()
            .custom_icons()
            .iter()
            .map(|icon| CustomIconView {
                id: icon.id().into(),
                url: icon_protocol::custom_icon_url(id, icon.id()),
            })
            .collect())
    })
}

/// Asks the user for a PNG image and adds it to the Vault's custom icons, returning the icon to
/// give an Entry (with `update_entry`). An image the Vault has already isn't added again.
#[tauri::command]
#[specta::specta]
pub async fn import_custom_icon(
    app: AppHandle,
    core: Core<'_>,
    vault: VaultId,
) -> ApiResult<Option<IconId>> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let Some(path) = pick_file(&app, "Choose an icon image (PNG)").await? else {
        return Ok(None);
    };
    let icon = core.update(|workspace| workspace.import_custom_icon(vault, &path))?;
    core.autosave();
    Ok(Some(icon.into()))
}
