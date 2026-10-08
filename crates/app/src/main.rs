//! keefast's desktop app: GPUI views that render Workspace state and send it commands.

mod browser_integration;
mod entry_editor;
mod pairing_dialog;
mod saving;
mod strings;
mod text_field;
mod theme;
mod unlock;
mod vault_browser;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, Application, Bounds, Context, Entity, Focusable, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgba, size,
};
use keefast_workspace::{
    FileConfigStore, LocalStorage, NativeMessagingHosts, VaultLocation, Workspace,
};

use crate::browser_integration::BrowserIntegration;
use crate::pairing_dialog::{PairingDialog, PairingDialogEvent};
use crate::unlock::{UnlockEvent, UnlockView};
use crate::vault_browser::{OpenAnotherVault, VaultBrowser};

/// The main window: the unlock dialog until a Vault is open, then the three panes, with the
/// unlock dialog shown again to open another Vault. Pairing requests from browsers are asked
/// over whatever is showing.
struct KeefastWindow {
    workspace: Entity<Workspace>,
    integration: Entity<BrowserIntegration>,
    unlock: Entity<UnlockView>,
    browser: Option<Entity<VaultBrowser>>,
    showing_unlock: bool,
    pairing: Option<Entity<PairingDialog>>,
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
        cx.observe_in(&integration, window, Self::show_pending_pairing)
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
        }
    }

    /// Shows the dialog for the Pairing request to answer now, if there is one.
    fn show_pending_pairing(
        &mut self,
        integration: Entity<BrowserIntegration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = integration.read(cx).pending_pairing(cx);
        let showing = self
            .pairing
            .as_ref()
            .map(|dialog| dialog.read(cx).pairing());
        if pending.map(|pending| pending.id) == showing {
            return;
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
            window.focus(&dialog.focus_handle(cx));
            dialog
        });
        if self.pairing.is_none() {
            self.focus_screen(window, cx);
        }
        cx.notify();
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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let screen = match (&self.browser, self.showing_unlock) {
            (Some(browser), false) => browser.clone().into_any_element(),
            _ => self.unlock.clone().into_any_element(),
        };
        div().size_full().relative().child(screen).when_some(
            self.pairing.clone(),
            |this, dialog| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgba(0x00000080))
                        .child(dialog),
                )
            },
        )
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
