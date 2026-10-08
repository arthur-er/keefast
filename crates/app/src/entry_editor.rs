use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, Window, div, prelude::*};
use keefast_core::{CustomField, EntryFields};
use keefast_workspace::Workspace;
use secrecy::{ExposeSecret, SecretString};

use crate::password_generator::{PasswordGenerator, PasswordGeneratorEvent};
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
    workspace: Entity<Workspace>,
    title: Entity<TextField>,
    username: Entity<TextField>,
    password: Entity<TextField>,
    url: Entity<TextField>,
    notes: Entity<TextField>,
    password_revealed: bool,
    /// The password generator, while it is open.
    generator: Option<Entity<PasswordGenerator>>,
    custom_fields: Vec<CustomFieldRow>,
}

/// A custom field being edited.
struct CustomFieldRow {
    name: Entity<TextField>,
    value: Entity<TextField>,
    protected: bool,
    revealed: bool,
}

impl EventEmitter<EntryEditorEvent> for EntryEditor {}

impl Focusable for EntryEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.title.focus_handle(cx)
    }
}

impl EntryEditor {
    /// A form filled with `fields`: an Entry's, or empty ones for a new Entry.
    pub fn new(fields: &EntryFields, workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let field = |placeholder: &'static str,
                     value: &str,
                     style: fn(TextField) -> TextField,
                     cx: &mut Context<Self>| {
            let field = cx.new(|cx| {
                let mut field = style(TextField::new(placeholder, cx));
                field.set_text(value, cx);
                field
            });
            cx.subscribe(&field, Self::on_field_event).detach();
            field
        };
        let plain = std::convert::identity;
        let title = field(strings::FIELD_TITLE, &fields.title, plain, cx);
        let username = field(strings::DETAIL_USERNAME, &fields.username, plain, cx);
        let password = field(
            strings::DETAIL_PASSWORD,
            fields.password.expose_secret(),
            TextField::masked,
            cx,
        );
        let url = field(strings::DETAIL_URL, &fields.url, plain, cx);
        let notes = field(
            strings::DETAIL_NOTES,
            &fields.notes,
            TextField::multiline,
            cx,
        );
        let custom_fields = fields
            .custom_fields
            .iter()
            .map(|field| Self::custom_field_row(field, cx))
            .collect();
        Self {
            workspace,
            title,
            username,
            password,
            url,
            notes,
            password_revealed: false,
            generator: None,
            custom_fields,
        }
    }

