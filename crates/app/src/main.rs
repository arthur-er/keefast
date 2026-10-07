//! keefast's desktop app: GPUI views that render Workspace state and send it commands.

mod strings;
mod theme;
mod unlock;
mod vault_browser;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, Application, Bounds, Context, Entity, Focusable, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};
use keefast_workspace::{LocalStorage, Workspace};

use crate::unlock::{UnlockView, Unlocked};
use crate::vault_browser::VaultBrowser;

/// The main window: the unlock dialog until a Vault is open, then the three panes.
struct KeefastWindow {
    workspace: Entity<Workspace>,
    screen: Screen,
}

enum Screen {
    Unlock(Entity<UnlockView>),
    Browse(Entity<VaultBrowser>),
}

impl KeefastWindow {
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|_| Workspace::new(Arc::new(LocalStorage)));
        let unlock = cx.new(|cx| UnlockView::new(workspace.clone(), path, cx));
        cx.subscribe_in(&unlock, window, Self::on_unlocked).detach();
        window.focus(&unlock.focus_handle(cx));
        Self {
            workspace,
            screen: Screen::Unlock(unlock),
        }
    }

    fn on_unlocked(
        &mut self,
        _: &Entity<UnlockView>,
        &Unlocked(vault): &Unlocked,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        self.screen = Screen::Browse(cx.new(|cx| VaultBrowser::new(workspace, vault, cx)));
        cx.notify();
    }
}

impl Render for KeefastWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let screen = match &self.screen {
            Screen::Unlock(view) => view.clone().into_any_element(),
            Screen::Browse(view) => view.clone().into_any_element(),
        };
        div().size_full().child(screen)
    }
}

fn main() {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
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
