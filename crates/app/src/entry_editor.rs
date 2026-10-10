use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    PathPromptOptions, SharedString, Window, anchored, deferred, div, prelude::*, px,
};
use keefast_core::{
    Attachment, CustomField, EntryColor, EntryFields, EntryIcon, STANDARD_ICON_COUNT, Timestamp,
};
use keefast_workspace::{ImportIconError, VaultId, Workspace};
use secrecy::{ExposeSecret, SecretString};

use crate::actions::{EDITOR_CONTEXT, SaveEntry};
use crate::attachments;
use crate::icons;
use crate::icons::IconName;
use crate::password_generator::{PasswordGenerator, PasswordGeneratorEvent};
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::ui::{self, Button, Typography};
use crate::vault_browser::color_name;
use crate::vault_browser::describe;

/// The standard icon of a key, which new Entries get: drawn as a monogram instead.
const KEY_ICON: u32 = 0;

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
    tags: Vec<String>,
    /// Where a tag is typed to add it.
    tag_input: Entity<TextField>,
    color: Option<EntryColor>,
    /// Whether the colours to choose from are shown.
    choosing_color: bool,
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
        let title = field(strings::FIELD_TITLE, &fields.title, TextField::title, cx);
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
        let tag_input = cx.new(|cx| TextField::new(strings::TAG_ADD, cx).compact());
        cx.subscribe(&tag_input, Self::on_tag_input_event).detach();
        let expiry_date = field(
            strings::EXPIRY_PLACEHOLDER,
            &fields.expires.map(Timestamp::to_date).unwrap_or_default(),
            TextField::compact,
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
            tags: fields.tags.clone(),
            tag_input,
            color: fields.color,
            choosing_color: false,
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
        self.choosing_color = false;
        cx.notify();
    }

    /// Enter adds the tags typed, or saves the Entry when nothing is typed.
    fn on_tag_input_event(
        &mut self,
        input: Entity<TextField>,
        event: &TextFieldEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit => {
                let typed = keefast_core::parse_tags(input.read(cx).text());
                if typed.is_empty() {
                    self.save(cx);
                    return;
                }
                for tag in typed {
                    if !self.tags.contains(&tag) {
                        self.tags.push(tag);
                    }
                }
                input.update(cx, |input, cx| input.set_text("", cx));
                cx.notify();
            }
            TextFieldEvent::Cancel => cx.emit(EntryEditorEvent::Cancel),
        }
    }

    fn remove_tag(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tags.len() {
            self.tags.remove(index);
        }
        cx.notify();
    }

    /// Escape closes what is open over the form, else the form, without saving.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        if self.choosing_color || self.choosing_icon {
            self.choosing_color = false;
            self.choosing_icon = false;
            cx.notify();
        } else {
            self.cancel(cx);
        }
        cx.stop_propagation();
    }

    /// Closes the form without saving.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(EntryEditorEvent::Cancel);
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
            window.focus(&self.expiry_date.focus_handle(cx), cx);
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
        window.focus(&row.name.focus_handle(cx), cx);
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
            window.focus(&generator.focus_handle(cx), cx);
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
        window.focus(&self.password.focus_handle(cx), cx);
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

    pub fn save(&mut self, cx: &mut Context<Self>) {
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
            tags: {
                // A tag typed but not yet added with Enter is kept too.
                let mut tags = self.tags.clone();
                for tag in keefast_core::parse_tags(&text(&self.tag_input)) {
                    if !tags.contains(&tag) {
                        tags.push(tag);
                    }
                }
                tags
            },
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
        let error = self.error.clone();
        div()
            .id("entry-editor")
            .key_context(EDITOR_CONTEXT)
            .on_action(cx.listener(|this, _: &SaveEntry, _, cx| this.save(cx)))
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(self.render_header(theme, cx))
            .when(self.choosing_icon, |this| {
                this.child(self.render_icon_picker(theme, cx))
            })
            .child(self.render_login(theme, cx))
            .child(self.render_custom_fields(theme, cx))
            .child(self.render_attachments(theme, cx))
            .when_some(error, |this, error| {
                this.child(ui::error_text(error, theme))
            })
    }
}

