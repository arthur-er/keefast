//! The Tauri app: the Workspace constructed as the GPUI app did, the command and event API typed
//! for the web UI, the background work, and the window.

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use keefast_agent::installed::AgentCommand;
use keefast_plugins::PluginStore;
use keefast_workspace::{
    Dropbox, DropboxConfig, FileConfigStore, FileVaultCache, HttpFavicons, LocalStorage,
    NativeMessagingHosts, OsSecretStore, Workspace,
};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_specta::{Builder, Event, collect_commands, collect_events};

use crate::agent_hosts::{self, AgentHosts};
use crate::args::Args;
use crate::autostart::{self, HiddenLaunch};
use crate::commands::{self, dropbox::Connecting, settings::apply_window_theme};
use crate::core::{AppCore, Launch};
use crate::events::{
    AppEvent, BrowserRequest, ClipboardCleared, Launched, MergeNotice, Notifier, SaveFailed,
    WorkspaceChanged,
};
use crate::{background, browser, icon_protocol, tray};

/// The label of keefast's one window, as `tauri.conf.json` names it.
pub const MAIN_WINDOW: &str = "main";

/// Every command and event the web UI can use. `ui/src/bindings.ts` is generated from this.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::workspace::get_workspace,
            commands::workspace::get_launch,
            commands::workspace::set_view,
            commands::workspace::note_activity,
            commands::workspace::group_for_new_items,
            commands::vaults::default_encryption,
            commands::vaults::estimate_unlock_time,
            commands::vaults::choose_vault_file,
            commands::vaults::choose_key_file,
            commands::vaults::choose_new_vault_file,
            commands::vaults::create_key_file,
            commands::vaults::unlock_vault,
            commands::vaults::create_vault,
            commands::vaults::forget_recent,
            commands::vaults::lock_all,
            commands::vaults::lock_vault,
            commands::vaults::answer_lock,
            commands::vaults::save_all,
            commands::vaults::save_vault,
            commands::vaults::sync_now,
            commands::groups::list_groups,
            commands::groups::create_group,
            commands::groups::rename_group,
            commands::groups::move_group,
            commands::groups::delete_group,
            commands::groups::deletes_group_for_good,
            commands::groups::set_browser_group,
            commands::entries::list_entries,
            commands::entries::search_palette,
            commands::entries::get_entry,
            commands::entries::reveal_password,
            commands::entries::reveal_custom_field,
            commands::entries::create_entry,
            commands::entries::update_entry,
            commands::entries::add_tags,
            commands::entries::move_entry,
            commands::entries::delete_entry,
            commands::entries::forget_access,
            commands::entries::open_entry_url,
            commands::entries::set_tag_color,
            commands::facets::list_facets,
            commands::facets::format_owned_field,
            commands::facets::validate_owned_field,
            commands::history::get_entry_history,
            commands::history::reveal_version_password,
            commands::history::reveal_version_custom_field,
            commands::history::restore_entry_version,
            commands::totp::get_totp_code,
            commands::totp::reveal_totp_setup,
            commands::totp::preview_totp,
            commands::totp::set_totp,
            commands::totp::remove_totp,
            commands::attachments::add_attachments,
            commands::attachments::remove_attachment,
            commands::attachments::preview_attachment,
            commands::attachments::save_attachment,
            commands::icons::list_custom_icons,
            commands::icons::import_custom_icon,
            commands::clipboard::copy_field,
            commands::generator::generator_limits,
            commands::generator::set_generator_settings,
            commands::generator::generate_password,
            commands::generator::estimate_strength,
            commands::settings::get_settings,
            commands::settings::set_theme,
            commands::settings::set_idle_lock_after,
            commands::settings::set_lock_on_os_lock,
            commands::settings::set_clipboard_clear_after,
            commands::settings::set_autosave,
            commands::settings::set_browser_integration,
            commands::settings::set_download_favicons,
            commands::settings::set_close_to_tray,
            commands::settings::set_pane_widths,
            commands::settings::set_shortcut,
            commands::settings::get_start_on_boot,
            commands::settings::set_start_on_boot,
            commands::plugins::list_plugins,
            commands::plugins::choose_plugin_package,
            commands::plugins::install_plugin,
            commands::plugins::set_plugin_enabled,
            commands::plugins::remove_plugin,
            commands::vault_settings::get_vault_settings,
            commands::vault_settings::change_vault_key,
            commands::vault_settings::rename_vault,
            commands::vault_settings::set_recycle_bin_enabled,
            commands::vault_settings::show_vault_file_in_folder,
            commands::vault_settings::set_default_access_policy,
            commands::vault_settings::revoke_pairing,
            commands::agents::get_agent_settings,
            commands::agents::set_agent_group,
            commands::agents::create_agent_pairing,
            commands::agents::set_agent_pairing_reads,
            commands::agents::revoke_agent_pairing,
            commands::agents::get_entry_agent_access,
            commands::agents::remember_agent_decision,
            commands::agents::forget_agent_decision,
            commands::agent_hosts::get_agent_hosts,
            commands::agent_hosts::add_agent_host,
            commands::agent_hosts::remove_agent_host,
            commands::agent_hosts::answer_agent_prompt,
            commands::agent_hosts::answer_agent_consent,
            commands::dropbox::dropbox_status,
            commands::dropbox::dropbox_connect,
            commands::dropbox::dropbox_connect_another,
            commands::dropbox::dropbox_reopen_connect_page,
            commands::dropbox::dropbox_cancel_connect,
            commands::dropbox::dropbox_disconnect,
            commands::dropbox::dropbox_list_folder,
            commands::browser::approve_pairing,
            commands::browser::deny_pairing,
            commands::browser::answer_access,
            commands::browser::save_login,
            commands::browser::cancel_login,
            commands::window::quit,
        ])
        .events(collect_events![
            WorkspaceChanged,
            MergeNotice,
            BrowserRequest,
            ClipboardCleared,
            SaveFailed,
            Launched,
        ])
}

