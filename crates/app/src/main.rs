//! keefast's desktop app: GPUI views that render Workspace state and send it commands.

mod entry_editor;
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
    WindowOptions, div, prelude::*, px, size,
};
use keefast_workspace::{FileConfigStore, LocalStorage, VaultLocation, Workspace};

use crate::unlock::{UnlockEvent, UnlockView};
use crate::vault_browser::{OpenAnotherVault, VaultBrowser};

/// The main window: the unlock dialog until a Vault is open, then the three panes, with the
/// unlock dialog shown again to open another Vault.
struct KeefastWindow {
    workspace: Entity<Workspace>,
    unlock: Entity<UnlockView>,
    browser: Option<Entity<VaultBrowser>>,
    showing_unlock: bool,
}

impl KeefastWindow {
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|_| {
            let workspace = Workspace::new(Arc::new(LocalStorage));
            match FileConfigStore::in_os_config_dir() {
                Some(config) => workspace.with_config(Arc::new(config)),
                None => workspace,
            }
        });
        let location = path.map(VaultLocation::Local);
        let unlock = cx.new(|cx| UnlockView::new(workspace.clone(), location, cx));
        cx.subscribe_in(&unlock, window, Self::on_unlock_event)
            .detach();
        window.focus(&unlock.focus_handle(cx));
        Self {
            workspace,
            unlock,
            browser: None,
            showing_unlock: true,
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
                let browser = cx.new(|cx| VaultBrowser::new(workspace, vault, cx));
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
        div().size_full().child(screen)
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
