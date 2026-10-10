use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Window, div,
    prelude::*,
};
use keefast_core::{Entry, EntryId, Timestamp};
use keefast_workspace::{VaultId, Workspace};
use secrecy::ExposeSecret;

use crate::actions::{self, CloseHistory, RestoreVersion, SelectNewerVersion, SelectOlderVersion};
use crate::strings;
use crate::theme::Theme;
use crate::ui::{Button, Typography};
use crate::vault_browser::title_of;

pub enum EntryHistoryEvent {
    /// Restore the version at this index in the Entry's history, oldest first.
    Restore(usize),
    Close,
}

/// An Entry's earlier versions, shown in the detail pane: a list of them, newest first, and the
/// picked one read-only, to restore it if wanted.
pub struct EntryHistory {
    workspace: Entity<Workspace>,
    vault: VaultId,
    entry: EntryId,
    focus: FocusHandle,
    /// The version shown.
    selected: Option<Picked>,
    /// Whether the shown version's password and protected fields are shown.
    revealed: bool,
}

/// A version of the Entry, by its index in the history, oldest first, and when it was saved,
/// which finds it again when a Merge changes the history while it is shown.
#[derive(Clone, Copy, PartialEq)]
struct Picked {
    index: usize,
    saved: Option<Timestamp>,
}

impl EventEmitter<EntryHistoryEvent> for EntryHistory {}

impl Focusable for EntryHistory {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntryHistory {
    /// Shows the Entry's history with its newest earlier version picked.
    pub fn new(
        workspace: Entity<Workspace>,
        vault: VaultId,
        entry: EntryId,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        let mut history = Self {
            workspace,
            vault,
            entry,
            focus: cx.focus_handle(),
            selected: None,
            revealed: false,
        };
        if let Some(newest) = history.versions(cx).checked_sub(1) {
            history.select(newest, cx);
        }
        history
    }

    fn history<'a>(&self, cx: &'a App) -> &'a [Entry] {
        self.workspace
            .read(cx)
            .entry(self.vault, self.entry)
            .map_or(&[], Entry::history)
    }

    fn versions(&self, cx: &App) -> usize {
        self.history(cx).len()
    }

    /// Where the picked version is in the history now, if it is still there: a Merge may have
    /// added versions before it, or trimmed it.
    fn selected(&self, cx: &App) -> Option<usize> {
        let picked = self.selected?;
        let history = self.history(cx);
        let saved = |index: usize| history.get(index).map(Entry::last_modified);
        if saved(picked.index) == Some(picked.saved) {
            return Some(picked.index);
        }
        history
            .iter()
            .position(|version| version.last_modified() == picked.saved)
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(version) = self.history(cx).get(index) else {
            return;
        };
        let picked = Picked {
            index,
            saved: version.last_modified(),
        };
        if self.selected != Some(picked) {
            self.selected = Some(picked);
            self.revealed = false;
            cx.notify();
        }
    }

    fn select_older(&mut self, _: &SelectOlderVersion, _: &mut Window, cx: &mut Context<Self>) {
        let versions = self.versions(cx);
        match self.selected(cx) {
            Some(index) => self.select(index.saturating_sub(1), cx),
            None if versions > 0 => self.select(versions - 1, cx),
            None => {}
        }
    }

    fn select_newer(&mut self, _: &SelectNewerVersion, _: &mut Window, cx: &mut Context<Self>) {
        let versions = self.versions(cx);
        match self.selected(cx) {
            Some(index) => self.select((index + 1).min(versions - 1), cx),
            None if versions > 0 => self.select(versions - 1, cx),
            None => {}
        }
    }

    fn restore(&mut self, _: &RestoreVersion, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected(cx) {
            cx.emit(EntryHistoryEvent::Restore(index));
        }
    }

    fn render_version(
        &self,
        index: usize,
        version: &Entry,
        theme: Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let field = |label: &'static str, value: SharedString| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_secondary().text_color(theme.fg_1).child(label))
                .child(div().child(value))
        };
        let shown = |value: &str, protected: bool| -> SharedString {
            if protected && !self.revealed {
                strings::HIDDEN_PASSWORD.into()
            } else {
                value.to_owned().into()
            }
        };
        let reveal = div()
            .id("history-reveal")
            .cursor_pointer()
            .text_secondary()
            .text_color(theme.accent)
            .child(if self.revealed {
                strings::HIDE_PASSWORD
            } else {
                strings::REVEAL_PASSWORD
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.revealed = !this.revealed;
                cx.notify();
            }));
        let totp = match version.totp() {
            None => strings::HISTORY_TOTP_NONE,
            Some(Ok(_)) => strings::HISTORY_TOTP_SET,
            Some(Err(_)) => strings::HISTORY_TOTP_UNREADABLE,
        };

        div()
            .flex()
            .flex_col()
            .gap_3()
            .pt_3()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div().text_dialog_title().child(
                            strings::HISTORY_VERSION_OF
                                .replace("{time}", &modified_at(version.last_modified())),
                        ),
                    )
                    .child(
                        Button::new("history-restore", strings::HISTORY_RESTORE)
                            .primary()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(EntryHistoryEvent::Restore(index))
                            })),
                    ),
            )
            .child(field(strings::FIELD_TITLE, title_of(version)))
            .child(field(
                strings::DETAIL_USERNAME,
                version.username().to_owned().into(),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_secondary()
                                    .text_color(theme.fg_1)
                                    .child(strings::DETAIL_PASSWORD),
                            )
                            .child(reveal),
                    )
                    .child(div().child(shown(version.password().expose_secret(), true))),
            )
            .child(field(strings::DETAIL_URL, version.url().to_owned().into()))
            .child(field(
                strings::DETAIL_NOTES,
                version.notes().to_owned().into(),
            ))
            .child(field(strings::DETAIL_TOTP, totp.into()))
            .children(version.custom_fields().iter().map(|custom| {
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_secondary()
                            .text_color(theme.fg_1)
                            .truncate()
                            .child(custom.name.clone()),
                    )
                    .child(div().child(shown(custom.value.expose_secret(), custom.protected)))
            }))
    }
}