    /// The fields to edit a custom field with; a protected value starts hidden.
    fn custom_field_row(field: &CustomField, cx: &mut Context<Self>) -> CustomFieldRow {
        let mut text_field = |placeholder: &'static str, text: &str, masked: bool| {
            let field = cx.new(|cx| {
                let mut field = TextField::new(placeholder, cx);
                field.set_text(text, cx);
                field.set_masked(masked, cx);
                field
            });
            cx.subscribe(&field, Self::on_field_event).detach();
            field
        };
        CustomFieldRow {
            name: text_field(strings::FIELD_NAME_PLACEHOLDER, &field.name, false),
            value: text_field(
                strings::FIELD_VALUE_PLACEHOLDER,
                field.value.expose_secret(),
                field.protected,
            ),
            protected: field.protected,
            revealed: false,
        }
    }

    fn add_custom_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let empty = CustomField {
            name: String::new(),
            value: SecretString::from(String::new()),
            protected: false,
        };
        let row = Self::custom_field_row(&empty, cx);
        window.focus(&row.name.focus_handle(cx));
        self.custom_fields.push(row);
        cx.notify();
    }

    fn remove_custom_field(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.custom_fields.len() {
            self.custom_fields.remove(index);
        }
        cx.notify();
    }

    /// Protecting a field hides its value, as a password is hidden.
    fn toggle_protected(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.custom_fields.get_mut(index) else {
            return;
        };
        row.protected = !row.protected;
        row.revealed = false;
        let masked = row.protected;
        row.value
            .update(cx, |field, cx| field.set_masked(masked, cx));
        cx.notify();
    }

    fn toggle_custom_reveal(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.custom_fields.get_mut(index) else {
            return;
        };
        row.revealed = !row.revealed;
        let masked = row.protected && !row.revealed;
        row.value
            .update(cx, |field, cx| field.set_masked(masked, cx));
        cx.notify();
    }

    fn toggle_generator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.generator.take().is_none() {
            let workspace = self.workspace.clone();
            let generator = cx.new(|cx| PasswordGenerator::new(workspace, cx));
            cx.subscribe_in(&generator, window, Self::on_generator_event)
                .detach();
            self.generator = Some(generator);
        }
        cx.notify();
    }

    fn on_generator_event(
        &mut self,
        _: &Entity<PasswordGenerator>,
        event: &PasswordGeneratorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let PasswordGeneratorEvent::Use(password) = event {
            self.password
                .update(cx, |field, cx| field.set_text(password.expose_secret(), cx));
        }
        self.generator = None;
        window.focus(&self.password.focus_handle(cx));
        cx.notify();
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
            custom_fields: self
                .custom_fields
                .iter()
                .map(|row| CustomField {
                    name: row.name.read(cx).text().trim().to_owned(),
                    value: SecretString::from(text(&row.value)),
                    protected: row.protected,
                })
                .collect(),
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
            .child(labelled(strings::FIELD_TITLE, &self.title))
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
                                    .flex()
                                    .gap_3()
                                    .text_color(theme.accent)
                                    .child(
                                        div()
                                            .id("generate-password")
                                            .cursor_pointer()
                                            .child(strings::GENERATE_PASSWORD)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.toggle_generator(window, cx)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("reveal-password")
                                            .cursor_pointer()
                                            .child(if self.password_revealed {
                                                strings::HIDE_PASSWORD
                                            } else {
                                                strings::REVEAL_PASSWORD
                                            })
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.toggle_reveal(cx)
                                                }),
                                            ),
                                    ),
                            ),
                    )
                    .child(self.password.clone())
                    .children(self.generator.clone()),
            )
            .child(labelled(strings::DETAIL_URL, &self.url))
            .child(labelled(strings::DETAIL_NOTES, &self.notes))
            .child(self.render_custom_fields(theme, cx))
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
                        button("editor-save", strings::EDITOR_DONE, theme)
                            .bg(theme.accent)
                            .text_color(theme.accent_text)
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
    }
}

impl EntryEditor {
    fn render_custom_fields(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.custom_fields.iter().enumerate().map(|(index, row)| {
            let link = |id: &'static str, label: &'static str| {
                div()
                    .id((id, index))
                    .cursor_pointer()
                    .text_color(theme.accent)
                    .child(label)
            };
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().w_1_3().child(row.name.clone()))
                        .child(div().flex_1().child(row.value.clone())),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_3()
                        .text_sm()
                        .child(
                            div()
                                .id(("protect-field", index))
                                .flex()
                                .gap_1()
                                .cursor_pointer()
                                .text_color(theme.muted)
                                .child(if row.protected {
                                    strings::CHECKBOX_ON
                                } else {
                                    strings::CHECKBOX_OFF
                                })
                                .child(strings::FIELD_PROTECTED)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_protected(index, cx)
                                })),
                        )
                        .when(row.protected, |this| {
                            this.child(
                                link(
                                    "reveal-field",
                                    if row.revealed {
                                        strings::HIDE_PASSWORD
                                    } else {
                                        strings::REVEAL_PASSWORD
                                    },
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.toggle_custom_reveal(index, cx),
                                )),
                            )
                        })
                        .child(link("remove-field", strings::REMOVE_FIELD).on_click(
                            cx.listener(move |this, _, _, cx| this.remove_custom_field(index, cx)),
                        )),
                )
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(strings::CUSTOM_FIELDS)
                    .child(
                        div()
                            .id("add-field")
                            .cursor_pointer()
                            .text_color(theme.accent)
                            .child(strings::ADD_FIELD)
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.add_custom_field(window, cx)
                                }),
                            ),
                    ),
            )
            .children(rows)
    }
}