/// The header of the generated bindings.
#[cfg(test)]
pub const BINDINGS_HEADER: &str = "// Generated by tauri-specta from crates/app/src/app.rs. Do not edit:\n\
     // run `pnpm bindings` in crates/app/ui after changing a command, DTO or event.\n";

/// Emits events into the webview, and brings the window to the front.
struct TauriNotifier(AppHandle);

impl Notifier for TauriNotifier {
    fn emit(&self, event: AppEvent) {
        let app = &self.0;
        let emitted = match event {
            AppEvent::WorkspaceChanged(event) => event.emit(app),
            AppEvent::MergeNotice(event) => event.emit(app),
            AppEvent::BrowserRequest(event) => event.emit(app),
            AppEvent::ClipboardCleared(event) => event.emit(app),
            AppEvent::SaveFailed(event) => event.emit(app),
            AppEvent::Launched(event) => event.emit(app),
        };
        if let Err(error) = emitted {
            eprintln!("keefast: can't tell the window: {error}");
        }
    }

    fn bring_to_front(&self) {
        show_main_window(&self.0);
    }
}

/// Shows keefast's window, hidden in the Tray or minimized, and focuses it.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// keefast was launched again: the running keefast gets that launch's command line (`argv`, run
/// from `cwd`), and the launch exits.
fn launched_again(app: &AppHandle, argv: Vec<String>, cwd: String) {
    let cwd = Path::new(&cwd);
    // Paths are relative to where the later launch ran, not to where keefast did.
    let args = Args::parse(argv.into_iter().skip(1).map(|arg| {
        if arg.starts_with("--") || Path::new(&arg).is_absolute() {
            OsString::from(arg)
        } else {
            cwd.join(arg).into_os_string()
        }
    }));
    // Start on boot while keefast runs already: it stays as it is.
    if args.hidden {
        return;
    }
    match app.try_state::<Arc<AppCore>>() {
        Some(core) => core.launched_again(Launch {
            vault_file: args.vault_file,
            key_file: args.key_file,
        }),
        None => show_main_window(app),
    }
}

/// The Workspace with its environment, as the GPUI app made it.
fn workspace() -> Workspace {
    // Nothing is downloaded until the user turns favicon download on.
    let mut workspace =
        Workspace::new(Arc::new(LocalStorage)).with_favicon_source(Arc::new(HttpFavicons::new()));
    if let Some(config) = FileConfigStore::in_os_config_dir() {
        workspace = workspace.with_config(Arc::new(config));
    }
    if let Some(hosts) = NativeMessagingHosts::for_current_user() {
        workspace = workspace.with_browser_hosts(Arc::new(hosts));
    }
    if let Some(config) = dropbox_config(&workspace) {
        let dropbox = Dropbox::new(config, Arc::new(OsSecretStore));
        workspace = workspace.with_dropbox(Arc::new(dropbox));
    }
    if let Some(cache) = FileVaultCache::in_os_data_dir() {
        workspace = workspace.with_vault_cache(Arc::new(cache));
    }
    workspace
}

/// Copies `keefast-agent` out of an AppImage to where Agents find it between runs (spec story
/// 83), off the main thread: it is a few megabytes, and nothing waits for it.
fn install_agent_command() {
    let Some(command) = AgentCommand::for_current_user() else {
        return;
    };
    std::thread::spawn(move || {
        if let Err(error) = command.install() {
            eprintln!(
                "keefast: couldn't copy keefast-agent to {}: {error}",
                command.path().display()
            );
        }
    });
}

