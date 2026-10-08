//! keefast's desktop app: GPUI views that render Workspace state and send it commands.

mod access_dialog;
mod actions;
mod browser_integration;
mod clipboard;
mod command_palette;
mod dropbox_panel;
mod entry_editor;
mod locking;
mod login_dialog;
mod merge_notice;
mod pairing_dialog;
mod password_generator;
mod saving;
mod strings;
mod text_field;
mod theme;
mod totp_setup;
mod unlock;
mod vault_browser;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyView, App, Application, Bounds, Context, Entity, FocusHandle, Focusable, PromptLevel,
    Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgba,
    size,
};
use keefast_workspace::{
    Dropbox, DropboxConfig, FileConfigStore, LocalStorage, NativeMessagingHosts, OsSecretStore,
    UnsavedLock, VaultId, VaultLocation, Workspace,
};

use crate::access_dialog::{AccessDialog, AccessDialogEvent, AskedEntry};
use crate::actions::ToggleCommandPalette;
use crate::browser_integration::BrowserIntegration;
use crate::clipboard::AppClipboard;
use crate::command_palette::{CommandPalette, Dismissed};
use crate::login_dialog::{LoginDialog, LoginDialogEvent};
use crate::pairing_dialog::{PairingDialog, PairingDialogEvent};
use crate::unlock::{UnlockEvent, UnlockView};
use crate::vault_browser::{OpenAnotherVault, UnlockLockedVault, VaultBrowser};

/// The main window: the unlock dialog until a Vault is open, then the three panes, with the
/// unlock dialog shown again to open another Vault, or to unlock one, and whenever every Vault
/// is locked. The command palette opens over it, and Pairing and Access requests and captured
/// logins from browsers are asked over everything, in that order.
struct KeefastWindow {
    workspace: Entity<Workspace>,
    integration: Entity<BrowserIntegration>,
    unlock: Entity<UnlockView>,
    browser: Option<Entity<VaultBrowser>>,
    showing_unlock: bool,
    pairing: Option<Entity<PairingDialog>>,
    access: Option<Entity<AccessDialog>>,
    login: Option<Entity<LoginDialog>>,
    palette: Option<Entity<CommandPalette>>,
    /// The Vault whose unsaved changes the user is being asked about before it locks.
    asking_lock: Option<VaultId>,
    _keystrokes: Subscription,
}

