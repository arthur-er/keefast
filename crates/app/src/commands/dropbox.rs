//! Opening Vault files from Dropbox: connecting the account in the browser, browsing the whole
//! Dropbox, and disconnecting. Every Dropbox call runs off the main thread. Dropbox Vaults' sync
//! status is in the workspace snapshot, and `sync_now` checks them at once.

use std::sync::{Arc, Mutex, PoisonError};

use keefast_core::Timestamp;
use keefast_workspace::{ConnectCanceller, Dropbox, DropboxAccount, DropboxItem};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::{Core, blocking};
use crate::dto::{Location, Provider, seconds as seconds_since_1970};
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
    /// A file's size in bytes, up to 4 GiB; `None` for a folder.
    pub size: Option<u32>,
    /// When a file last changed in Dropbox, in seconds since 1970 UTC; `None` for a folder.
    pub modified: Option<f64>,
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
            size: item
                .size
                .map(|size| u32::try_from(size).unwrap_or(u32::MAX)),
            modified: item.modified.as_deref().and_then(unix_seconds),
        }
    }
}

/// Seconds since 1970 for a UTC time as Dropbox writes it: `2026-10-07T23:15:22Z`.
fn unix_seconds(text: &str) -> Option<f64> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let day = Timestamp::parse_date(date)?;
    let mut parts = time.splitn(3, ':').map(|part| part.parse::<u8>().ok());
    let (Some(Some(hour)), Some(Some(minute)), Some(Some(second))) =
        (parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let seconds = i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second);
    Some(seconds_since_1970(Timestamp::from_unix_seconds(
        day.unix_seconds() + seconds,
    )))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropbox_times_become_seconds_since_1970() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(unix_seconds("2026-10-07T23:15:22Z"), Some(1_791_414_922.0));
    }

    #[test]
    fn times_dropbox_would_not_write_are_left_out() {
        for text in [
            "",
            "2026-10-07",
            "2026-10-07T23:15:22",
            "2026-02-30T00:00:00Z",
            "2026-10-07T24:00:00Z",
            "2026-10-07T23:15Z",
            "2026-10-07Tab:cd:efZ",
        ] {
            assert_eq!(unix_seconds(text), None, "{text}");
        }
    }

    #[test]
    fn a_file_has_its_size_and_time_and_a_folder_neither() {
        let file = DropboxEntry::from(DropboxItem {
            name: "Work.kdbx".into(),
            path: "/Apps/keefast/Work.kdbx".into(),
            is_folder: false,
            size: Some(217_088),
            modified: Some("1970-01-02T00:00:00Z".into()),
        });
        assert_eq!(file.size, Some(217_088));
        assert_eq!(file.modified, Some(86_400.0));
        assert_eq!(file.location.path, "/Apps/keefast/Work.kdbx");

        let folder = DropboxEntry::from(DropboxItem {
            name: "Apps".into(),
            path: "/Apps".into(),
            is_folder: true,
            size: None,
            modified: None,
        });
        assert_eq!((folder.size, folder.modified), (None, None));
    }
}