impl EntryEditor {
    /// The Entry's picture, title and marks: its colour, expiry and tags.
    fn render_header(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let vault = workspace.vault(self.vault).map(|open| open.vault());
        let picture = match (self.icon, vault) {
            (EntryIcon::Standard(KEY_ICON), _) | (_, None) => {
                ui::monogram(self.title.read(cx).text(), 48.).into_any_element()
            }
            (icon, Some(vault)) => ui::picture_tile(
                icons::entry_icon(icon, vault, px(28.), theme.fg_1),
                48.,
                theme,
            )
            .into_any_element(),
        };
        div()
            .flex()
            .items_start()
            .gap(px(16.))
            .child(
                div()
                    .id("choose-icon")
                    .relative()
                    .flex_none()
                    .cursor_pointer()
                    .child(picture)
                    .child(
                        div()
                            .absolute()
                            .right(px(-4.))
                            .bottom(px(-4.))
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme.bg_2)
                            .border_1()
                            .border_color(theme.border_strong)
                            .child(icons::ui_icon(IconName::Pencil, px(11.), theme.fg_1)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.choosing_icon = !this.choosing_icon;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().ml(px(-7.)).child(self.title.clone()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .child(self.render_color_chip(theme, cx))
                            .child(self.render_expiry_chip(theme, cx))
                            .child(div().w_px().h(px(16.)).mx(px(2.)).bg(theme.border))
                            .children(self.tags.iter().enumerate().map(|(ix, tag)| {
                                ui::tag(tag.clone(), None, theme).child(
                                    div()
                                        .id(("remove-tag", ix))
                                        .cursor_pointer()
                                        .child(icons::ui_icon(IconName::X, px(11.), theme.fg_2))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.remove_tag(ix, cx)
                                        })),
                                )
                            }))
                            .child(div().w(px(110.)).child(self.tag_input.clone())),
                    ),
            )
    }

    /// A small bordered button in the row of marks under the title.
    fn chip(id: &'static str, theme: Theme) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.bg_1)
            .text_size(px(12.))
            .cursor_pointer()
            .hover(|style| style.bg(theme.bg_2))
    }

    fn render_color_chip(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = self.choosing_color.then(|| {
            let none = ui::MenuItem::new(("color", 0usize), strings::NO_COLOR)
                .checked(self.color.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.set_color(None, cx)));
            let colors = EntryColor::ALL.into_iter().enumerate().map(|(ix, color)| {
                div()
                    .id(("color-choice", ix))
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .when(self.color == Some(color), |this| {
                        this.border_2().border_color(theme.fg_1)
                    })
                    .hover(|style| style.bg(theme.bg_hover))
                    .child(ui::dot(theme.label(color), 14.))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_color(Some(color), cx)))
            });
            deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.))
                    .child(ui::appear(
                        "color-menu",
                        ui::menu(theme)
                            .id("color-menu-box")
                            .occlude()
                            .mt(px(30.))
                            .min_w(px(220.))
                            .child(none)
                            .child(div().flex().gap(px(2.)).p(px(4.)).children(colors)),
                    )),
            )
            .with_priority(1)
        });
        div()
            .child(
                Self::chip("color-chip", theme)
                    .map(|this| match self.color {
                        Some(color) => this
                            .child(ui::dot(theme.label(color), 8.))
                            .child(color_name(color)),
                        None => this
                            .text_color(theme.fg_2)
                            .child(ui::dot(theme.border_strong, 8.))
                            .child(strings::FIELD_COLOR),
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.choosing_color = !this.choosing_color;
                        cx.notify();
                    })),
            )
            .children(menu)
    }

    fn render_expiry_chip(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.expires {
            return Self::chip("expires", theme)
                .text_color(theme.fg_2)
                .child(icons::ui_icon(IconName::Calendar, px(13.), theme.fg_2))
                .child(strings::FIELD_EXPIRES)
                .on_click(cx.listener(|this, _, window, cx| this.toggle_expires(window, cx)))
                .into_any_element();
        }
        let expired = self
            .expiry(cx)
            .ok()
            .flatten()
            .is_some_and(|at| at <= self.workspace.read(cx).now());
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .child(
                ui::IconButton::new("expiry-note", IconName::Calendar)
                    .small()
                    .tooltip(strings::EXPIRY_NOTE),
            )
            .child(div().w(px(104.)).child(self.expiry_date.clone()))
            .when(expired, |this| {
                this.child(ui::Badge::new(strings::EXPIRED_BADGE).tone(ui::Tone::Danger))
            })
            .child(
                ui::IconButton::new("expires", IconName::X)
                    .small()
                    .tooltip(strings::EXPIRY_CLEAR)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_expires(window, cx))),
            )
            .into_any_element()
    }

    /// The username, password, URL and notes, each with its name in a column.
    fn render_login(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, field: AnyElement| {
            div()
                .flex()
                .items_start()
                .gap(px(16.))
                .child(
                    div()
                        .w(px(96.))
                        .flex_none()
                        .pt(px(7.))
                        .text_color(theme.fg_1)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(field))
        };
        let generator = self.generator.clone().map(|generator| {
            // The row's end aligns the popover's right edge with the field's buttons.
            div().flex().justify_end().child(
                deferred(
                    anchored()
                        .snap_to_window_with_margin(px(8.))
                        .child(ui::appear(
                            "generator-appear",
                            div().mt(px(6.)).child(generator),
                        )),
                )
                .with_priority(1),
            )
        });
        let password =
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().flex_1().min_w_0().child(self.password.clone()))
                        .child(
                            ui::IconButton::new(
                                "reveal-password",
                                if self.password_revealed {
                                    IconName::EyeOff
                                } else {
                                    IconName::Eye
                                },
                            )
                            .tooltip(if self.password_revealed {
                                strings::HIDE_PASSWORD
                            } else {
                                strings::REVEAL_PASSWORD
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_reveal(cx))),
                        )
                        .child(
                            Button::new("generate-password", strings::GENERATE_PASSWORD)
                                .icon(IconName::WandSparkles)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_generator(window, cx)
                                })),
                        ),
                )
                .children(generator);
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().text_heading().child(strings::EDITOR_LOGIN))
            .child(row(
                strings::DETAIL_USERNAME,
                self.username.clone().into_any_element(),
            ))
            .child(row(strings::DETAIL_PASSWORD, password.into_any_element()))
            .child(row(
                strings::DETAIL_URL,
                self.url.clone().into_any_element(),
            ))
            .child(row(
                strings::DETAIL_NOTES,
                self.notes.clone().into_any_element(),
            ))
    }

    fn render_icon_picker(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let vault = workspace.vault(self.vault).map(|open| open.vault());
        let cell = |id: (&'static str, usize), selected: bool| {
            div()
                .id(id)
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .border_1()
                .border_color(if selected {
                    theme.accent
                } else {
                    gpui::transparent_black().into()
                })
                .when(selected, |this| this.bg(theme.accent_bg))
                .hover(|this| this.bg(theme.bg_hover))
                .cursor_pointer()
        };
        let standard = (0..STANDARD_ICON_COUNT).map(|number| {
            let icon = EntryIcon::Standard(number);
            cell(("standard-icon", number as usize), self.icon == icon)
                .child(icons::standard_icon(number, px(17.), theme.fg_1))
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
                        vault.map(|vault| icons::entry_icon(icon, vault, px(18.), theme.fg_1)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_icon(icon, cx)))
            })
            .collect();
        ui::card(theme)
            .gap(px(8.))
            .p(px(12.))
            .child(ui::section_heading(strings::STANDARD_ICONS, theme).px_0())
            .child(div().flex().flex_wrap().gap(px(2.)).children(standard))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(ui::section_heading(strings::CUSTOM_ICONS, theme).px_0())
                    .child(
                        Button::new("import-icon", strings::IMPORT_ICON)
                            .small()
                            .ghost()
                            .icon(IconName::ImagePlus)
                            .on_click(cx.listener(|this, _, _, cx| this.import_icon(cx))),
                    ),
            )
            .child(div().flex().flex_wrap().gap(px(2.)).children(custom))
    }

    /// A section's heading, with its count and the button that adds to it.
    fn section_head(
        title: &'static str,
        count: usize,
        add: Button,
        theme: Theme,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().text_heading().child(title))
            .when(count > 0, |this| {
                this.child(
                    div()
                        .text_micro()
                        .text_color(theme.fg_2)
                        .child(SharedString::from(count.to_string())),
                )
            })
            .child(div().flex_1())
            .child(add)
    }

    fn render_attachments(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let rows =
            self.attachments
                .iter()
                .enumerate()
                .map(|(index, attachment)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .min_h(px(40.))
                        .px(px(12.))
                        .when(index > 0, |this| {
                            this.border_t_1().border_color(theme.border)
                        })
                        .child(icons::ui_icon(IconName::FileText, px(16.), theme.fg_1))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_strong()
                                .child(attachment.name.clone()),
                        )
                        .child(
                            div()
                                .text_secondary()
                                .text_color(theme.fg_2)
                                .child(attachments::size_label(attachment.size())),
                        )
                        .child(
                            Button::new(("remove-attachment", index), strings::REMOVE_ATTACHMENT)
                                .small()
                                .danger_ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_attachment(index, cx)
                                })),
                        )
                })
                .collect::<Vec<_>>();
        let any = !rows.is_empty();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(Self::section_head(
                strings::ATTACHMENTS,
                self.attachments.len(),
                Button::new("add-attachment", strings::ADD_ATTACHMENT)
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.add_attachments(cx))),
                theme,
            ))
            .when_some(self.attachment_errors.clone(), |this, error| {
                this.child(ui::error_text(error, theme))
            })
            .when(any, |this| this.child(ui::card(theme).children(rows)))
    }

    fn render_custom_fields(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.custom_fields.iter().enumerate().map(|(index, row)| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(160.)).flex_none().child(row.name.clone()))
                .child(div().flex_1().min_w_0().child(row.value.clone()))
                .when(row.protected, |this| {
                    this.child(
                        ui::IconButton::new(
                            ("reveal-field", index),
                            if row.revealed {
                                IconName::EyeOff
                            } else {
                                IconName::Eye
                            },
                        )
                        .tooltip(if row.revealed {
                            strings::HIDE_PASSWORD
                        } else {
                            strings::REVEAL_PASSWORD
                        })
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_custom_reveal(index, cx)),
                        ),
                    )
                })
                .child(
                    div()
                        .id(("protect-field", index))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(8.))
                        .px(px(4.))
                        .cursor_pointer()
                        .text_color(theme.fg_1)
                        .child(ui::switch_mark(row.protected, theme))
                        .child(strings::FIELD_PROTECTED)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_protected(index, cx)),
                        ),
                )
                .child(
                    ui::IconButton::new(("remove-field", index), IconName::X)
                        .tooltip(strings::REMOVE_FIELD)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.remove_custom_field(index, cx)),
                        ),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(Self::section_head(
                strings::CUSTOM_FIELDS,
                self.custom_fields.len(),
                Button::new("add-field", strings::ADD_FIELD)
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| this.add_custom_field(window, cx))),
                theme,
            ))
            .children(rows)
    }
}

fn describe_import(error: &ImportIconError) -> &'static str {
    match error {
        ImportIconError::Unreadable(_) => strings::ERROR_ICON_UNREADABLE,
        ImportIconError::Edit(error) => describe(error),
    }
}
