//! Opening Vault files from Dropbox: connecting the account in the browser, browsing the whole
//! Dropbox, and disconnecting. Every Dropbox call runs off the main thread. Dropbox Vaults' sync
//! status is in the workspace snapshot, and `sync_now` checks them at once.

use std::sync::{Arc, Mutex, PoisonError};

use keefast_workspace::{ConnectCanceller, Dropbox, DropboxAccount, DropboxItem};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::{Core, blocking};
use crate::dto::{Location, Provider};
use crate::error::{ApiError, ApiResult, ErrorKind};

/// The connection the user is giving in their browser, while they are.
#[derive(Default)]
pub struct Connecting(Mutex<Option<(String, ConnectCanceller)>>);

/// The connected Dropbox account, as the user knows it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DropboxAccountView {
    pub name: String,
    pub email: String,
}

impl From<DropboxAccount> for DropboxAccountView {
    fn from(account: DropboxAccount) -> Self {
        Self {
            name: account.name,
            email: account.email,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DropboxStatus {
    /// Whether this build can reach Dropbox at all (it has an app key).
    pub available: bool,
    pub account: Option<DropboxAccountView>,
    /// Whether the user is connecting the account in their browser now.
    pub connecting: bool,
}

/// A file or folder in Dropbox.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DropboxEntry {
    pub name: String,
    /// The full path, as Dropbox displays it: `/Vaults/Personal.kdbx`.
    pub path: String,
    pub is_folder: bool,
    /// Where the file is, to open it with `unlock_vault`.
    pub location: Location,
}

impl From<DropboxItem> for DropboxEntry {
    fn from(item: DropboxItem) -> Self {
        Self {
            location: Location {
                provider: Provider::Dropbox,
                path: item.path.clone(),
            },
            name: item.name,
            path: item.path,
            is_folder: item.is_folder,
        }
    }
}

fn dropbox(core: &Core<'_>) -> ApiResult<Arc<Dropbox>> {
    core.read(|workspace| workspace.dropbox().cloned())
        .ok_or_else(|| {
            ApiError::new(
                ErrorKind::Unavailable,
                "This build of keefast has no Dropbox app key.",
            )
        })
}

/// Whether Dropbox can be reached, and which account is connected. Reads the OS secret store.
#[tauri::command]
#[specta::specta]
pub async fn dropbox_status(
    core: Core<'_>,
    connecting: State<'_, Connecting>,
) -> ApiResult<DropboxStatus> {
    let is_connecting = connecting
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some();
    let Ok(dropbox) = dropbox(&core) else {
        return Ok(DropboxStatus {
            available: false,
            account: None,
            connecting: false,
        });
    };
    let account = blocking(move || dropbox.account()).await??;
    Ok(DropboxStatus {
        available: true,
        account: account.map(Into::into),
        connecting: is_connecting,
    })
}

/// Connects a Dropbox account: opens Dropbox's page in the user's browser, and waits until
/// Dropbox sends the browser back. keefast never sees the Dropbox password.
#[tauri::command]
#[specta::specta]
pub async fn dropbox_connect(
    app: AppHandle,
    core: Core<'_>,
    connecting: State<'_, Connecting>,
) -> ApiResult<DropboxAccountView> {
    let dropbox = dropbox(&core)?;
    let authorization = dropbox.begin_connect()?;
    let url = authorization.url().to_owned();
    *connecting.0.lock().unwrap_or_else(PoisonError::into_inner) =
        Some((url.clone(), authorization.canceller()));
    if let Err(error) = app.opener().open_url(&url, None::<&str>) {
        eprintln!("keefast: can't open Dropbox's page in the browser: {error}");
    }
    let connected = blocking(move || dropbox.finish_connect(authorization)).await?;
    *connecting.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    Ok(connected?.into())
}

/// Opens Dropbox's page again, while connecting.
#[tauri::command]
#[specta::specta]
pub async fn dropbox_reopen_connect_page(
    app: AppHandle,
    connecting: State<'_, Connecting>,
) -> ApiResult<()> {
    let url = connecting
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|(url, _)| url.clone());
    if let Some(url) = url {
        app.opener().open_url(url, None::<&str>).map_err(|error| {
            ApiError::new(ErrorKind::Unavailable, "The browser could not be opened.")
                .with_detail(error.to_string())
        })?;
    }
    Ok(())
}

/// Stops waiting for the browser; `dropbox_connect` then fails as cancelled.
#[tauri::command]
#[specta::specta]
pub async fn dropbox_cancel_connect(connecting: State<'_, Connecting>) -> ApiResult<()> {
    if let Some((_, canceller)) = connecting
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        canceller.cancel();
    }
    Ok(())
}

/// Disconnects the Dropbox account, forgetting its tokens.
#[tauri::command]
#[specta::specta]
pub async fn dropbox_disconnect(core: Core<'_>) -> ApiResult<()> {
    let dropbox = dropbox(&core)?;
    blocking(move || dropbox.disconnect()).await??;
    Ok(())
}

/// The files and folders in a Dropbox folder; `""` is the top of the Dropbox.
#[tauri::command]
#[specta::specta]
pub async fn dropbox_list_folder(core: Core<'_>, path: String) -> ApiResult<Vec<DropboxEntry>> {
    let dropbox = dropbox(&core)?;
    let items = blocking(move || dropbox.list_folder(&path)).await??;
    Ok(items.into_iter().map(Into::into).collect())
}
