//! keefast's desktop app: GPUI views that render Workspace state and send it commands.

mod access_dialog;
mod browser_integration;
mod entry_editor;
mod login_dialog;
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
    AnyView, App, Application, Bounds, Context, Entity, FocusHandle, Focusable, TitlebarOptions,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, rgba, size,
};
use keefast_workspace::{
    FileConfigStore, LocalStorage, NativeMessagingHosts, VaultLocation, Workspace,
};

use crate::access_dialog::{AccessDialog, AccessDialogEvent, AskedEntry};
use crate::browser_integration::BrowserIntegration;
use crate::login_dialog::{LoginDialog, LoginDialogEvent};
use crate::pairing_dialog::{PairingDialog, PairingDialogEvent};
use crate::unlock::{UnlockEvent, UnlockView};
use crate::vault_browser::{OpenAnotherVault, VaultBrowser};

/// The main window: the unlock dialog until a Vault is open, then the three panes, with the
/// unlock dialog shown again to open another Vault. Pairing and Access requests and captured
/// logins from browsers are asked over whatever is showing, in that order.
struct KeefastWindow {
    workspace: Entity<Workspace>,
    integration: Entity<BrowserIntegration>,
    unlock: Entity<UnlockView>,
    browser: Option<Entity<VaultBrowser>>,
    showing_unlock: bool,
    pairing: Option<Entity<PairingDialog>>,
    access: Option<Entity<AccessDialog>>,
    login: Option<Entity<LoginDialog>>,
}

impl KeefastWindow {
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|_| {
            let mut workspace = Workspace::new(Arc::new(LocalStorage));
            if let Some(config) = FileConfigStore::in_os_config_dir() {
                workspace = workspace.with_config(Arc::new(config));
            }
            if let Some(hosts) = NativeMessagingHosts::for_current_user() {
                workspace = workspace.with_browser_hosts(Arc::new(hosts));
            }
            workspace
        });
        let integration = cx.new(|cx| BrowserIntegration::new(workspace.clone(), cx));
        cx.observe_in(&integration, window, Self::show_pending_requests)
            .detach();
        let location = path.map(VaultLocation::Local);
        let unlock = cx.new(|cx| UnlockView::new(workspace.clone(), location, cx));
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
        }
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
            (&UnlockEvent::Unlocked(vault), Some(browser)) => {
                browser.update(cx, |browser, cx| browser.show_vault(vault, cx));
                browser.clone()
            }
            (&UnlockEvent::Unlocked(vault), None) => {
                let workspace = self.workspace.clone();
                let integration = self.integration.clone();
                let browser = cx.new(|cx| VaultBrowser::new(workspace, integration, vault, cx));
                cx.subscribe_in(&browser, window, Self::on_open_another)
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
            .child(screen)
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

fn main() {
    // Recent Vault files are remembered by full path, so a relative path is made absolute.
    let path = std::env::args_os()
        .nth(1)
        .map(|path| std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(path)));
    Application::new().run(move |cx: &mut App| {
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
            |window, cx| cx.new(|cx| KeefastWindow::new(path, window, cx)),
        )
        .expect("failed to open the main window");
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}
