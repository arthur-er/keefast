//! Keeping keefast running in the Tray (ADR 0013): what closing the window does, whether the
//! desktop has a Tray to keep it in, and the Tray icon with its menu while Close to tray is on.
//!
//! The Companion extension only gets Entries from a running keefast (ADR 0001), so with Close to
//! tray on, closing the window hides it instead of quitting. Where the desktop has no Tray
//! (GNOME without an AppIndicator extension), closing minimizes instead, so keefast can always be
//! found again.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use keefast_workspace::Workspace;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Window, WindowEvent};

use crate::app::{self, MAIN_WINDOW};
use crate::core::AppCore;

/// The id of keefast's one Tray icon.
const TRAY_ID: &str = "keefast";

const SHOW: &str = "show";
const LOCK_ALL: &str = "lockAll";
const QUIT: &str = "quit";

/// What closing keefast's window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// keefast quits, clearing what it copied.
    Quit,
    /// The window hides; keefast keeps running in the Tray.
    Hide,
    /// The window minimizes: the user asked to keep keefast running, but there is no Tray to
    /// find a hidden window in again.
    Minimize,
}

/// What closing the window does, with Close to tray on or off, and with or without a Tray on
/// the desktop.
pub fn close_action(close_to_tray: bool, tray_available: bool) -> CloseAction {
    match (close_to_tray, tray_available) {
        (false, _) => CloseAction::Quit,
        (true, true) => CloseAction::Hide,
        (true, false) => CloseAction::Minimize,
    }
}

/// Whether the desktop had a Tray when last checked: [`TRAY_UNKNOWN`] until then.
static TRAY_AVAILABLE: AtomicU8 = AtomicU8::new(TRAY_UNKNOWN);
const TRAY_UNKNOWN: u8 = 0;
const TRAY_YES: u8 = 1;
const TRAY_NO: u8 = 2;

/// Whether the desktop has a Tray, as last checked: at startup, or when Close to tray was last
/// turned on. Checks now the first time.
pub fn tray_available() -> bool {
    match TRAY_AVAILABLE.load(Ordering::SeqCst) {
        TRAY_UNKNOWN => check_tray(),
        state => state == TRAY_YES,
    }
}

/// Checks again whether the desktop has a Tray, and remembers the answer.
pub fn check_tray() -> bool {
    let available = platform::has_tray();
    let state = if available { TRAY_YES } else { TRAY_NO };
    TRAY_AVAILABLE.store(state, Ordering::SeqCst);
    available
}

/// Shows the Tray icon while Close to tray is on and the desktop has a Tray, and removes it
/// otherwise. Run at startup and whenever the setting changes: no restart is needed.
pub fn sync(app: &AppHandle, close_to_tray: bool) {
    let wanted = close_to_tray && tray_available();
    let present = app.tray_by_id(TRAY_ID).is_some();
    if wanted && !present {
        if let Err(error) = build(app) {
            eprintln!("keefast: can't show the tray icon: {error}");
        }
    } else if !wanted && present {
        let _ = app.remove_tray_by_id(TRAY_ID);
    }
}

/// The Tray icon, with its menu: Show keefast, Lock all Vaults, and Quit.
fn build(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, SHOW, "Show keefast", true, None::<&str>)?,
            &MenuItem::with_id(app, LOCK_ALL, "Lock all Vaults", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("keefast")
        .menu(&menu)
        // A left click shows the window, where the platform tells keefast of clicks (not on
        // Linux, whose AppIndicator opens the menu on any click).
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                app::show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        SHOW => app::show_main_window(app),
        LOCK_ALL => {
            if let Some(core) = app.try_state::<Arc<AppCore>>() {
                core.lock_with(|workspace, clipboard| workspace.lock_all(clipboard));
                // A Vault with unsaved changes asks whether to save them first: in the window.
                if core.read(Workspace::pending_lock).is_some() {
                    app::show_main_window(app);
                }
            }
        }
        QUIT => app.exit(0),
        _ => {}
    }
}

/// Closing the window: quits, hides it or minimizes it, as [`close_action`] says.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != MAIN_WINDOW {
        return;
    }
    let Some(core) = window.try_state::<Arc<AppCore>>() else {
        return;
    };
    match close_action(core.read(Workspace::close_to_tray), tray_available()) {
        CloseAction::Quit => {}
        CloseAction::Hide => {
            api.prevent_close();
            let _ = window.hide();
        }
        CloseAction::Minimize => {
            api.prevent_close();
            let _ = window.minimize();
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use zbus::blocking::Connection;
    use zbus::blocking::fdo::DBusProxy;
    use zbus::names::BusName;

    /// The desktop's Tray, as KDE, the AppIndicator extensions for GNOME, and most others serve
    /// it.
    const STATUS_NOTIFIER_WATCHER: &str = "org.kde.StatusNotifierWatcher";

    /// Whether a Tray is serving on the session bus, and the library Tauri draws Tray icons with
    /// (libayatana-appindicator, loaded when the first icon is made) is installed: without it,
    /// making the icon would abort keefast.
    pub fn has_tray() -> bool {
        watcher_running() && appindicator_installed()
    }

    fn watcher_running() -> bool {
        let running = || -> zbus::Result<bool> {
            let connection = Connection::session()?;
            let name = BusName::try_from(STATUS_NOTIFIER_WATCHER)?;
            Ok(DBusProxy::new(&connection)?.name_has_owner(name)?)
        };
        running().unwrap_or(false)
    }

    fn appindicator_installed() -> bool {
        ["libayatana-appindicator3.so.1", "libappindicator3.so.1"]
            .iter()
            // SAFETY: the library is the one Tauri loads for the Tray icon anyway; loading it
            // runs nothing but its initialisers.
            .any(|name| unsafe { libloading::Library::new(name) }.is_ok())
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    /// Windows always has a notification area.
    pub fn has_tray() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_quits_unless_close_to_tray_is_on() {
        assert_eq!(close_action(false, true), CloseAction::Quit);
        assert_eq!(close_action(false, false), CloseAction::Quit);
    }

    #[test]
    fn closing_to_the_tray_hides_the_window() {
        assert_eq!(close_action(true, true), CloseAction::Hide);
    }

    #[test]
    fn without_a_tray_closing_minimizes_instead() {
        assert_eq!(close_action(true, false), CloseAction::Minimize);
    }
}