impl KeefastWindow {
    fn new(args: Args, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|_| {
            let mut workspace = Workspace::new(Arc::new(LocalStorage));
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
            workspace
        });
        // Nothing keefast copied outlives it.
        let quitting = workspace.clone();
        App::on_app_quit(cx, move |cx| {
            quitting.update(cx, |workspace, cx| {
                workspace.clear_clipboard_now(&mut AppClipboard(cx))
            });
            async {}
        })
        .detach();
        saving::watch_vault_files(&workspace, cx);
        locking::watch(&workspace, cx);
        // Any key typed in keefast counts as activity, wherever it goes.
        let typing = workspace.clone();
        let keystrokes =
            cx.intercept_keystrokes(move |_, _, cx| locking::note_activity(&typing, cx));
        cx.observe_in(&workspace, window, Self::on_workspace_changed)
            .detach();
        let integration = cx.new(|cx| BrowserIntegration::new(workspace.clone(), cx));
        cx.observe_in(&integration, window, Self::show_pending_requests)
            .detach();
        let location = args.vault_file.map(VaultLocation::Local);
        let unlock = cx.new(|cx| UnlockView::new(workspace.clone(), location, args.key_file, cx));
        cx.subscribe_in(&unlock, window, Self::on_unlock_event)
            .detach();
        window.focus(&unlock.focus_handle(cx));
        Self {
            workspace,
            integration,
            unlock,
            browser: None,
            showing_unlock: true,
            pairing: None,
            access: None,
            login: None,
            palette: None,
            asking_lock: None,
            _keystrokes: keystrokes,
        }
    }

    fn toggle_command_palette(
        &mut self,
        _: &ToggleCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            self.close_command_palette(window, cx);
            return;
        }
        // A browser request is answered first.
        if self.dialog(cx).is_some() {
            return;
        }
        let palette = cx.new(|cx| CommandPalette::new(window, cx));
        cx.subscribe(&palette, |this, _, _: &Dismissed, cx| {
            this.palette = None;
            cx.notify();
        })
        .detach();
        self.palette = Some(palette);
        cx.notify();
    }

    fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.take() {
            palette.update(cx, |palette, cx| palette.dismiss(window, cx));
            cx.notify();
        }
    }

    /// Shows the unlock dialog once every Vault is locked, and asks about the unsaved changes
    /// of a Vault waiting to lock.
    fn on_workspace_changed(
        &mut self,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (all_locked, to_unlock, pending_lock) = {
            let workspace = workspace.read(cx);
            let to_unlock = workspace.vault_to_unlock();
            let all_locked = workspace.vaults().next().is_none() && to_unlock.is_some();
            (all_locked, to_unlock, workspace.pending_lock())
        };
        if all_locked && !self.showing_unlock {
            if let Some(vault) = to_unlock {
                self.unlock
                    .update(cx, |unlock, cx| unlock.unlock_locked(vault, cx));
            }
            self.showing_unlock = true;
            if self.dialog(cx).is_none() {
                window.focus(&self.unlock.focus_handle(cx));
            }
            cx.notify();
        }
        if pending_lock != self.asking_lock {
            self.asking_lock = pending_lock;
            if let Some(vault) = pending_lock {
                self.ask_lock(vault, window, cx);
            }
        }
    }

    /// Asks whether to save a Vault's unsaved changes before it locks, as KeePassXC does.
    fn ask_lock(&mut self, vault: VaultId, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .workspace
            .read(cx)
            .vault(vault)
            .map(|open| open.display_name())
            .unwrap_or_default();
        let question = strings::LOCK_UNSAVED.replace("{vault}", &name);
        // A lock the user asked for can be called off. One that idle time, the OS locking or a
        // suspend asked for can't: whoever is at the keyboard may not be the user, so the
        // Vault stays hidden until the changes are saved or discarded.
        let can_cancel = self.workspace.read(cx).can_cancel_lock(vault);
        let buttons: &[&str] = match can_cancel {
            true => &[strings::LOCK_SAVE, strings::LOCK_DISCARD, strings::CANCEL],
            false => &[strings::LOCK_SAVE, strings::LOCK_DISCARD],
        };
        let detail = match can_cancel {
            true => strings::LOCK_UNSAVED_DETAIL,
            false => strings::LOCK_UNSAVED_AUTO_DETAIL,
        };
        let answer = window.prompt(PromptLevel::Warning, &question, Some(detail), buttons, cx);
        let workspace = self.workspace.clone();
        cx.spawn(async move |this, cx| {
            let answer = match answer.await {
                Ok(0) => UnsavedLock::Save,
                Ok(1) => UnsavedLock::Discard,
                _ => UnsavedLock::Cancel,
            };
            cx.update(|cx| {
                // Asked again if the Vault is still waiting, as after a failed save.
                this.update(cx, |this, _| this.asking_lock = None).ok();
                locking::answer(&workspace, vault, answer, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Shows the dialog for the browser request to answer now, if there is one: a Pairing
    /// request, or else an Access request, or else a captured login.
    fn show_pending_requests(
        &mut self,
        integration: Entity<BrowserIntegration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.show_pending_pairing(&integration, window, cx)
            | self.show_pending_access(&integration, window, cx)
            | self.show_pending_login(&integration, window, cx);
        if !changed {
            return;
        }
        match self.dialog(cx) {
            Some((_, focus)) => {
                // The request is answered first; the palette doesn't wait under it.
                if let Some(palette) = self.palette.take() {
                    palette.update(cx, |palette, cx| palette.close(cx));
                }
                window.focus(&focus);
                // The user is in the browser when it asks, so keefast comes to the front.
                window.activate_window();
            }
            None => self.focus_screen(window, cx),
        }
        cx.notify();
    }

    /// The browser request dialog showing, and its focus: a Pairing request's over an Access
    /// request's over a captured login's.
    fn dialog(&self, cx: &App) -> Option<(AnyView, FocusHandle)> {
        match (&self.pairing, &self.access, &self.login) {
            (Some(pairing), ..) => Some((pairing.clone().into(), pairing.focus_handle(cx))),
            (None, Some(access), _) => Some((access.clone().into(), access.focus_handle(cx))),
            (None, None, Some(login)) => Some((login.clone().into(), login.focus_handle(cx))),
            (None, None, None) => None,
        }
    }

    /// Shows the dialog for the Pairing request to answer now, if there is one, and returns
    /// whether that changed.
    fn show_pending_pairing(
        &mut self,
        integration: &Entity<BrowserIntegration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pending = integration.read(cx).pending_pairing(cx);
        let showing = self
            .pairing
            .as_ref()
            .map(|dialog| dialog.read(cx).pairing());
        if pending.map(|pending| pending.id) == showing {
            return false;
        }
        self.pairing = pending.map(|pending| {
            let vault_name = self
                .workspace
                .read(cx)
                .vault(pending.vault)
                .map(|open| open.display_name())
                .unwrap_or_default();
            let dialog = cx.new(|cx| PairingDialog::new(pending.id, vault_name, cx));
            cx.subscribe_in(&dialog, window, Self::on_pairing_event)
                .detach();
            dialog
        });
        true
    }

    /// Shows the dialog for the Access request to answer now, if there is one, and returns
    /// whether that changed.
    fn show_pending_access(
        &mut self,
        integration: &Entity<BrowserIntegration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pending = integration.read(cx).pending_access(cx);
        let showing = self.access.as_ref().map(|dialog| dialog.read(cx).access());
        if pending.as_ref().map(|pending| pending.id) == showing {
            return false;
        }
        self.access = pending.map(|pending| {
            let workspace = self.workspace.read(cx);
            let entries = pending
                .entries
                .iter()
                .filter_map(|&(vault, entry)| {
                    let open = workspace.vault(vault)?;
                    let entry = open.vault().entry(entry)?;
                    Some(AskedEntry {
                        title: entry.title().to_owned().into(),
                        username: entry.username().to_owned().into(),
                        vault: open.display_name().into(),
                    })
                })
                .collect();
            let dialog = cx.new(|cx| AccessDialog::new(pending.id, pending.site, entries, cx));
            cx.subscribe_in(&dialog, window, Self::on_access_event)
                .detach();
            dialog
        });
        true
    }

    /// Shows the dialog for the captured login to save now, if there is one, and returns
    /// whether that changed.
    fn show_pending_login(
        &mut self,
        integration: &Entity<BrowserIntegration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pending = integration.read(cx).pending_login(cx);
        let showing = self.login.as_ref().map(|dialog| dialog.read(cx).login());
        if pending.as_ref().map(|pending| pending.id) == showing {
            return false;
        }
        self.login = pending.map(|pending| {
            let rows = login_dialog::rows_for(self.workspace.read(cx), &pending);
            let dialog = cx.new(|cx| {
                LoginDialog::new(
                    pending.id,
                    pending.site,
                    pending.username,
                    rows,
                    pending.suggested,
                    cx,
                )
            });
            cx.subscribe_in(&dialog, window, Self::on_login_event)
                .detach();
            dialog
        });
        true
    }

    fn on_login_event(
        &mut self,
        dialog: &Entity<LoginDialog>,
        event: &LoginDialogEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let login = dialog.read(cx).login();
        match event {
            &LoginDialogEvent::Save(target) => {
                let result = self.integration.update(cx, |integration, cx| {
                    integration.save_login(login, target, cx)
                });
                if let Err(error) = result {
                    dialog.update(cx, |dialog, cx| {
                        dialog.set_error(vault_browser::describe(&error), cx)
                    });
                }
            }
            LoginDialogEvent::Cancel => self
                .integration
                .update(cx, |integration, cx| integration.cancel_login(login, cx)),
        }
    }

    fn on_access_event(
        &mut self,
        dialog: &Entity<AccessDialog>,
        AccessDialogEvent(answer): &AccessDialogEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let access = dialog.read(cx).access();
        self.integration.update(cx, |integration, cx| {
            integration.answer_access(access, *answer, cx)
        });
    }

    fn on_pairing_event(
        &mut self,
        dialog: &Entity<PairingDialog>,
        event: &PairingDialogEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pairing = dialog.read(cx).pairing();
        match event {
            PairingDialogEvent::Approve(name) => {
                let result = self.integration.update(cx, |integration, cx| {
                    integration.approve_pairing(pairing, name, cx)
                });
                if let Err(error) = result {
                    dialog.update(cx, |dialog, cx| {
                        dialog.set_error(vault_browser::describe(&error), cx)
                    });
                }
            }
            PairingDialogEvent::Deny => self
                .integration
                .update(cx, |integration, cx| integration.deny_pairing(pairing, cx)),
        }
    }

    fn focus_screen(&self, window: &mut Window, cx: &mut Context<Self>) {
        match (&self.browser, self.showing_unlock) {
            (Some(browser), false) => window.focus(&browser.focus_handle(cx)),
            _ => window.focus(&self.unlock.focus_handle(cx)),
        }
    }

    fn on_unlock_event(
        &mut self,
        _: &Entity<UnlockView>,
        event: &UnlockEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let browser = match (event, &self.browser) {
            (UnlockEvent::Cancelled, None) => return,
            (UnlockEvent::Cancelled, Some(browser)) => browser.clone(),
            (UnlockEvent::Unlocked, Some(browser)) => {
                browser.update(cx, |browser, cx| browser.clear_detail(cx));
                browser.clone()
            }
            (UnlockEvent::Unlocked, None) => {
                let workspace = self.workspace.clone();
                let integration = self.integration.clone();
                let browser = cx.new(|cx| VaultBrowser::new(workspace, integration, window, cx));
                cx.subscribe_in(&browser, window, Self::on_open_another)
                    .detach();
                cx.subscribe_in(&browser, window, Self::on_unlock_locked)
                    .detach();
                self.browser = Some(browser.clone());
                browser
            }
        };
        window.focus(&browser.focus_handle(cx));
        self.showing_unlock = false;
        cx.notify();
    }

    fn on_open_another(
        &mut self,
        _: &Entity<VaultBrowser>,
        _: &OpenAnotherVault,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unlock.update(cx, |unlock, cx| unlock.reset(cx));
        window.focus(&self.unlock.focus_handle(cx));
        self.showing_unlock = true;
        cx.notify();
    }

    fn on_unlock_locked(
        &mut self,
        _: &Entity<VaultBrowser>,
        &UnlockLockedVault(vault): &UnlockLockedVault,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unlock
            .update(cx, |unlock, cx| unlock.unlock_locked(vault, cx));
        window.focus(&self.unlock.focus_handle(cx));
        self.showing_unlock = true;
        cx.notify();
    }
}

impl Render for KeefastWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = match (&self.browser, self.showing_unlock) {
            (Some(browser), false) => browser.clone().into_any_element(),
            _ => self.unlock.clone().into_any_element(),
        };
        let dialog = self.dialog(cx).map(|(view, _)| view);
        div()
            .size_full()
            .relative()
            .on_action(cx.listener(Self::toggle_command_palette))
            // Clicks and scrolling anywhere in keefast count as activity.
            .capture_any_mouse_down(
                cx.listener(|this, _, _, cx| locking::note_activity(&this.workspace, cx)),
            )
            .on_scroll_wheel(
                cx.listener(|this, _, _, cx| locking::note_activity(&this.workspace, cx)),
            )
            .child(screen)
            .when_some(self.palette.clone(), |this, palette| {
                // Clicking beside the palette closes it.
                this.child(
                    div()
                        .id("palette-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .flex()
                        .justify_center()
                        .items_start()
                        .pt(px(80.))
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.close_command_palette(window, cx)
                            }),
                        )
                        .child(palette),
                )
            })
            .when_some(dialog, |this, dialog| {
                // The backdrop keeps clicks on and around the dialog from reaching the screen.
                this.child(
                    div()
                        .id("dialog-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgba(0x00000080))
                        .child(dialog),
                )
            })
    }
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

/// The command line: `keefast [VAULT_FILE] [--key-file KEY_FILE]`.
#[derive(Default)]
struct Args {
    vault_file: Option<PathBuf>,
    key_file: Option<PathBuf>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = std::ffi::OsString>) -> Self {
        // Recent Vault files are remembered by full path, so relative paths are made absolute.
        let absolute = |path: std::ffi::OsString| {
            std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(path))
        };
        let mut parsed = Args::default();
        while let Some(arg) = args.next() {
            if arg == "--key-file" {
                parsed.key_file = args.next().map(absolute);
            } else if arg.to_string_lossy().starts_with("--") {
                eprintln!("keefast: ignoring unknown option {}", arg.to_string_lossy());
            } else if parsed.vault_file.is_none() {
                parsed.vault_file = Some(absolute(arg));
            }
        }
        parsed
    }
}

fn main() {
    let args = Args::parse(std::env::args_os().skip(1));
    Application::new().run(move |cx: &mut App| {
        actions::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(strings::APP_NAME.into()),
                    ..Default::default()
                }),
                app_id: Some(strings::APP_NAME.into()),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| KeefastWindow::new(args, window, cx)),
        )
        .expect("failed to open the main window");
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}
