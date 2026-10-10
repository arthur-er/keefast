//! An Entry's earlier versions, and making one current again.

use keefast_core::Entry;
use secrecy::ExposeSecret;
use serde::Serialize;
use specta::Type;

use super::Core;
use super::entries::{CustomFieldView, custom_field_views, find};
use crate::dto::{Color, EntryId, VaultId, seconds, title_of};
use crate::error::{ApiError, ApiResult};

/// Whether a version has TOTP set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TotpState {
    None,
    Set,
    /// Set up, but its settings can't be read.
    Unreadable,
}

/// A version of an Entry, without its password and protected custom fields.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryVersion {
    /// Its index in the Entry's history, oldest first: what `restore_entry_version` takes.
    pub index: u32,
    /// When it was saved, in seconds since 1970, UTC.
    pub modified: Option<f64>,
    pub title: String,
    pub username: String,
    pub url: String,
    pub notes: String,
    pub has_password: bool,
    /// Whether its password is the Entry's current one: tells a version that changed the
    /// password without showing either.
    pub password_is_current: bool,
    pub custom_fields: Vec<CustomFieldView>,
    pub tags: Vec<String>,
    pub color: Option<Color>,
    pub totp: TotpState,
    pub attachments: Vec<String>,
}

fn version(index: usize, version: &Entry, current: &Entry) -> EntryVersion {
    EntryVersion {
        index: index as u32,
        modified: version.last_modified().map(seconds),
        title: title_of(version),
        username: version.username().to_owned(),
        url: version.url().to_owned(),
        notes: version.notes().to_owned(),
        has_password: !version.password().expose_secret().is_empty(),
        password_is_current: version.password().expose_secret()
            == current.password().expose_secret(),
        custom_fields: custom_field_views(version),
        tags: version.tags().to_vec(),
        color: version.color().map(Into::into),
        totp: match version.totp() {
            None => TotpState::None,
            Some(Ok(_)) => TotpState::Set,
            Some(Err(_)) => TotpState::Unreadable,
        },
        attachments: version
            .attachments()
            .iter()
            .map(|attachment| attachment.name.clone())
            .collect(),
    }
}

/// The Entry's earlier versions, oldest first. Each edit keeps the version it replaces here.
#[tauri::command]
#[specta::specta]
pub async fn get_entry_history(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
) -> ApiResult<Vec<EntryVersion>> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        Ok(found
            .history()
            .iter()
            .enumerate()
            .map(|(index, old)| version(index, old, found))
            .collect())
    })
}

/// The password of a version of the Entry, for the user to see.
#[tauri::command]
#[specta::specta]
pub async fn reveal_version_password(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    version: u32,
) -> ApiResult<String> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        let old = found
            .history()
            .get(version as usize)
            .ok_or_else(ApiError::not_found)?;
        Ok(old.password().expose_secret().to_owned())
    })
}

/// A custom field's value in a version of the Entry, for the user to see.
#[tauri::command]
#[specta::specta]
pub async fn reveal_version_custom_field(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    version: u32,
    index: u32,
) -> ApiResult<String> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        found
            .history()
            .get(version as usize)
            .and_then(|old| old.custom_fields().get(index as usize))
            .map(|field| field.value.expose_secret().to_owned())
            .ok_or_else(ApiError::not_found)
    })
}

/// Makes an earlier version of the Entry current again. The version it replaces goes to the
/// history.
#[tauri::command]
#[specta::specta]
pub async fn restore_entry_version(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    version: u32,
) -> ApiResult<()> {
    let (vault, entry) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, entry.resolve()?)))?;
    core.edit(|workspace| workspace.restore_entry_version(vault, entry, version as usize))
}
