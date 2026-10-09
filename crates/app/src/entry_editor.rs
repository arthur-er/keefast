use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, PathPromptOptions, SharedString,
    Window, div, prelude::*, px,
};
use keefast_core::{
    Attachment, CustomField, EntryColor, EntryFields, EntryIcon, STANDARD_ICON_COUNT, Timestamp,
};
use keefast_workspace::{ImportIconError, VaultId, Workspace};
use secrecy::{ExposeSecret, SecretString};

use crate::attachments;
use crate::icons;
use crate::password_generator::{PasswordGenerator, PasswordGeneratorEvent};
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::vault_browser::describe;
use crate::widgets::button;

pub enum EntryEditorEvent {
    Save(Box<EntryFields>),
    Cancel,
}

/// The form for creating or editing an Entry, shown in the detail pane.
pub struct EntryEditor {
    workspace: Entity<Workspace>,
    /// The Vault the Entry is in, whose custom icons it can use.
    vault: VaultId,
    title: Entity<TextField>,
    username: Entity<TextField>,
    password: Entity<TextField>,
    url: Entity<TextField>,
    notes: Entity<TextField>,
    password_revealed: bool,
    /// The password generator, while it is open.
    generator: Option<Entity<PasswordGenerator>>,
    custom_fields: Vec<CustomFieldRow>,
    tags: Entity<TextField>,
    color: Option<EntryColor>,
    icon: EntryIcon,
    /// Whether the icons to choose from are shown.
    choosing_icon: bool,
    expires: bool,
    expiry_date: Entity<TextField>,
    /// The expiry the Entry had, kept to the second while its date isn't changed.
    original_expiry: Option<Timestamp>,
    attachments: Vec<Attachment>,
    /// Why the form can't be saved, or an icon imported.
    error: Option<SharedString>,
    /// Why files picked to attach couldn't be read.
    attachment_errors: Option<SharedString>,
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
    pub fn new(
        fields: &EntryFields,
        vault: VaultId,
        workspace: Entity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
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
        let tags = field(
            strings::TAGS_PLACEHOLDER,
            &fields.tags.join(", "),
            plain,
            cx,
        );
        let expiry_date = field(
            strings::EXPIRY_PLACEHOLDER,
            &fields.expires.map(Timestamp::to_date).unwrap_or_default(),
            plain,
            cx,
        );
        Self {
            workspace,
            vault,
            title,
            username,
            password,
            url,
            notes,
            password_revealed: false,
            generator: None,
            custom_fields,
            tags,
            color: fields.color,
            icon: fields.icon,
            choosing_icon: false,
            expires: fields.expires.is_some(),
            expiry_date,
            original_expiry: fields.expires,
            attachments: fields.attachments.clone(),
            error: None,
            attachment_errors: None,
        }
    }

    fn set_color(&mut self, color: Option<EntryColor>, cx: &mut Context<Self>) {
        self.color = color;
        cx.notify();
    }

    fn set_icon(&mut self, icon: EntryIcon, cx: &mut Context<Self>) {
        self.icon = icon;
        self.choosing_icon = false;
        self.error = None;
        cx.notify();
    }

    /// Turns expiry on or off. Turned on without a date, it offers today's to change.
    fn toggle_expires(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.expires = !self.expires;
        if self.expires {
            let today = self.workspace.read(cx).now().to_date();
            self.expiry_date.update(cx, |field, cx| {
                if field.text().trim().is_empty() {
                    field.set_text(&today, cx);
                }
            });
            window.focus(&self.expiry_date.focus_handle(cx));
        }
        cx.notify();
    }

