//! The commands the web UI invokes, by domain. Each is typed for the UI by tauri-specta; every
//! one returns a `Result` so the UI handles failures the same way everywhere.
//!
//! Commands are `async` so Tauri runs them off the main thread; the slow ones (key derivation,
//! file dialogs, Dropbox) run on blocking threads, never holding the Workspace while they wait.

pub mod attachments;
pub mod browser;
pub mod clipboard;
pub mod dropbox;
pub mod entries;
pub mod generator;
pub mod groups;
pub mod history;
pub mod icons;
pub mod plugins;
pub mod settings;
pub mod totp;
pub mod vault_settings;
pub mod vaults;
pub mod workspace;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder, FilePath};

use crate::core::AppCore;
use crate::error::{ApiError, ApiResult, ErrorKind};

/// The app's state, as commands get it.
pub type Core<'a> = tauri::State<'a, Arc<AppCore>>;

/// Runs blocking work on a thread made for it.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> ApiResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| {
            ApiError::new(ErrorKind::Unavailable, "keefast could not do that.")
                .with_detail(error.to_string())
        })
}

/// A file dialog over keefast's window.
fn file_dialog<R: Runtime>(app: &AppHandle<R>, title: &'static str) -> FileDialogBuilder<R> {
    let dialog = app.dialog().file().set_title(title);
    match app.get_webview_window(crate::app::MAIN_WINDOW) {
        Some(window) => dialog.set_parent(&window),
        None => dialog,
    }
}

/// A file dialog's answer as a path on disk, if the user picked one.
fn local_path(picked: Option<FilePath>) -> ApiResult<Option<PathBuf>> {
    match picked {
        None => Ok(None),
        Some(path) => path.into_path().map(Some).map_err(|_| ApiError::picker()),
    }
}

/// Asks the user for one existing file.
pub async fn pick_file(app: &AppHandle, title: &'static str) -> ApiResult<Option<PathBuf>> {
    let app = app.clone();
    let picked = blocking(move || file_dialog(&app, title).blocking_pick_file()).await?;
    local_path(picked)
}

/// Asks the user for any number of existing files.
pub async fn pick_files(app: &AppHandle, title: &'static str) -> ApiResult<Vec<PathBuf>> {
    let app = app.clone();
    let picked = blocking(move || file_dialog(&app, title).blocking_pick_files()).await?;
    picked
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.into_path().map_err(|_| ApiError::picker()))
        .collect()
}

/// Asks the user where to save a new file, starting in `directory` with `name` filled in.
pub async fn pick_new_file(
    app: &AppHandle,
    title: &'static str,
    directory: Option<PathBuf>,
    name: &str,
) -> ApiResult<Option<PathBuf>> {
    let app = app.clone();
    let name = name.to_owned();
    let picked = blocking(move || {
        let mut dialog = file_dialog(&app, title).set_file_name(name);
        if let Some(directory) = directory {
            dialog = dialog.set_directory(directory);
        }
        dialog.blocking_save_file()
    })
    .await?;
    local_path(picked)
}

/// The user's home directory, where file dialogs start when nothing better is known.
pub fn home() -> Option<PathBuf> {
    dirs::home_dir()
}
