//! The app settings: machine-local preferences kept in the app config (ADR 0005).

use std::collections::BTreeMap;
use std::io;
use std::sync::PoisonError;
use std::time::Duration;

use keefast_workspace::{PaneWidths, ThemeSetting, Workspace};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

use super::Core;
use super::generator::GeneratorOptions;
use crate::core::AppCore;
use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::{autostart, browser, shortcuts, tray};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow the OS light/dark setting.
    System,
    Dark,
    Light,
}

impl From<ThemeSetting> for Theme {
    fn from(theme: ThemeSetting) -> Self {
        match theme {
            ThemeSetting::System => Self::System,
            ThemeSetting::Dark => Self::Dark,
            ThemeSetting::Light => Self::Light,
        }
    }
}

impl From<Theme> for ThemeSetting {
    fn from(theme: Theme) -> Self {
        match theme {
            Theme::System => Self::System,
            Theme::Dark => Self::Dark,
            Theme::Light => Self::Light,
        }
    }
}

/// How wide the Vault browser's sidebar and Entry list are, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PaneWidthsView {
    pub sidebar: u32,
    pub entry_list: u32,
}

/// Where browser integration stands.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub enabled: bool,
    /// Which browsers the native host is registered with, or what failed; `None` while off.
    pub message: Option<String>,
    /// Whether something failed.
    pub error: bool,
    pub browsers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: Theme,
    /// Seconds without the user's input before every Vault locks; `None` never.
    pub idle_lock_seconds: Option<u32>,
    pub lock_on_os_lock: bool,
    /// Seconds copied secrets stay on the clipboard; `None` keeps them.
    pub clipboard_clear_seconds: Option<u32>,
    pub autosave: bool,
    pub browser: BrowserStatus,
    /// Whether the favicons of Entries' sites are downloaded. Off unless the user turns it on.
    pub download_favicons: bool,
    pub generator: GeneratorOptions,
    pub pane_widths: PaneWidthsView,
    /// The keys the user gave keefast's shortcuts, by name (`shortcuts.json`'s), in place of
    /// their defaults. Only those the user changed.
    pub shortcuts: BTreeMap<String, Vec<String>>,
    /// Whether closing the window keeps keefast running in the Tray (ADR 0013).
    pub close_to_tray: bool,
    /// Whether the desktop has a Tray, as last checked: without one, closing minimizes instead.
    pub tray_available: bool,
    /// Why the app config couldn't be read or saved, if it couldn't.
    pub config_error: Option<String>,
}

fn seconds(wait: Option<Duration>) -> Option<u32> {
    wait.map(|wait| u32::try_from(wait.as_secs()).unwrap_or(u32::MAX))
}

fn wait(seconds: Option<u32>) -> Option<Duration> {
    seconds
        .filter(|&seconds| seconds > 0)
        .map(|seconds| Duration::from_secs(u64::from(seconds)))
}

