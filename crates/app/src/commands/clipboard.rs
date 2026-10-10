//! Copying Entry fields, on this side of the IPC boundary: the secret goes from the Workspace to
//! the clipboard without passing through the web UI, and is cleared after the configured wait.

use keefast_workspace::CopyField;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Core;
use crate::dto::{EntryId, VaultId};
use crate::error::ApiResult;

/// The field of an Entry to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CopyTarget {
    Username,
    Password,
    /// The Entry's TOTP code now.
    Totp,
    /// A custom field, by its index in the Entry's custom fields.
    CustomField {
        index: u32,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    /// Whole seconds until the clipboard is cleared, if it will be. `ClipboardCleared` says when
    /// it was.
    pub clears_in_seconds: Option<u32>,
}

/// Copies a field of an Entry to the clipboard, to be cleared after the configured wait. An empty
/// field isn't copied (the error's kind is `empty`), so the clipboard keeps what it holds.
#[tauri::command]
#[specta::specta]
pub async fn copy_field(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    field: CopyTarget,
) -> ApiResult<Copied> {
    let field = match field {
        CopyTarget::Username => CopyField::Username,
        CopyTarget::Password => CopyField::Password,
        CopyTarget::Totp => CopyField::TotpCode,
        CopyTarget::CustomField { index } => CopyField::CustomField(index as usize),
    };
    let clears_in = core.update_with_clipboard(|workspace, clipboard| {
        let vault = vault.resolve_open(workspace)?;
        let entry = entry.resolve()?;
        workspace.copy_field(clipboard, vault, entry, field)?;
        ApiResult::Ok(workspace.clipboard_clears_in())
    })?;
    if clears_in.is_some() {
        core.clear_clipboard_when_due();
    }
    Ok(Copied {
        // Rounded up: a moment has passed since copying.
        clears_in_seconds: clears_in.map(|wait| wait.as_secs_f64().ceil() as u32),
    })
}
