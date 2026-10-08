use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, Window, div, prelude::*};
use keefast_core::{Entry, EntryFields};
use secrecy::{ExposeSecret, SecretString};

use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::widgets::button;

pub enum EntryEditorEvent {
    Save(EntryFields),
    Cancel,
}

/// The form for creating or editing an Entry, shown in the detail pane.
pub struct EntryEditor {
    title: Entity<TextField>,
    username: Entity<TextField>,
    password: Entity<TextField>,
    url: Entity<TextField>,
    notes: Entity<TextField>,
    password_revealed: bool,
}

impl EventEmitter<EntryEditorEvent> for EntryEditor {}

impl Focusable for EntryEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.title.focus_handle(cx)
    }
}

impl EntryEditor {
    /// A form filled from `entry`, or empty for a new Entry.
    pub fn new(entry: Option<&Entry>, cx: &mut Context<Self>) -> Self {
        let field = |placeholder: &'static str,
                     value: Option<&str>,
                     style: fn(TextField) -> TextField,
                     cx: &mut Context<Self>| {
            let field = cx.new(|cx| {
                let mut field = style(TextField::new(placeholder, cx));
                if let Some(value) = value {
                    field.set_text(value, cx);
                }
                field
            });
            cx.subscribe(&field, Self::on_field_event).detach();
            field
        };
        let plain = std::convert::identity;
        let title = field(strings::EDITOR_TITLE, entry.map(Entry::title), plain, cx);
        let username = field(
            strings::DETAIL_USERNAME,
            entry.map(Entry::username),
            plain,
            cx,
        );
        let password = field(
            strings::DETAIL_PASSWORD,
            entry.map(|entry| entry.password().expose_secret()),
            TextField::masked,
            cx,
        );
        let url = field(strings::DETAIL_URL, entry.map(Entry::url), plain, cx);
        let notes = field(
            strings::DETAIL_NOTES,
            entry.map(Entry::notes),
            TextField::multiline,
            cx,
        );
        Self {
            title,
            username,
            password,
            url,
            notes,
            password_revealed: false,
        }
    }

    fn on_field_event(
        &mut self,
        _: Entity<TextField>,
        event: &TextFieldEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit => self.save(cx),
            TextFieldEvent::Cancel => cx.emit(EntryEditorEvent::Cancel),
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let text = |field: &Entity<TextField>| field.read(cx).text().to_owned();
        let fields = EntryFields {
            title: text(&self.title),
            username: text(&self.username),
            password: SecretString::from(text(&self.password)),
            url: text(&self.url),
            notes: text(&self.notes),
        };
        cx.emit(EntryEditorEvent::Save(fields));
    }

    fn toggle_reveal(&mut self, cx: &mut Context<Self>) {
        self.password_revealed = !self.password_revealed;
        let masked = !self.password_revealed;
        self.password
            .update(cx, |field, cx| field.set_masked(masked, cx));
        cx.notify();
    }
}

impl Render for EntryEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let labelled = |label: &'static str, field: &Entity<TextField>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().text_color(theme.muted).child(label))
                .child(field.clone())
        };

        div()
            .id("entry-editor")
            .flex()
            .flex_col()
            .gap_3()
            .child(labelled(strings::EDITOR_TITLE, &self.title))
            .child(labelled(strings::DETAIL_USERNAME, &self.username))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_sm()
                            .text_color(theme.muted)
                            .child(strings::DETAIL_PASSWORD)
                            .child(
                                div()
                                    .id("reveal-password")
                                    .cursor_pointer()
                                    .text_color(theme.accent)
                                    .child(if self.password_revealed {
                                        strings::HIDE_PASSWORD
                                    } else {
                                        strings::REVEAL_PASSWORD
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_reveal(cx))),
                            ),
                    )
                    .child(self.password.clone()),
            )
            .child(labelled(strings::DETAIL_URL, &self.url))
            .child(labelled(strings::DETAIL_NOTES, &self.notes))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("editor-cancel", strings::CANCEL, theme)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(EntryEditorEvent::Cancel))),
                    )
                    .child(
                        button("editor-save", strings::SAVE_ENTRY, theme)
                            .bg(theme.accent)
                            .text_color(theme.accent_text)
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
    }
}
