//! Updates (ADR 0014): keefast asks GitHub whether there is a newer release, once a day while
//! the user has agreed to it or when they ask, and installs it when they say so.
//!
//! The updater installs nothing whose signature doesn't verify with the public key in
//! `tauri.conf.json`; releases are signed with keefast's update key in CI. Only the AppImage and
//! the Windows installer's install update themselves. Other installs (the tarball, Nix, a build
//! from source) hear of a new release and are pointed at its page.

use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use keefast_workspace::Workspace;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use tauri_specta::Event;

use crate::core::AppCore;
use crate::error::{ApiError, ApiResult, ErrorKind};

/// How often keefast checks while the user has agreed to it.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// How long after starting keefast first checks, so checking doesn't slow starting.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(10);

/// Where keefast's releases are; a release's page is under `tag/v<version>`.
const RELEASES: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases");

/// A newer release than the one running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub version: String,
    /// The release's page, with what changed and its packages.
    pub release_page: String,
}

impl AvailableUpdate {
    fn of(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            release_page: format!("{RELEASES}/tag/v{}", update.version),
        }
    }
}

/// Where checking for and installing an update stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateStatus {
    /// Not checked since keefast started.
    Unchecked,
    Checking,
    #[serde(rename_all = "camelCase")]
    UpToDate {
        /// When, in seconds since 1970.
        checked_at: f64,
    },
    Available {
        update: AvailableUpdate,
    },
    /// Downloading `update`; once downloaded, it is installed and keefast restarts.
    Downloading {
        update: AvailableUpdate,
        /// How much is downloaded, from 0 to 1, when the size is known.
        progress: Option<f64>,
    },
    /// Checking failed, or installing `update` did.
    Failed {
        message: String,
        update: Option<AvailableUpdate>,
    },
}

/// keefast's version, whether this install updates itself, and where updating stands. Sent to
/// the UI whenever it changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub current_version: String,
    /// Whether keefast can install an update itself: the AppImage and the Windows installer's
    /// install can. Others are pointed at the release's page.
    pub installs_updates: bool,
    pub status: UpdateStatus,
}

/// Updating, managed as Tauri state.
pub struct Updates {
    inner: Mutex<Inner>,
    /// Wakes the checking thread, when the user turns checking on.
    wake: Mutex<mpsc::Sender<()>>,
    installs_updates: bool,
}

struct Inner {
    status: UpdateStatus,
    /// The update found by the last check, to install.
    pending: Option<Update>,
    last_checked: Option<Instant>,
}

impl Updates {
    pub fn state(&self) -> UpdateState {
        UpdateState {
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            installs_updates: self.installs_updates,
            status: self.lock().status.clone(),
        }
    }

    /// Has the checking thread see whether a check is due, as when checking was turned on.
    pub fn wake(&self) {
        let _ = self
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(());
    }

