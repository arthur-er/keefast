//! Quitting keefast, which closing its window doesn't do with Close to tray on (ADR 0013).

use tauri::AppHandle;

use crate::error::ApiResult;

/// Quits keefast, as the Tray's Quit does: what it copied is cleared on the way out.
#[tauri::command]
#[specta::specta]
pub async fn quit(app: AppHandle) -> ApiResult<()> {
    app.exit(0);
    Ok(())
}
