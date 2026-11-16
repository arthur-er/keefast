//! keefast's desktop app: a Tauri webview that renders Workspace state and sends it commands.
// Windows opens a console window for a console program, so release builds are GUI programs there.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod args;
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
    if args.unregister_browser_hosts {
        unregister_browser_hosts();
    }
    app::run(args);
}

/// Removes keefast's registrations of its native host and exits, without opening a window.
/// Browser integration stays on in the app config, so reinstalling keefast registers it again.
fn unregister_browser_hosts() -> ! {
    let result = NativeMessagingHosts::for_current_user()
        .ok_or_else(|| std::io::Error::other("no data directory for the current user"))
        .and_then(|hosts| hosts.uninstall());
    match result {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("keefast: couldn't unregister the native host: {error}");
            std::process::exit(1)
        }
    }
}
