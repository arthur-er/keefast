use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use keefast_core::DEFAULT_BROWSER_GROUP;
use keefast_workspace::{LoginId, LoginTarget, PendingLogin, Workspace};

use crate::strings;
use crate::theme::Theme;
use crate::ui::{self, Button, Dialog, Typography};
use crate::vault_browser::flatten_groups;

pub enum LoginDialogEvent {
    Save(LoginTarget),
    Cancel,
}

/// A line of the dialog's list: a heading, or a place the login can go.
pub struct LoginRow {
    /// Where the login goes when this row is chosen; `None` for a heading.
    pub target: Option<LoginTarget>,
    pub label: SharedString,
    /// Shown muted after the label: a username or a Vault's name.
    pub detail: Option<SharedString>,
    pub depth: usize,
}

/// The places a captured login can go: the Entries it could update, then every Group of each
/// Vault it may go to, with the Browser group to be made when a Vault has none yet.
pub fn rows_for(workspace: &Workspace, pending: &PendingLogin) -> Vec<LoginRow> {
    let heading = |label: &str| LoginRow {
        target: None,
        label: label.to_owned().into(),
        detail: None,
        depth: 0,
    };
    let mut rows = Vec::new();
    if !pending.entries.is_empty() {
        rows.push(heading(strings::LOGIN_UPDATE));
    }
    for &(vault, entry) in &pending.entries {
        let Some(open) = workspace.vault(vault) else {
            continue;
        };
        let Some(found) = open.vault().entry(entry) else {
            continue;
        };
        let title = match found.title() {
            "" => strings::UNTITLED.to_owned(),
            title => title.to_owned(),
        };
        rows.push(LoginRow {
            target: Some(LoginTarget::Update { vault, entry }),
            label: format!("{title} · {}", found.username()).into(),
            detail: Some(open.display_name().into()),
            depth: 1,
        });
    }
    rows.push(heading(strings::LOGIN_NEW));
    for &vault in &pending.vaults {
        let Some(open) = workspace.vault(vault) else {
            continue;
        };
        let browser_group = open.vault().browser_group();
        let new_entry = |group| Some(LoginTarget::NewEntry { vault, group });
        let root = open.vault().root();
        rows.push(LoginRow {
            target: new_entry(Some(root.id())),
            label: open.display_name().into(),
            detail: None,
            depth: 1,
        });
        if browser_group.is_none() {
            rows.push(LoginRow {
                target: new_entry(None),
                label: strings::LOGIN_NEW_BROWSER_GROUP
                    .replace("{name}", DEFAULT_BROWSER_GROUP)
                    .into(),
                detail: Some(strings::BROWSER_GROUP_MARK.into()),
                depth: 2,
            });
        }
        let mut groups = Vec::new();
        flatten_groups(root.groups(), 2, &mut groups);
        for (group, depth) in groups {
            if open.vault().is_group_in_recycle_bin(group.id()) != Ok(false) {
                continue;
            }
            rows.push(LoginRow {
                target: new_entry(Some(group.id())),
                label: group.name().to_owned().into(),
                detail: (browser_group == Some(group.id()))
                    .then(|| strings::BROWSER_GROUP_MARK.into()),
                depth,
            });
        }
    }
    rows
}

/// Asks the user where a login captured in the browser goes: an Entry to update, or a Group
/// for a new Entry, with the suggested place chosen. Enter saves there; Escape cancels.
pub struct LoginDialog {
    login: LoginId,
    site: SharedString,
    username: SharedString,
    rows: Vec<LoginRow>,
    chosen: LoginTarget,
    error: Option<SharedString>,
    focus: FocusHandle,
}

impl EventEmitter<LoginDialogEvent> for LoginDialog {}

impl Focusable for LoginDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LoginDialog {
    pub fn new(
        login: LoginId,
        site: String,
        username: String,
        rows: Vec<LoginRow>,
        suggested: LoginTarget,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            login,
            site: site.into(),
            username: username.into(),
            rows,
            chosen: suggested,
            error: None,
            focus: cx.focus_handle(),
        }
    }

    /// The captured login this dialog asks about.
    pub fn login(&self) -> LoginId {
        self.login
    }

    pub fn set_error(&mut self, error: &'static str, cx: &mut Context<Self>) {
        self.error = Some(error.into());
        cx.notify();
    }

    fn choose(&mut self, target: LoginTarget, cx: &mut Context<Self>) {
        self.chosen = target;
        self.error = None;
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        cx.emit(LoginDialogEvent::Save(self.chosen));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(LoginDialogEvent::Cancel),
            "enter" => self.save(cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for LoginDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let rows = self.rows.iter().enumerate().map(|(ix, row)| {
            let line = div()
                .id(("login-row", ix))
                .flex()
                .justify_between()
                .gap_3()
                .pl(px(12. + 14. * row.depth as f32))
                .pr_3()
                .py_1()
                .child(div().truncate().child(row.label.clone()))
                .when_some(row.detail.clone(), |this, detail| {
                    this.child(
                        div()
                            .flex_none()
                            .text_secondary()
                            .text_color(theme.fg_1)
                            .child(detail),
                    )
                });
            match row.target {
                None => line.pt_2().text_micro().text_color(theme.fg_2),
                Some(target) => line
                    .cursor_pointer()
                    .rounded(px(6.))
                    .map(|this| match target == self.chosen {
                        true => this.bg(theme.bg_active).text_color(theme.fg_0),
                        false => this.hover(|this| this.bg(theme.bg_hover)),
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(target, cx))),
            }
        });
        let prompt = strings::LOGIN_PROMPT
            .replace("{site}", &self.site)
            .replace("{username}", &self.username);
        div()
            .id("login-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(ui::appear(
                "login-dialog-appear",
                div().child(
                    Dialog::new(strings::LOGIN_TITLE)
                        .description(prompt)
                        .width(px(480.))
                        .child(
                            div()
                                .id("login-targets")
                                .max_h(px(300.))
                                .overflow_y_scroll()
                                .p(px(4.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.bg_1)
                                .children(rows),
                        )
                        .when_some(self.error.clone(), |this, error| {
                            this.child(ui::error_text(error, theme))
                        })
                        .action(
                            Button::new("cancel-login", strings::CANCEL).on_click(
                                cx.listener(|_, _, _, cx| cx.emit(LoginDialogEvent::Cancel)),
                            ),
                        )
                        .action(
                            Button::new("save-login", strings::LOGIN_SAVE)
                                .primary()
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        ),
                ),
            ))
    }
}