/// What to tell the user about browser integration, if it is on: which browsers the native host
/// is registered with, or what failed.
fn browser_status(core: &AppCore, workspace: &Workspace) -> BrowserStatus {
    let enabled = workspace.browser_integration();
    let off = BrowserStatus {
        enabled,
        message: None,
        error: false,
        browsers: Vec::new(),
    };
    if !enabled {
        return off;
    }
    let serve_failed = core
        .browser
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .serve_error()
        .map(|error| match error.kind() {
            io::ErrorKind::AddrInUse => {
                "Another keefast is already serving the browser.".to_owned()
            }
            _ => format!("keefast can't serve the browser: {error}"),
        });
    if let Some(message) = serve_failed {
        return BrowserStatus {
            message: Some(message),
            error: true,
            ..off
        };
    }
    match workspace.browser_hosts_status() {
        None => off,
        Some(Err(error)) => BrowserStatus {
            message: Some(format!("Browser integration could not be set up: {error}")),
            error: true,
            ..off
        },
        Some(Ok(browsers)) if browsers.is_empty() => BrowserStatus {
            message: Some(
                "No supported browser found (Chrome, Chromium, Brave, Edge, Vivaldi).".to_owned(),
            ),
            ..off
        },
        Some(Ok(browsers)) => {
            let names: Vec<String> = browsers.iter().map(|b| b.name().to_owned()).collect();
            BrowserStatus {
                message: Some(format!("Set up for {}", names.join(", "))),
                browsers: names,
                ..off
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(core: Core<'_>) -> ApiResult<AppSettings> {
    Ok(core.read(|workspace| {
        let widths = workspace.pane_widths();
        AppSettings {
            theme: workspace.theme().into(),
            idle_lock_seconds: seconds(workspace.idle_lock_after()),
            lock_on_os_lock: workspace.lock_on_os_lock(),
            clipboard_clear_seconds: seconds(workspace.clipboard_clear_after()),
            autosave: workspace.autosave(),
            browser: browser_status(&core, workspace),
            download_favicons: workspace.downloads_favicons(),
            generator: workspace.generator_settings().into(),
            pane_widths: PaneWidthsView {
                sidebar: widths.sidebar,
                entry_list: widths.entry_list,
            },
            shortcuts: workspace.shortcuts().clone(),
            close_to_tray: workspace.close_to_tray(),
            tray_available: tray::tray_available(),
            config_error: workspace.config_error().map(ToString::to_string),
        }
    }))
}

/// Gives the window's own frame the theme, where the OS draws it.
pub fn apply_window_theme(app: &AppHandle, theme: ThemeSetting) {
    if let Some(window) = app.get_webview_window(crate::app::MAIN_WINDOW) {
        let theme = match theme {
            ThemeSetting::System => None,
            ThemeSetting::Dark => Some(tauri::Theme::Dark),
            ThemeSetting::Light => Some(tauri::Theme::Light),
        };
        let _ = window.set_theme(theme);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn set_theme(app: AppHandle, core: Core<'_>, theme: Theme) -> ApiResult<()> {
    core.update(|workspace| workspace.set_theme(theme.into()));
    apply_window_theme(&app, theme.into());
    Ok(())
}

/// Changes how long the Vaults wait for the user's input before they lock; `None` (or 0) never.
#[tauri::command]
#[specta::specta]
pub async fn set_idle_lock_after(core: Core<'_>, seconds: Option<u32>) -> ApiResult<()> {
    core.update(|workspace| workspace.set_idle_lock_after(wait(seconds)));
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_lock_on_os_lock(core: Core<'_>, enabled: bool) -> ApiResult<()> {
    core.update(|workspace| workspace.set_lock_on_os_lock(enabled));
    Ok(())
}

/// Changes how long secrets copied from now on stay on the clipboard; `None` (or 0) keeps them.
#[tauri::command]
#[specta::specta]
pub async fn set_clipboard_clear_after(core: Core<'_>, seconds: Option<u32>) -> ApiResult<()> {
    core.update(|workspace| workspace.set_clipboard_clear_after(wait(seconds)));
    Ok(())
}

/// Turns saving after every change on or off. Turned on, Vaults with unsaved changes are saved.
#[tauri::command]
#[specta::specta]
pub async fn set_autosave(core: Core<'_>, enabled: bool) -> ApiResult<()> {
    core.update(|workspace| workspace.set_autosave(enabled));
    core.autosave();
    Ok(())
}

/// Turns browser integration on or off: registers or unregisters the native host with the
/// browsers, and serves them.
#[tauri::command]
#[specta::specta]
pub async fn set_browser_integration(core: Core<'_>, enabled: bool) -> ApiResult<()> {
    browser::set_enabled(core.inner(), enabled);
    Ok(())
}

/// Turns Close to tray on or off, adding or removing the Tray icon at once. Turned on, keefast
/// checks again whether the desktop has a Tray, which it may have gained since keefast started.
#[tauri::command]
#[specta::specta]
pub async fn set_close_to_tray(app: AppHandle, core: Core<'_>, enabled: bool) -> ApiResult<()> {
    if enabled {
        tray::check_tray();
    }
    core.update(|workspace| workspace.set_close_to_tray(enabled));
    tray::sync(&app, enabled);
    Ok(())
}

/// Turns favicon download on or off. Turned off, the favicons downloaded are forgotten.
#[tauri::command]
#[specta::specta]
pub async fn set_download_favicons(core: Core<'_>, enabled: bool) -> ApiResult<()> {
    core.update(|workspace| workspace.set_download_favicons(enabled));
    Ok(())
}

/// Remembers how wide the panes are. Changes nothing else, so the UI isn't told.
#[tauri::command]
#[specta::specta]
pub async fn set_pane_widths(core: Core<'_>, widths: PaneWidthsView) -> ApiResult<()> {
    core.update_quietly(|workspace| {
        workspace.set_pane_widths(PaneWidths {
            sidebar: widths.sidebar,
            entry_list: widths.entry_list,
        })
    });
    Ok(())
}

/// Gives keefast's shortcut `name` these keys ("Ctrl J"), or its default keys back with `None`.
/// Keys another of keefast's shortcuts, a running Plugin's command or text fields have are
/// refused. Plugins loaded from now on can't have them either.
#[tauri::command]
#[specta::specta]
pub async fn set_shortcut(core: Core<'_>, name: String, keys: Option<String>) -> ApiResult<()> {
    let Some(keys) = keys else {
        core.update(|workspace| workspace.set_shortcut(&name, None));
        return Ok(());
    };
    let changed = core.read(|workspace| workspace.shortcuts().clone());
    let keys = {
        let plugins = core.plugins();
        shortcuts::check(&name, &keys, &changed, |combo| {
            plugins.shortcut_holder(combo).map(str::to_owned)
        })?
    };
    core.change_plugins(|_, host| host.reserve_shortcuts(&[&keys]));
    core.update(|workspace| workspace.set_shortcut(&name, Some(vec![keys])));
    Ok(())
}

/// Whether keefast starts when the user logs in, as the OS has it registered now: there is no
/// setting for it, so an entry removed by hand reads as off.
#[tauri::command]
#[specta::specta]
pub async fn get_start_on_boot(app: AppHandle) -> ApiResult<bool> {
    autostart::is_enabled(&app).map_err(|error| {
        ApiError::new(
            ErrorKind::Io,
            "keefast couldn't tell whether it starts on boot.",
        )
        .with_detail(error.to_string())
    })
}

/// Registers keefast to start, hidden, when the user logs in, or removes its registration.
#[tauri::command]
#[specta::specta]
pub async fn set_start_on_boot(app: AppHandle, enabled: bool) -> ApiResult<()> {
    autostart::set_enabled(&app, enabled).map_err(|error| {
        let message = if enabled {
            "keefast couldn't set itself to start on boot."
        } else {
            "keefast couldn't stop starting on boot."
        };
        ApiError::new(ErrorKind::Io, message).with_detail(error.to_string())
    })
}