    /// The release page to show: the update's, else the latest release's.
    pub fn release_page(&self) -> String {
        match &self.lock().status {
            UpdateStatus::Available { update }
            | UpdateStatus::Downloading { update, .. }
            | UpdateStatus::Failed {
                update: Some(update),
                ..
            } => update.release_page.clone(),
            _ => format!("{RELEASES}/latest"),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the status, and tells the UI.
    fn set_status(&self, app: &AppHandle, status: UpdateStatus) {
        self.lock().status = status;
        if let Err(error) = self.state().emit(app) {
            eprintln!("keefast: can't tell the window about the update: {error}");
        }
    }
}

/// Starts the thread that checks once a day while the user has agreed to it, and returns the
/// updating state for the commands.
pub fn start(app: &AppHandle, core: &Arc<AppCore>) -> Arc<Updates> {
    let (wake, woken) = mpsc::channel();
    let updates = Arc::new(Updates {
        inner: Mutex::new(Inner {
            status: UpdateStatus::Unchecked,
            pending: None,
            last_checked: None,
        }),
        wake: Mutex::new(wake),
        installs_updates: installs_updates(app),
    });
    let started = Instant::now();
    let due = {
        let core = Arc::clone(core);
        let updates = Arc::clone(&updates);
        move || {
            let automatic = core.read(Workspace::check_for_updates) == Some(true);
            next_check(
                automatic,
                updates.lock().last_checked,
                started,
                Instant::now(),
            )
        }
    };
    let app = app.clone();
    let checking = Arc::clone(&updates);
    thread::spawn(move || {
        loop {
            let woken = match due() {
                Some(wait) => woken.recv_timeout(wait),
                None => woken.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            if woken == Err(RecvTimeoutError::Disconnected) {
                return;
            }
            if due() == Some(Duration::ZERO) {
                tauri::async_runtime::block_on(check(&app, &checking));
            }
        }
    });
    updates
}

/// How long until the next automatic check: `None` while the user hasn't agreed to them.
/// The first is shortly after starting; then one a day after the last check, the user's own
/// included.
fn next_check(
    automatic: bool,
    last_checked: Option<Instant>,
    started: Instant,
    now: Instant,
) -> Option<Duration> {
    if !automatic {
        return None;
    }
    let due = match last_checked {
        Some(checked) => checked + CHECK_EVERY,
        None => started + FIRST_CHECK_AFTER,
    };
    Some(due.saturating_duration_since(now))
}

/// Asks the update endpoint whether there's a newer release. Does nothing while one downloads.
pub async fn check(app: &AppHandle, updates: &Updates) {
    if matches!(updates.lock().status, UpdateStatus::Downloading { .. }) {
        return;
    }
    updates.set_status(app, UpdateStatus::Checking);
    let checked = match updater(app) {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    let status = {
        let mut inner = updates.lock();
        inner.last_checked = Some(Instant::now());
        match checked {
            Ok(Some(update)) => {
                let status = UpdateStatus::Available {
                    update: AvailableUpdate::of(&update),
                };
                inner.pending = Some(update);
                status
            }
            Ok(None) => {
                inner.pending = None;
                UpdateStatus::UpToDate {
                    checked_at: now_seconds(),
                }
            }
            // The update found before is still there to install.
            Err(_) if inner.pending.is_some() => UpdateStatus::Available {
                update: inner.pending.as_ref().map(AvailableUpdate::of).unwrap(),
            },
            Err(error) => UpdateStatus::Failed {
                message: format!("keefast couldn't check for updates: {error}"),
                update: None,
            },
        }
    };
    updates.set_status(app, status);
}

/// Downloads the update found, then installs it and restarts keefast; the UI hears of the
/// download's progress. Refused while a Vault has unsaved changes, which restarting would lose.
pub fn install(app: &AppHandle, core: &Arc<AppCore>, updates: &Arc<Updates>) -> ApiResult<()> {
    if !updates.installs_updates {
        return Err(ApiError::new(
            ErrorKind::Unavailable,
            "This copy of keefast doesn't update itself: download the new version from its release page.",
        ));
    }
    unsaved_changes(core)?;
    let update = {
        let inner = updates.lock();
        if matches!(inner.status, UpdateStatus::Downloading { .. }) {
            return Ok(());
        }
        inner
            .pending
            .clone()
            .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "There's no update to install."))?
    };
    let available = AvailableUpdate::of(&update);
    updates.set_status(
        app,
        UpdateStatus::Downloading {
            update: available.clone(),
            progress: None,
        },
    );
    let (app, core, updates) = (app.clone(), Arc::clone(core), Arc::clone(updates));
    tauri::async_runtime::spawn(async move {
        let failed = |message: String| UpdateStatus::Failed {
            message,
            update: Some(available.clone()),
        };
        let mut downloaded = 0u64;
        let mut shown = None;
        let progress = |chunk: usize, total: Option<u64>| {
            downloaded += chunk as u64;
            let progress = total
                .filter(|&total| total > 0)
                .map(|total| (downloaded as f64 / total as f64).min(1.0));
            // Each whole percent, not each chunk.
            let percent = progress.map(|progress| (progress * 100.0) as u32);
            if percent != shown {
                shown = percent;
                updates.set_status(
                    &app,
                    UpdateStatus::Downloading {
                        update: available.clone(),
                        progress,
                    },
                );
            }
        };
        let bytes = match update.download(progress, || {}).await {
            Ok(bytes) => bytes,
            Err(error) => {
                let message = format!("keefast couldn't download the update: {error}");
                updates.set_status(&app, failed(message));
                return;
            }
        };
        // An edit made during the download.
        if let Err(error) = unsaved_changes(&core) {
            updates.set_status(&app, failed(error.message));
            return;
        }
        core.clear_clipboard_now();
        // On Windows, this starts the installer and exits keefast (see `updater`).
        match update.install(bytes) {
            Ok(()) => app.restart(),
            Err(error) => {
                let message = format!("keefast couldn't install the update: {error}");
                updates.set_status(&app, failed(message));
            }
        }
    });
    Ok(())
}

/// The updater, checking the endpoint in `tauri.conf.json`, or `KEEFAST_UPDATE_URL`'s to try
/// an update served elsewhere. Either way, only an update signed with keefast's update key
/// installs.
fn updater(app: &AppHandle) -> tauri_plugin_updater::Result<Updater> {
    let exiting = app.clone();
    let mut builder = app.updater_builder().on_before_exit(move || {
        // The installer exits keefast at once on Windows, skipping its exit handler: what it
        // copied is cleared here, and the next keefast mustn't take it for one still running.
        if let Some(core) = exiting.try_state::<Arc<AppCore>>() {
            core.clear_clipboard_now();
        }
        tauri_plugin_single_instance::destroy(&exiting);
    });
    if let Some(url) = std::env::var("KEEFAST_UPDATE_URL")
        .ok()
        .filter(|url| !url.is_empty())
    {
        let url = url.parse().map_err(|_| {
            tauri_plugin_updater::Error::Io(std::io::Error::other(
                "KEEFAST_UPDATE_URL is not a URL",
            ))
        })?;
        builder = builder.endpoints(vec![url])?;
    }
    builder.build()
}

/// Refuses while a Vault has unsaved changes or is being saved.
fn unsaved_changes(core: &AppCore) -> ApiResult<()> {
    let unsaved = core.read(|workspace| {
        workspace
            .vaults()
            .find(|open| open.has_unsaved_changes() || open.is_saving())
            .map(|open| open.display_name())
    });
    match unsaved {
        None => Ok(()),
        Some(name) => Err(ApiError::new(
            ErrorKind::Conflict,
            format!("Save {name} before updating: keefast restarts to update."),
        )),
    }
}

/// Whether this install updates itself: the AppImage (the updater replaces the file it runs
/// from) and the Windows installer's install (the updater runs the new installer). A build run
/// from elsewhere would be replaced by an install, so it doesn't.
fn installs_updates(app: &AppHandle) -> bool {
    #[cfg(target_os = "linux")]
    {
        app.env().appimage.is_some()
    }
    #[cfg(windows)]
    {
        let _ = app;
        match (std::env::current_exe(), dirs::data_local_dir()) {
            (Ok(exe), Some(local)) => installed_by_the_installer(&exe, &local),
            _ => false,
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = app;
        false
    }
}

/// Whether `exe` is where the Windows installer puts keefast: `%LOCALAPPDATA%\Programs\keefast`
/// (packaging/windows/keefast.wxs).
#[cfg_attr(not(windows), allow(dead_code))]
fn installed_by_the_installer(exe: &Path, local_app_data: &Path) -> bool {
    let folder = local_app_data.join("Programs").join("keefast");
    exe.parent().is_some_and(|parent| {
        parent
            .to_string_lossy()
            .eq_ignore_ascii_case(&folder.to_string_lossy())
    })
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn nothing_is_checked_until_the_user_agrees() {
        let now = Instant::now();
        assert_eq!(next_check(false, None, now, now), None);
        assert_eq!(next_check(false, Some(now), now, now), None);
    }

    #[test]
    fn the_first_check_is_shortly_after_starting() {
        let started = Instant::now();
        assert_eq!(
            next_check(true, None, started, started),
            Some(FIRST_CHECK_AFTER)
        );
        // Agreed to long after starting: due at once.
        let later = started + Duration::from_secs(3600);
        assert_eq!(next_check(true, None, started, later), Some(Duration::ZERO));
    }

    #[test]
    fn then_once_a_day_after_the_last_check() {
        let started = Instant::now();
        let checked = started + Duration::from_secs(60);
        let hour_later = checked + Duration::from_secs(3600);
        assert_eq!(
            next_check(true, Some(checked), started, hour_later),
            Some(CHECK_EVERY - Duration::from_secs(3600))
        );
        let day_later = checked + CHECK_EVERY + Duration::from_secs(1);
        assert_eq!(
            next_check(true, Some(checked), started, day_later),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn only_the_installers_install_updates_itself_on_windows() {
        let local = PathBuf::from("C:/Users/me/AppData/Local");
        assert!(installed_by_the_installer(
            &local.join("Programs").join("keefast").join("keefast.exe"),
            &local
        ));
        assert!(installed_by_the_installer(
            &local.join("programs").join("KeeFast").join("keefast.exe"),
            &local
        ));
        assert!(!installed_by_the_installer(
            Path::new("C:/src/keefast/target/release/keefast.exe"),
            &local
        ));
    }

    #[test]
    fn a_release_page_is_its_tag_on_github() {
        assert_eq!(
            format!("{RELEASES}/tag/v1.2.3"),
            "https://github.com/arthur-er/keefast/releases/tag/v1.2.3"
        );
    }
}