    /// Asks for a PNG image and adds it to the Vault's icons, as the Entry's icon.
    fn import_icon(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(strings::IMPORT_ICON_PROMPT.into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = paths.await;
            this.update(cx, |this, cx| match picked {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => {
                    let path = paths.swap_remove(0);
                    let vault = this.vault;
                    let imported = this.workspace.update(cx, |workspace, cx| {
                        cx.notify();
                        workspace.import_custom_icon(vault, &path)
                    });
                    match imported {
                        Ok(icon) => this.set_icon(EntryIcon::Custom(icon), cx),
                        Err(error) => {
                            this.error = Some(describe_import(&error).into());
                            cx.notify();
                        }
                    }
                }
                Ok(Ok(_)) | Err(_) => {}
                Ok(Err(_)) => {
                    this.error = Some(strings::ERROR_PICKER.into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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

    /// Asks for files and attaches them, a name another attachment has getting a number.
    fn add_attachments(&mut self, cx: &mut Context<Self>) {
        let picked = attachments::pick_and_read(cx);
        cx.spawn(async move |this, cx| {
            let Some(read) = picked.await else {
                return;
            };
            this.update(cx, |this, cx| {
                let mut errors = Vec::new();
                for result in read {
                    match result {
                        Ok(attachment) => {
                            attachment.add_to(&mut this.attachments);
                        }
                        Err(error) => errors.push(error),
                    }
                }
                this.attachment_errors = (!errors.is_empty()).then(|| errors.join("\n").into());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
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

    /// The expiry the form sets, or `Err` when its date can't be read.
    fn expiry(&self, cx: &App) -> Result<Option<Timestamp>, ()> {
        if !self.expires {
            return Ok(None);
        }
        let date = self.expiry_date.read(cx).text();
        Timestamp::expiry_on(date, self.original_expiry)
            .map(Some)
            .ok_or(())
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Ok(expires) = self.expiry(cx) else {
            self.error = Some(strings::ERROR_EXPIRY_DATE.into());
            cx.notify();
            return;
        };
        self.error = None;
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
            tags: keefast_core::parse_tags(&text(&self.tags)),
            color: self.color,
            icon: self.icon,
            expires,
            attachments: self.attachments.clone(),
        };
        cx.emit(EntryEditorEvent::Save(Box::new(fields)));
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
        let theme = Theme::for_window(window, cx);
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
            .child(labelled(strings::FIELD_TAGS, &self.tags))
            .child(self.render_color(theme, cx))
            .child(self.render_icon(theme, cx))
            .child(self.render_expiry(theme, cx))
            .child(self.render_custom_fields(theme, cx))
            .child(self.render_attachments(theme, cx))
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.error).child(error))
            })
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
    fn render_color(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let swatch = |id: usize, selected: bool| {
            div()
                .id(("color", id))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_2()
                .border_color(if selected {
                    theme.accent
                } else {
                    gpui::transparent_black().into()
                })
                .cursor_pointer()
        };
        let colors = EntryColor::ALL.into_iter().enumerate().map(|(ix, color)| {
            swatch(ix + 1, self.color == Some(color))
                .child(icons::color_dot(color, px(14.)))
                .on_click(cx.listener(move |this, _, _, cx| this.set_color(Some(color), cx)))
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(strings::FIELD_COLOR),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        swatch(0, self.color.is_none())
                            .w_auto()
                            .px_2()
                            .text_sm()
                            .child(strings::NO_COLOR)
                            .on_click(cx.listener(|this, _, _, cx| this.set_color(None, cx))),
                    )
                    .children(colors),
            )
    }

    fn render_icon(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let vault = workspace.vault(self.vault).map(|open| open.vault());
        let cell = |id: (&'static str, usize), selected: bool| {
            div()
                .id(id)
                .size(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .border_1()
                .border_color(if selected {
                    theme.accent
                } else {
                    gpui::transparent_black().into()
                })
                .when(selected, |this| this.bg(theme.sidebar_selection))
                .hover(|this| this.bg(theme.sidebar_selection))
                .cursor_pointer()
        };
        let current = vault.map(|vault| icons::entry_icon(self.icon, vault, px(20.), theme.text));
        let picker = self.choosing_icon.then(|| {
            let standard = (0..STANDARD_ICON_COUNT).map(|number| {
                let icon = EntryIcon::Standard(number);
                cell(("standard-icon", number as usize), self.icon == icon)
                    .child(icons::standard_icon(number, px(18.), theme.text))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_icon(icon, cx)))
            });
            let custom: Vec<_> = vault
                .map(|vault| vault.custom_icons())
                .unwrap_or_default()
                .iter()
                .enumerate()
                .map(|(ix, custom)| {
                    let icon = EntryIcon::Custom(custom.id());
                    cell(("custom-icon", ix), self.icon == icon)
                        .children(
                            vault.map(|vault| icons::entry_icon(icon, vault, px(18.), theme.text)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.set_icon(icon, cx)))
                })
                .collect();
            let heading =
                |label: &'static str| div().text_xs().text_color(theme.muted).child(label);
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .child(heading(strings::STANDARD_ICONS))
                .child(div().flex().flex_wrap().children(standard))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(heading(strings::CUSTOM_ICONS))
                        .child(
                            div()
                                .id("import-icon")
                                .text_sm()
                                .text_color(theme.accent)
                                .cursor_pointer()
                                .child(strings::IMPORT_ICON)
                                .on_click(cx.listener(|this, _, _, cx| this.import_icon(cx))),
                        ),
                )
                .child(div().flex().flex_wrap().children(custom))
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(strings::FIELD_ICON),
            )
            .child(
                div().flex().items_center().gap_3().children(current).child(
                    div()
                        .id("choose-icon")
                        .text_sm()
                        .text_color(theme.accent)
                        .cursor_pointer()
                        .child(strings::CHOOSE_ICON)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.choosing_icon = !this.choosing_icon;
                            cx.notify();
                        })),
                ),
            )
            .children(picker)
    }

    fn render_expiry(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id("expires")
                    .flex()
                    .gap_1()
                    .text_sm()
                    .text_color(theme.muted)
                    .cursor_pointer()
                    .child(if self.expires {
                        strings::CHECKBOX_ON
                    } else {
                        strings::CHECKBOX_OFF
                    })
                    .child(strings::FIELD_EXPIRES)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_expires(window, cx))),
            )
            .when(self.expires, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(140.)).child(self.expiry_date.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted)
                                .child(strings::EXPIRY_NOTE),
                        ),
                )
            })
    }

    fn render_attachments(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                let remove = div()
                    .id(("remove-attachment", index))
                    .cursor_pointer()
                    .text_color(theme.accent)
                    .child(strings::REMOVE_ATTACHMENT)
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_attachment(index, cx)));
                attachments::row(attachment, remove, theme)
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
                    .child(strings::ATTACHMENTS)
                    .child(
                        div()
                            .id("add-attachment")
                            .cursor_pointer()
                            .text_color(theme.accent)
                            .child(strings::ADD_ATTACHMENT)
                            .on_click(cx.listener(|this, _, _, cx| this.add_attachments(cx))),
                    ),
            )
            .when_some(self.attachment_errors.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.error).child(error))
            })
            .children(rows)
    }

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

fn describe_import(error: &ImportIconError) -> &'static str {
    match error {
        ImportIconError::Unreadable(_) => strings::ERROR_ICON_UNREADABLE,
        ImportIconError::Edit(error) => describe(error),
    }
}
