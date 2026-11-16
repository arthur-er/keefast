//! keefast's desktop app: a Tauri webview that renders Workspace state and sends it commands.
// Windows opens a console window for a console program, so release builds are GUI programs there.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod args;
mod autostart;
mod background;
mod browser;
mod clipboard;
mod commands;
mod core;
mod dto;
mod error;
mod events;
mod icon_protocol;
mod merge_notice;
mod shortcuts;
mod tray;

use keefast_workspace::{BrowserHosts, NativeMessagingHosts};

use crate::args::Args;

fn main() {
    let args = Args::parse(std::env::args_os().skip(1));
    if args.unregister {
        unregister();
    }
    app::run(args);
}

/// Removes what keefast registered with the OS and exits, without opening a window: its native
/// host's registrations with the browsers, and Start on boot. Browser integration stays on in
/// the app config, so reinstalling keefast registers the native host again.
fn unregister() -> ! {
    let browsers = NativeMessagingHosts::for_current_user()
        .ok_or_else(|| std::io::Error::other("no data directory for the current user"))
        .and_then(|hosts| hosts.uninstall());
    if let Err(error) = &browsers {
        eprintln!("keefast: couldn't unregister the native host: {error}");
    }
    let start_on_boot = autostart::unregister();
    if let Err(error) = &start_on_boot {
        eprintln!("keefast: couldn't remove Start on boot: {error}");
    }
    std::process::exit(if browsers.is_ok() && start_on_boot.is_ok() {
        0
    } else {
        1
    })
}