impl Render for EntryHistory {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let selected = self.selected(cx);
        let workspace = self.workspace.read(cx);
        let Some(entry) = workspace.entry(self.vault, self.entry) else {
            return div().into_any_element();
        };

        let row = |id: usize, time: String, summary: SharedString, picked: bool| {
            div()
                .id(("history-version", id))
                .flex()
                .justify_between()
                .gap_3()
                .px_2()
                .py_1()
                .rounded_md()
                .when(picked, |this| {
                    this.bg(theme.bg_active).text_color(theme.fg_0)
                })
                .child(div().flex_none().child(time))
                .child(
                    div()
                        .when(!picked, |this| this.text_color(theme.fg_1))
                        .truncate()
                        .child(summary),
                )
        };
        let current = row(
            usize::MAX,
            modified_at(entry.last_modified()),
            strings::HISTORY_CURRENT.into(),
            false,
        );
        let versions = entry
            .history()
            .iter()
            .enumerate()
            .rev()
            .map(|(index, version)| {
                row(
                    index,
                    modified_at(version.last_modified()),
                    summary(version),
                    selected == Some(index),
                )
                .cursor_pointer()
                .when(selected != Some(index), |this| {
                    this.hover(|this| this.bg(theme.bg_active))
                })
                .on_click(cx.listener(move |this, _, _, cx| this.select(index, cx)))
            });
        let picked = selected.map(|index| (index, &entry.history()[index]));

        div()
            .id("entry-history")
            .key_context(actions::HISTORY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_older))
            .on_action(cx.listener(Self::select_newer))
            .on_action(cx.listener(|_, _: &CloseHistory, _, cx| cx.emit(EntryHistoryEvent::Close)))
            .when(selected.is_some(), |this| {
                this.on_action(cx.listener(Self::restore))
            })
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_title()
                            .truncate()
                            .child(strings::HISTORY_TITLE.replace("{title}", &title_of(entry))),
                    )
                    .child(
                        Button::new("history-close", strings::HISTORY_CLOSE)
                            .small()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(EntryHistoryEvent::Close))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_secondary()
                    .child(current)
                    .children(versions),
            )
            .when(entry.history().is_empty(), |this| {
                this.child(
                    div()
                        .text_secondary()
                        .text_color(theme.fg_1)
                        .child(strings::HISTORY_EMPTY),
                )
            })
            .when_some(picked, |this, (index, version)| {
                this.child(self.render_version(index, version, theme, cx))
            })
            .into_any_element()
    }
}

/// When a version was saved, or that it isn't known.
fn modified_at(time: Option<Timestamp>) -> String {
    time.map_or_else(
        || strings::HISTORY_UNKNOWN_TIME.to_owned(),
        Timestamp::to_utc_string,
    )
}

/// A version's title and username, to tell versions apart in the list.
fn summary(version: &Entry) -> SharedString {
    match version.username() {
        "" => title_of(version),
        username => format!("{} · {username}", title_of(version)).into(),
    }
}
