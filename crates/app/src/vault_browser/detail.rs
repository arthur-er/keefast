//! The Entry detail: the selected Entry's title and secrets over tabs of its other fields, and
//! the forms that take its place (editing, TOTP setup, history, moving, a Group's commands).

use gpui::{
    AnyElement, ClickEvent, Context, Div, IntoElement, ParentElement, SharedString, Stateful,
    Styled, Window, div, prelude::*, px,
};
use keefast_core::{AccessDecision, AttachmentPreview, Entry, EntryId, TotpCode};
use keefast_workspace::{EntryScope, VaultId};
use secrecy::ExposeSecret;

use super::entry_list::{color_name, entry_avatar, site_host};
use super::{
    Detail, DetailTab, EditTarget, FOLDER_ICON, InlineField, ListMenu, MenuItem, Subject,
    VaultBrowser, flatten_groups, title_of,
};
use crate::actions::{self, CopyCustomField};
use crate::attachments;
use crate::icons::{self, IconName};
use crate::strings;
use crate::theme::{MONO_FONT, Theme};
use crate::totp_setup;
use crate::ui::{self, Button, Kbd, Typography};

/// How wide the detail's content grows at most, so lines stay readable on a wide window.
const CONTENT_MAX_WIDTH: f32 = 720.;

impl VaultBrowser {
    pub(super) fn render_detail(
        &self,
        edit_keys: Option<Kbd>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane = div()
            .id("entry-detail")
            .flex_1()
            // Long values wrap rather than widen the pane past the window.
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .bg(theme.bg_0);
        let form = |pane: Stateful<Div>, form: AnyElement| {
            pane.child(
                div()
                    .max_w(px(CONTENT_MAX_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .px(px(28.))
                    .py(px(24.))
                    .when_some(self.error.clone(), |this, error| {
                        this.child(ui::error_text(error, theme))
                    })
                    .child(form),
            )
            .into_any_element()
        };
        match &self.detail {
            Detail::Editing { editor, .. } => form(pane, editor.clone().into_any_element()),
            Detail::Totp { setup, .. } => form(pane, setup.clone().into_any_element()),
            Detail::History { view, .. } => form(pane, view.clone().into_any_element()),
            Detail::VaultKey { form: key, .. } => form(pane, key.clone().into_any_element()),
            Detail::Group(vault, group) => {
                let group = self.render_group(*vault, *group, theme, cx);
                form(pane, group)
            }
            Detail::Moving(item) => {
                let picker = self.render_move_picker(*item, theme, cx);
                form(pane, picker)
            }
            Detail::Entry => self.render_entry(pane, edit_keys, theme, cx),
        }
    }

    fn render_entry(
        &self,
        pane: Stateful<Div>,
        edit_keys: Option<Kbd>,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let Some((vault, entry)) = workspace
            .view()
            .entry
            .and_then(|(vault, id)| workspace.entry(vault, id).map(|entry| (vault, entry)))
        else {
            return pane
                .child(ui::empty_state(
                    IconName::KeyRound,
                    strings::DETAIL_EMPTY,
                    theme,
                ))
                .into_any_element();
        };
        let id = entry.id();
        let header = self.render_entry_header(vault, entry, edit_keys, theme, cx);
        let secrets = self.render_secrets(vault, entry, theme, cx);
        let tabs = self.render_tabs(vault, entry, theme, cx);
        let content = match self.detail_tab {
            DetailTab::Overview => self.render_overview(vault, entry, theme, cx),
            DetailTab::Fields => self.render_custom_fields(vault, entry, theme, cx),
            DetailTab::Files => self.render_attachments(entry, theme, cx),
            DetailTab::Access => self.render_access(vault, id, entry, theme, cx),
        };
        pane.child(
            div()
                .max_w(px(CONTENT_MAX_WIDTH))
                .flex()
                .flex_col()
                .gap(px(20.))
                .px(px(28.))
                .pt(px(24.))
                .pb(px(32.))
                .when_some(self.error.clone(), |this, error| {
                    this.child(ui::error_text(error, theme))
                })
                .child(header)
                .child(secrets)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(tabs)
                        .child(content),
                ),
        )
        .into_any_element()
    }

    /// The Entry's picture, title and site, with its commands.
    fn render_entry_header(
        &self,
        vault: VaultId,
        entry: &Entry,
        edit_keys: Option<Kbd>,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let id = entry.id();
        let group = self.group_of(vault, id, cx);
        let host = site_host(entry);
        let url = entry.url().to_owned();
        let menu = (self.menu == Some(ListMenu::EntryMore)).then(|| {
            let mut items = vec![
                MenuItem::new(strings::TOTP_BUTTON, false, move |this, window, cx| {
                    this.open_totp_setup(vault, id, window, cx)
                }),
                MenuItem::new(strings::HISTORY_BUTTON, false, move |this, window, cx| {
                    this.open_history(vault, id, window, cx)
                }),
                MenuItem::new(strings::MOVE, false, move |this, _, _| {
                    this.detail = Detail::Moving(Subject::Entry(vault, id));
                }),
            ];
            items.push(
                MenuItem::new(strings::DELETE, false, move |this, window, cx| {
                    this.delete(Subject::Entry(vault, id), window, cx)
                })
                .danger(),
            );
            self.render_menu(items, theme, cx)
        });
        let subtitle = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_body()
            .text_color(theme.fg_1)
            .when_some(host, |this, host| {
                this.child(
                    div()
                        .id("open-url")
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .hover(|style| style.text_color(theme.fg_0))
                        .child(host)
                        .child(icons::ui_icon(IconName::ArrowUpRight, px(12.), theme.fg_2))
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
                .child(div().text_color(theme.fg_2).child("·"))
            })
            .when_some(group, |this, (group_id, name)| {
                this.child(
                    div()
                        .id("entry-group")
                        .cursor_pointer()
                        .hover(|style| style.text_color(theme.fg_0))
                        .child(name)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select(EntryScope::Group(vault, group_id), cx)
                        })),
                )
            })
            .when(workspace.is_expired(entry), |this| {
                this.child(ui::Badge::new(strings::EXPIRED_BADGE).tone(ui::Tone::Danger))
            });
        div()
            .flex()
            .items_start()
            .gap(px(16.))
            .child(entry_avatar(workspace, vault, entry, 48., theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().flex().text_title().child(self.editable_value(
                        vault,
                        id,
                        InlineField::Title,
                        div().truncate().child(title_of(entry)),
                        theme,
                        cx,
                    )))
                    .child(subtitle),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        Button::new("edit-entry", strings::EDIT)
                            .small()
                            .icon(IconName::Pencil)
                            .kbd(edit_keys)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_editor(EditTarget::Existing(vault, id), window, cx)
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            .child(
                                ui::IconButton::new("entry-more", IconName::Ellipsis)
                                    .small()
                                    .selected(menu.is_some())
                                    .tooltip(strings::TOOLTIP_MORE)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.menu = Some(ListMenu::EntryMore);
                                        cx.notify();
                                    })),
                            )
                            .children(menu),
                    ),
            )
            .into_any_element()
    }

    /// The username, password and TOTP code, each copied when clicked.
    fn render_secrets(
        &self,
        vault: VaultId,
        entry: &Entry,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let id = entry.id();
        let password: SharedString = if self.password_revealed {
            entry.password().expose_secret().to_owned().into()
        } else {
            strings::PASSWORD_MASK_CHAR
                .repeat(
                    entry
                        .password()
                        .expose_secret()
                        .chars()
                        .count()
                        .clamp(8, 16),
                )
                .into()
        };
        let username = secret_card(
            "username-card",
            IconName::User,
            strings::DETAIL_USERNAME,
            theme,
        )
        .child(self.secret_value(
            vault,
            id,
            InlineField::Username,
            div().truncate().child(entry.username().to_owned()),
        ))
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            this.on_secret_click(event, vault, id, InlineField::Username, window, cx)
        }));
        let password = secret_card(
            "password-card",
            IconName::KeyRound,
            strings::DETAIL_PASSWORD,
            theme,
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    self.secret_value(
                        vault,
                        id,
                        InlineField::Password,
                        div()
                            .truncate()
                            .font_family(MONO_FONT)
                            .when(!self.password_revealed, |this| this.text_color(theme.fg_1))
                            .child(password),
                    ),
                )
                .child(
                    ui::IconButton::new(
                        "reveal-password",
                        if self.password_revealed {
                            IconName::EyeOff
                        } else {
                            IconName::Eye
                        },
                    )
                    .small()
                    .tooltip(if self.password_revealed {
                        strings::HIDE_PASSWORD
                    } else {
                        strings::REVEAL_PASSWORD
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        // Not the card's click, which copies.
                        cx.stop_propagation();
                        this.password_revealed = !this.password_revealed;
                        cx.notify();
                    })),
                ),
        )
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            this.on_secret_click(event, vault, id, InlineField::Password, window, cx)
        }));
        let totp = workspace.totp_code(vault, id).map(|code| match code {
            Ok(code) => self.render_totp_card(code, theme, cx),
            Err(error) => secret_card("totp-card", IconName::Timer, strings::DETAIL_TOTP, theme)
                .child(div().text_secondary().text_color(theme.danger).child(
                    strings::TOTP_UNREADABLE.replace("{reason}", totp_setup::describe(error)),
                ))
                .into_any_element(),
        });
        div()
            .flex()
            .gap(px(10.))
            .child(username)
            .child(password)
            .children(totp)
            .into_any_element()
    }

    /// A click on the username or password copies it; a double click changes it in place.
    fn on_secret_click(
        &mut self,
        event: &ClickEvent,
        vault: VaultId,
        entry: EntryId,
        field: InlineField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inline_input(vault, entry, field).is_some() {
            return;
        }
        if event.click_count() >= 2 {
            self.start_inline_edit(vault, entry, field, window, cx);
            return;
        }
        let copy = match field {
            InlineField::Username => keefast_workspace::CopyField::Username,
            _ => keefast_workspace::CopyField::Password,
        };
        self.copy(copy, cx);
    }

    /// The value of a secret card, or its text field while it is being changed in place.
    fn secret_value(
        &self,
        vault: VaultId,
        entry: EntryId,
        field: InlineField,
        value: impl IntoElement,
    ) -> AnyElement {
        match self.inline_input(vault, entry, field) {
            Some(input) => div().flex_1().min_w_0().child(input).into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .h(px(24.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .child(value)
                .into_any_element(),
        }
    }

    /// The current TOTP code with a ring counting down its time, copied when clicked.
    fn render_totp_card(&self, code: TotpCode, theme: Theme, cx: &Context<Self>) -> AnyElement {
        let copied =
            self.selected_entry(cx).is_some() && self.totp_copied == self.selected_entry(cx);
        let left = code.remaining() as f32 / code.period() as f32;
        let warn = code.remaining() <= 5;
        secret_card("totp-card", IconName::Timer, strings::DETAIL_TOTP, theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(24.))
                    .child(ui::countdown_ring(
                        left,
                        16.,
                        if warn { theme.warn } else { theme.accent },
                        theme.border_strong,
                    ))
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(px(17.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(super::grouped(code.code())),
                    )
                    .child(
                        div()
                            .text_secondary()
                            .text_color(if copied { theme.ok } else { theme.fg_2 })
                            .child(if copied {
                                SharedString::from(strings::TOTP_COPIED)
                            } else {
                                strings::TOTP_SECONDS_LEFT
                                    .replace("{seconds}", &code.remaining().to_string())
                                    .into()
                            }),
                    ),
            )
            .on_click(cx.listener(|this, _, _, cx| this.copy_totp(cx)))
            .into_any_element()
    }

    fn render_tabs(
        &self,
        vault: VaultId,
        entry: &Entry,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = entry.id();
        let tab =
            |tab: DetailTab, label: &'static str, count: Option<usize>, cx: &Context<Self>| {
                ui::tab(
                    ("detail-tab", tab as usize),
                    label,
                    count,
                    self.detail_tab == tab,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.detail_tab = tab;
                    cx.notify();
                }))
                .into_any_element()
            };
        let fields = entry.custom_fields().len();
        let files = entry.attachments().len();
        let history = entry.history().len();
        let access = entry.remembered_access().len();
        let mut tabs = vec![tab(DetailTab::Overview, strings::TAB_OVERVIEW, None, cx)];
        if fields > 0 {
            tabs.push(tab(
                DetailTab::Fields,
                strings::TAB_FIELDS,
                Some(fields),
                cx,
            ));
        }
        if files > 0 {
            tabs.push(tab(DetailTab::Files, strings::TAB_FILES, Some(files), cx));
        }
        if history > 0 {
            // The history takes the pane, as the Entry's other forms do.
            tabs.push(
                ui::tab(
                    "detail-tab-history",
                    strings::TAB_HISTORY,
                    Some(history),
                    false,
                    theme,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.open_history(vault, id, window, cx)
                    }),
                )
                .into_any_element(),
            );
        }
        if access > 0 {
            tabs.push(tab(
                DetailTab::Access,
                strings::TAB_ACCESS,
                Some(access),
                cx,
            ));
        }
        div()
            .flex()
            .gap(px(2.))
            .border_b_1()
            .border_color(theme.border)
            .children(tabs)
            .into_any_element()
    }

    /// The Entry's URL, notes and particulars.
    fn render_overview(
        &self,
        vault: VaultId,
        entry: &Entry,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let id = entry.id();
        let url = entry.url().to_owned();
        let has_url = !url.is_empty();
        let url_card = ui::card(theme).child(
            ui::kv_row(strings::DETAIL_URL, true, theme)
                .child(
                    self.editable_value(
                        vault,
                        id,
                        InlineField::Url,
                        div()
                            .truncate()
                            .text_color(if has_url { theme.fg_0 } else { theme.fg_2 })
                            .child(if has_url {
                                url.clone()
                            } else {
                                strings::NONE_SET.to_owned()
                            }),
                        theme,
                        cx,
                    ),
                )
                .when(has_url, |this| {
                    this.child(
                        ui::IconButton::new("open-url-button", IconName::ExternalLink)
                            .small()
                            .tooltip(strings::TOOLTIP_OPEN_URL)
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                }),
        );
        let notes = entry.notes().to_owned();
        let has_notes = !notes.is_empty();
        let notes_card = ui::card(theme).px(px(8.)).py(px(6.)).child(
            self.editable_value(
                vault,
                id,
                InlineField::Notes,
                div()
                    .when(!has_notes, |this| this.text_color(theme.fg_2))
                    .child(if has_notes {
                        notes
                    } else {
                        strings::NOTES_EMPTY.to_owned()
                    }),
                theme,
                cx,
            ),
        );

        let tags = entry.tags().iter().enumerate().map(|(ix, tag)| {
            let scope = EntryScope::Tag(tag.clone());
            ui::tag(tag.clone(), None, theme)
                .id(("detail-tag", ix))
                .cursor_pointer()
                .hover(|style| style.bg(theme.bg_hover))
                .on_click(cx.listener(move |this, _, _, cx| this.select(scope.clone(), cx)))
        });
        let group = self.group_of(vault, id, cx);
        let expired = workspace.is_expired(entry);
        let date = |time: Option<keefast_core::Timestamp>| {
            time.map_or_else(|| strings::NONE_SET.to_owned(), |time| time.to_date())
        };
        let details = ui::card(theme)
            .when_some(group, |this, (group, name)| {
                this.child(
                    ui::kv_row(strings::DETAIL_GROUP, true, theme).child(
                        div()
                            .id("detail-group")
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .cursor_pointer()
                            .hover(|style| style.text_color(theme.fg_1))
                            .child(icons::standard_icon(FOLDER_ICON, px(14.), theme.fg_1))
                            .child(name)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select(EntryScope::Group(vault, group), cx)
                            })),
                    ),
                )
            })
            .child(
                ui::kv_row(strings::DETAIL_TAGS, false, theme).child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .children(tags)
                        .when(entry.tags().is_empty(), |this| {
                            this.text_color(theme.fg_2).child(strings::NONE_SET)
                        }),
                ),
            )
            .child(
                ui::kv_row(strings::FIELD_COLOR, false, theme).child(match entry.color() {
                    Some(color) => div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::dot(theme.label(color), 8.))
                        .child(color_name(color)),
                    None => div().text_color(theme.fg_2).child(strings::NO_COLOR),
                }),
            )
            .child(
                ui::kv_row(strings::DETAIL_EXPIRES, false, theme).child(match entry.expires() {
                    Some(expires) => div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(expired, |this| this.text_color(theme.danger))
                        .child(expires.to_date())
                        .when(expired, |this| {
                            this.child(
                                ui::Badge::new(strings::EXPIRED_BADGE).tone(ui::Tone::Danger),
                            )
                        }),
                    None => div().text_color(theme.fg_2).child(strings::NEVER),
                }),
            )
            .child(
                ui::kv_row(strings::DETAIL_MODIFIED, false, theme)
                    .child(date(entry.last_modified())),
            )
            .child(ui::kv_row(strings::DETAIL_CREATED, false, theme).child(date(entry.created())));

        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(url_card)
            .child(section(strings::DETAIL_NOTES).child(notes_card))
            .child(section(strings::DETAILS).child(details))
            .into_any_element()
    }

    /// The Entry's custom fields, each reached with Tab and copied with Enter or Ctrl+C.
    fn render_custom_fields(
        &self,
        vault: VaultId,
        entry: &Entry,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = entry.id();
        let rows = entry
            .custom_fields()
            .iter()
            .zip(&self.custom_field_focus)
            .enumerate()
            .map(|(index, (custom, focus))| {
                let revealed = self.revealed_fields.contains(&index);
                let masked = custom.protected && !revealed;
                let value: SharedString = if masked {
                    strings::HIDDEN_PASSWORD.into()
                } else {
                    custom.value.expose_secret().to_owned().into()
                };
                let copied = self.copied_field == Some(index);
                div()
                    .id(("custom-field", index))
                    .key_context(actions::CUSTOM_FIELD_CONTEXT)
                    .track_focus(focus)
                    .on_action(cx.listener(move |this, _: &CopyCustomField, _, cx| {
                        this.copy_custom_field(index, cx)
                    }))
                    .on_key_down(cx.listener(move |this, event, window, cx| {
                        this.on_custom_field_key(index, event, window, cx)
                    }))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_h(px(44.))
                    .px(px(14.))
                    .when(index > 0, |this| {
                        this.border_t_1().border_color(theme.border)
                    })
                    .focus(|style| style.bg(theme.bg_hover))
                    .child(
                        div()
                            .w(px(120.))
                            .flex_none()
                            .truncate()
                            .text_color(theme.fg_2)
                            .child(custom.name.clone()),
                    )
                    .child(
                        self.editable_value(
                            vault,
                            id,
                            InlineField::Custom(index),
                            div()
                                .truncate()
                                .when(custom.protected, |this| this.font_family(MONO_FONT))
                                .when(masked, |this| this.text_color(theme.fg_1))
                                .child(value),
                            theme,
                            cx,
                        ),
                    )
                    .when(custom.protected, |this| {
                        this.child(
                            ui::IconButton::new(
                                ("reveal-custom-field", index),
                                if revealed {
                                    IconName::EyeOff
                                } else {
                                    IconName::Eye
                                },
                            )
                            .small()
                            .tooltip(if revealed {
                                strings::HIDE_PASSWORD
                            } else {
                                strings::REVEAL_PASSWORD
                            })
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.toggle_field_reveal(index, cx),
                            )),
                        )
                    })
                    .child(
                        ui::IconButton::new(
                            ("copy-custom-field", index),
                            if copied {
                                IconName::Check
                            } else {
                                IconName::Copy
                            },
                        )
                        .small()
                        .tooltip(if copied {
                            strings::COPIED
                        } else {
                            strings::COPY
                        })
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.copy_custom_field(index, cx)),
                        ),
                    )
            });
        ui::card(theme).children(rows).into_any_element()
    }

    /// The Entry's attachments, each with its size, a preview when it is text or an image, and
    /// a way to save it to disk.
    fn render_attachments(&self, entry: &Entry, theme: Theme, cx: &Context<Self>) -> AnyElement {
        let rows = entry
            .attachments()
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                let preview = self
                    .attachment_preview
                    .as_ref()
                    .filter(|preview| preview.shows(index, attachment));
                let previewable = attachment.preview() != AttachmentPreview::None;
                div()
                    .flex()
                    .flex_col()
                    .when(index > 0, |this| {
                        this.border_t_1().border_color(theme.border)
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .min_h(px(44.))
                            .px(px(14.))
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
                            .when(previewable, |this| {
                                this.child(
                                    Button::new(
                                        ("preview-attachment", index),
                                        if preview.is_some() {
                                            strings::HIDE_PREVIEW
                                        } else {
                                            strings::PREVIEW_ATTACHMENT
                                        },
                                    )
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.toggle_attachment_preview(index, cx)
                                        },
                                    )),
                                )
                            })
                            .child(
                                Button::new(("save-attachment", index), strings::SAVE_ATTACHMENT)
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.save_attachment(index, cx)
                                    })),
                            ),
                    )
                    .when_some(preview, |this, preview| {
                        this.child(div().px(px(14.)).pb(px(12.)).child(preview.render(theme)))
                    })
            });
        ui::card(theme).children(rows).into_any_element()
    }

    /// The Access decisions remembered on the Entry, each of which can be forgotten.
    fn render_access(
        &self,
        vault: VaultId,
        id: EntryId,
        entry: &Entry,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let rows = entry
            .remembered_access()
            .into_iter()
            .enumerate()
            .map(|(ix, access)| {
                let host: SharedString = access.host.into();
                let (decision, tone) = match access.decision {
                    AccessDecision::Allow => (strings::ACCESS_ALLOWED, ui::Tone::Neutral),
                    AccessDecision::Deny => (strings::ACCESS_DENIED, ui::Tone::Danger),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_h(px(44.))
                    .px(px(14.))
                    .when(ix > 0, |this| this.border_t_1().border_color(theme.border))
                    .child(icons::ui_icon(IconName::Globe, px(15.), theme.fg_1))
                    .child(div().flex_1().min_w_0().truncate().child(host.clone()))
                    .child(ui::Badge::new(decision).tone(tone))
                    .child(
                        Button::new(("forget-access", ix), strings::ACCESS_FORGET)
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.forget_access(vault, id, Some(host.clone()), cx)
                            })),
                    )
            });
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_secondary()
                            .text_color(theme.fg_1)
                            .child(strings::DETAIL_BROWSER_ACCESS),
                    )
                    .child(
                        Button::new("forget-all-access", strings::ACCESS_FORGET_ALL)
                            .small()
                            .danger_ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.forget_access(vault, id, None, cx)
                            })),
                    ),
            )
            .child(ui::card(theme).children(rows))
            .into_any_element()
    }

    /// What can be done with a Group, from its pencil in the sidebar.
    fn render_group(
        &self,
        vault: VaultId,
        group: keefast_core::GroupId,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let Some(found) = workspace.group(vault, group) else {
            return div().into_any_element();
        };
        let subject = Subject::Group(vault, group);
        let is_browser_group = workspace.browser_group(vault) == Some(group);
        let in_bin = workspace
            .vault(vault)
            .is_none_or(|open| open.vault().is_group_in_recycle_bin(group) != Ok(false));
        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(ui::picture_tile(
                        icons::standard_icon(FOLDER_ICON, px(24.), theme.fg_1),
                        48.,
                        theme,
                    ))
                    .child(div().text_title().truncate().child(found.name().to_owned())),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(
                        Button::new("group-new-entry", strings::NEW_ENTRY)
                            .small()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_editor(EditTarget::New(vault, group), window, cx)
                            })),
                    )
                    .child(
                        Button::new("group-new-group", strings::NEW_GROUP)
                            .small()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.new_group(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("rename-group", strings::RENAME)
                            .small()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_rename(vault, group, window, cx)
                            })),
                    )
                    .child(
                        Button::new("move-group", strings::MOVE)
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.detail = Detail::Moving(subject);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("delete-group", strings::DELETE)
                            .small()
                            .danger_ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.delete(subject, window, cx)
                            })),
                    ),
            )
            .when(!in_bin, |this| {
                this.child(match is_browser_group {
                    true => div()
                        .flex()
                        .child(ui::Badge::new(strings::BROWSER_GROUP_MARK))
                        .into_any_element(),
                    false => div()
                        .flex()
                        .child(
                            Button::new("browser-group", strings::USE_AS_BROWSER_GROUP)
                                .small()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.edit(cx, |workspace| {
                                        workspace.set_browser_group(vault, group)
                                    });
                                })),
                        )
                        .into_any_element(),
                })
            })
            .child(div().flex().justify_end().child(
                Button::new("close-group", strings::EDITOR_DONE).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.detail = Detail::Entry;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    },
                )),
            ))
            .into_any_element()
    }

    /// Every Group of the Vault, to pick where an Entry or Group moves to.
    fn render_move_picker(&self, item: Subject, theme: Theme, cx: &Context<Self>) -> AnyElement {
        let Some(open) = self.workspace.read(cx).vault(item.vault()) else {
            return div().into_any_element();
        };
        let root = open.vault().root();
        let mut groups = vec![(root, 0)];
        flatten_groups(root.groups(), 1, &mut groups);
        let rows: Vec<AnyElement> = groups
            .into_iter()
            .enumerate()
            .map(|(ix, (group, depth))| {
                let to = group.id();
                let name: SharedString = if depth == 0 {
                    open.display_name().into()
                } else {
                    group.name().to_owned().into()
                };
                ui::nav_row(("move-target", ix), false, theme)
                    .pl(px(8. + 14. * depth as f32))
                    .child(icons::standard_icon(FOLDER_ICON, px(15.), theme.fg_1))
                    .child(name)
                    .on_click(cx.listener(move |this, _, _, cx| this.move_to(item, to, cx)))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().text_dialog_title().child(strings::MOVE_TO))
            .child(ui::card(theme).p(px(4.)).children(rows))
            .child(div().flex().justify_end().child(
                Button::new("cancel-move", strings::CANCEL).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.detail = Detail::Entry;
                        cx.notify();
                    },
                )),
            ))
            .into_any_element()
    }
}

/// A card for a secret: its label with an icon over its value.
fn secret_card(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    theme: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(14.))
        .py(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg_1)
        .cursor_pointer()
        .hover(|style| style.border_color(theme.border_strong))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_secondary()
                .text_color(theme.fg_2)
                .child(icons::ui_icon(icon, px(13.), theme.fg_2))
                .child(label),
        )
}

/// A titled section of the detail.
fn section(title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().text_heading().child(title))
}
