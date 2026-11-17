//! Updates (ADR 0014): where updating stands, checking now, installing, and the release's page.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::Core;
use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::updates::{self, UpdateState, Updates};

type UpdatesState<'a> = State<'a, Arc<Updates>>;

#[tauri::command]
#[specta::specta]
pub async fn get_updates(updates: UpdatesState<'_>) -> ApiResult<UpdateState> {
    Ok(updates.state())
}

/// Checks for an update now, as the user asked: whether or not automatic checks are on.
#[tauri::command]
#[specta::specta]
pub async fn check_for_updates_now(
    app: AppHandle,
    updates: UpdatesState<'_>,
) -> ApiResult<UpdateState> {
    updates::check(&app, &updates).await;
    Ok(updates.state())
}

/// Downloads and installs the update found, then restarts keefast. Returns once the download
/// starts; its progress comes as `UpdateState` events.
#[tauri::command]
#[specta::specta]
pub async fn install_update(
    app: AppHandle,
    core: Core<'_>,
    updates: UpdatesState<'_>,
) -> ApiResult<()> {
    updates::install(&app, &core, &updates)
}

/// Opens the update's release page in the browser, or the latest release's.
#[tauri::command]
#[specta::specta]
pub async fn open_release_page(app: AppHandle, updates: UpdatesState<'_>) -> ApiResult<()> {
    app.opener()
        .open_url(updates.release_page(), None::<&str>)
        .map_err(|error| {
            ApiError::new(ErrorKind::Io, "keefast couldn't open the release's page.")
                .with_detail(error.to_string())
        })
}