/// Which Dropbox app keefast connects as: the app key from the `KEEFAST_DROPBOX_APP_KEY`
/// environment variable, else the app config's `dropbox_app_key`, else the one keefast was built
/// with (`KEEFAST_DROPBOX_APP_KEY` at build time). Without one, Dropbox isn't offered. The
/// `KEEFAST_DROPBOX_*_URL` variables point keefast at another Dropbox, such as the fake one
/// (`cargo run -p keefast-workspace --example fake_dropbox`).
fn dropbox_config(workspace: &Workspace) -> Option<DropboxConfig> {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let app_key = env("KEEFAST_DROPBOX_APP_KEY")
        .or_else(|| workspace.dropbox_app_key().map(str::to_owned))
        .or_else(|| option_env!("KEEFAST_DROPBOX_APP_KEY").map(str::to_owned))
        .filter(|key| !key.is_empty())?;
    let mut config = DropboxConfig::new(app_key);
    if let Some(url) = env("KEEFAST_DROPBOX_API_URL") {
        config.api_url = url;
    }
    if let Some(url) = env("KEEFAST_DROPBOX_CONTENT_URL") {
        config.content_url = url;
    }
    if let Some(url) = env("KEEFAST_DROPBOX_AUTHORIZE_URL") {
        config.authorize_url = url;
    }
    Some(config)
}

/// Shows the window, which `tauri.conf.json` creates hidden so a `--hidden` launch never shows
/// it, not even for a moment before it is minimized. `close_to_tray` is the setting, and the
/// Tray icon already exists when it is on and the desktop has a Tray.
fn show_window(app: &AppHandle, hidden: bool, close_to_tray: bool) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    if !hidden {
        let _ = window.show();
        return;
    }
    match autostart::hidden_launch(close_to_tray, tray::tray_available()) {
        // The Tray icon shows the window when the user asks for it.
        HiddenLaunch::TrayOnly => {}
        // Minimized before it is shown, so it appears only in the taskbar.
        HiddenLaunch::Minimized => {
            let _ = window.minimize();
            let _ = window.show();
        }
    }
}

pub fn run(args: Args) {
    let builder = specta_builder();
    let hidden = args.hidden;
    let launch = Launch {
        vault_file: args.vault_file,
        key_file: args.key_file,
    };
    let app = tauri::Builder::default()
        // First, so a later launch hands its command line over before anything else starts.
        .plugin(tauri_plugin_single_instance::init(launched_again))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    // Linux writes its own Start on boot entry (see `autostart`).
    #[cfg(not(target_os = "linux"))]
    let app = app.plugin(autostart::plugin());
    let app = app
        .manage(Connecting::default())
        .invoke_handler(builder.invoke_handler())
        .on_window_event(tray::on_window_event)
        .register_uri_scheme_protocol(icon_protocol::SCHEME, |ctx, request| {
            match ctx.app_handle().try_state::<Arc<AppCore>>() {
                Some(core) => icon_protocol::respond(&core, request.uri().path()),
                None => icon_protocol::respond_missing(),
            }
        })
        .setup(move |app| {
            builder.mount_events(app);
            let handle = app.handle().clone();
            let core = AppCore::new(workspace(), Box::new(TauriNotifier(handle.clone())), launch);
            if let Some(store) = PluginStore::in_os_data_dir() {
                core.use_plugin_store(store);
            }
            app.manage(Arc::clone(&core));
            apply_window_theme(&handle, core.read(Workspace::theme));
            // Whether the desktop has a Tray, checked once at startup and again when Close to
            // tray is turned on.
            tray::check_tray();
            tray::sync(&handle, core.read(Workspace::close_to_tray));
            background::start(&core);
            install_agent_command();
            if let Some(file) = AgentHosts::file_in_os_config_dir() {
                agent_hosts::start(&core, file, Arc::new(OsSecretStore));
            }
            if core.read(Workspace::browser_integration) {
                browser::serve(&core);
            }
            show_window(&handle, hidden, core.read(Workspace::close_to_tray));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("keefast could not start its window");
    app.run(|app, event| {
        // Nothing keefast copied outlives it.
        if let RunEvent::Exit = event
            && let Some(core) = app.try_state::<Arc<AppCore>>()
        {
            core.clear_clipboard_now();
        }
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use specta_typescript::Typescript;

    use super::*;

    /// Fails when `ui/src/bindings.ts` doesn't match the commands, DTOs and events. With
    /// `KEEFAST_UPDATE_BINDINGS=1` (`pnpm bindings`), it writes them instead.
    #[test]
    fn bindings_are_up_to_date() {
        let committed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/src/bindings.ts");
        let fresh = tempfile::NamedTempFile::new().unwrap();
        specta_builder()
            .export(Typescript::default().header(BINDINGS_HEADER), fresh.path())
            .expect("the bindings export");
        let fresh = std::fs::read_to_string(fresh.path()).unwrap();
        if std::env::var_os("KEEFAST_UPDATE_BINDINGS").is_some() {
            std::fs::write(&committed, &fresh).unwrap();
            return;
        }
        // A Windows checkout may have turned the file's line endings into CRLF.
        let committed = std::fs::read_to_string(&committed)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            committed == fresh,
            "crates/app/ui/src/bindings.ts is out of date: run `pnpm bindings` in crates/app/ui"
        );
    }
}
