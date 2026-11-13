//! A Vault's custom icons, which its Entries can use.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use super::{Core, pick_file};
use crate::core::AppCore;
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
    core.read(|workspace| vault.resolve_open(workspace))?;
    let Some(path) = pick_file(&app, "Choose an icon image (PNG)").await? else {
        return Ok(None);
    };
    import_icon(&core, vault, &path).map(Some)
}

/// Adds the PNG image at `path` to the Vault's custom icons, then autosaves like any other edit.
fn import_icon(core: &Arc<AppCore>, vault: VaultId, path: &Path) -> ApiResult<IconId> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let icon = core.update(|workspace| workspace.import_custom_icon(vault, path))?;
    core.autosave();
    Ok(icon.into())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use keefast_workspace::{SaveStatus, UnlockSecrets, VaultLocation};

    use super::import_icon;
    use crate::core::tests::core;

    /// A 1x1 opaque red PNG.
    const RED_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    /// The save status once an icon is imported into a copy of the KeePassXC fixture, after any
    /// save that started has finished.
    fn status_after_importing(autosave: bool) -> SaveStatus {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("icons.kdbx");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/keepassxc-kdbx4.kdbx"
            ),
            &path,
        )
        .unwrap();
        let png = dir.path().join("red.png");
        std::fs::write(&png, RED_PNG).unwrap();
        let (core, _) = core();
        let vault = core.update(|workspace| {
            workspace.set_autosave(autosave);
            workspace
                .open_vault(
                    &VaultLocation::Local(path.clone()),
                    &UnlockSecrets::password("fixture-password"),
                )
                .unwrap()
        });

        import_icon(&core, vault.into(), &png).unwrap();

        // Saves run on threads of their own; one can take a while on a slow, busy machine.
        let started = Instant::now();
        loop {
            let status = core.read(|workspace| workspace.save_status());
            if status != SaveStatus::Saving || started.elapsed() > Duration::from_secs(60) {
                return status;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn importing_an_icon_autosaves_like_any_other_edit() {
        assert_eq!(status_after_importing(true), SaveStatus::Saved);
        assert_eq!(status_after_importing(false), SaveStatus::Unsaved);
    }
}
