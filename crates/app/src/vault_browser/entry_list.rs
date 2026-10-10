//! The Entry list, its Entries under the Group each is in, and the toolbar over it and the
//! Entry detail.

use std::collections::HashMap;

use gpui::{
    AnyElement, Context, Entity, IntoElement, ParentElement, SharedString, Styled, div, prelude::*,
    px, uniform_list,
};
use keefast_core::{Entry, EntryIcon, EntryId, EntrySort, GroupId, Site};
use keefast_workspace::{EntryScope, VaultId, Workspace};

use super::sidebar::vault_status;
use super::{
    Detail, EditTarget, KEY_ICON, ListMenu, MenuItem, UnlockLockedVault, VaultBrowser,
    flatten_groups, sort_label, title_of,
};
use crate::actions;
use crate::entry_editor::EntryEditor;
use crate::favicons;
use crate::icons::{self, IconName};
use crate::strings;
use crate::theme::Theme;
use crate::ui::{self, Button, Kbd, Typography};

/// How tall each row of the Entry list is, Group headings included: the list is a uniform
/// list, which draws only the rows in view.
const ROW_HEIGHT: f32 = 34.;

/// A row of the Entry list.
pub(super) enum ListRow<'a> {
    /// The heading over a Group's Entries.
    Group {
        vault: VaultId,
        group: GroupId,
        name: SharedString,
        count: usize,
        collapsed: bool,
    },
    Entry(VaultId, &'a Entry),
}

/// The Entry list as drawn: the Entries in scope matching the search, in the chosen order, and
/// under a heading per Group when they come from several and no search is narrowing them.
pub(super) struct ListModel<'a> {
    pub rows: Vec<ListRow<'a>>,
}

impl<'a> ListModel<'a> {
    /// The Entries drawn, in the order drawn: those in folded Groups left out.
    pub fn entries(&self) -> impl Iterator<Item = (VaultId, &'a Entry)> + '_ {
        self.rows.iter().filter_map(|row| match row {
            ListRow::Entry(vault, entry) => Some((*vault, *entry)),
            ListRow::Group { .. } => None,
        })
    }

    /// The row the Entry is drawn in.
    pub fn row_of(&self, at: (VaultId, EntryId)) -> Option<usize> {
        self.rows.iter().position(|row| match row {
            ListRow::Entry(vault, entry) => (*vault, entry.id()) == at,
            ListRow::Group { .. } => false,
        })
    }
}

impl VaultBrowser {
    pub(super) fn list_model<'a>(&self, workspace: &'a Workspace) -> ListModel<'a> {
        let entries = self.listed_entries(workspace);
        let flat = |entries: Vec<keefast_workspace::ListedEntry<'a>>| ListModel {
            rows: entries
                .into_iter()
                .map(|listed| ListRow::Entry(listed.vault, listed.entry))
                .collect(),
        };
        if !self.query.is_empty() {
            return flat(entries);
        }
        // Each Entry's Group, and where that Group comes in the sidebar's order.
        let mut parents: HashMap<(VaultId, EntryId), (usize, GroupId, SharedString)> =
            HashMap::new();
        let mut order = 0;
        for open in workspace.readable_vaults() {
            let vault = open.id();
            let root = open.vault().root();
            let mut groups = vec![(root, 0)];
            flatten_groups(root.groups(), 1, &mut groups);
            for (group, depth) in groups {
                let name: SharedString = if depth == 0 {
                    open.display_name().into()
                } else {
                    group.name().to_owned().into()
                };
                for entry in group.entries() {
                    parents.insert((vault, entry.id()), (order, group.id(), name.clone()));
                }
                order += 1;
            }
        }
        let parent = |listed: &keefast_workspace::ListedEntry| {
            parents.get(&(listed.vault, listed.entry.id())).cloned()
        };
        let mut entries = entries;
        // Stable, so each Group's Entries keep the chosen order.
        entries.sort_by_key(|listed| parent(listed).map_or(usize::MAX, |(order, ..)| order));
        let several = entries
            .windows(2)
            .any(|pair| parent(&pair[0]).map(|p| p.0) != parent(&pair[1]).map(|p| p.0));
        if !several {
            return flat(entries);
        }
        let mut rows = Vec::new();
        let mut current = None;
        let mut collapsed = false;
        for listed in &entries {
            let Some((order, group, name)) = parent(listed) else {
                continue;
            };
            if current != Some(order) {
                current = Some(order);
                collapsed = self.collapsed_groups.contains(&(listed.vault, group));
                let count = entries
                    .iter()
                    .filter(|other| parent(other).is_some_and(|p| p.0 == order))
                    .count();
                rows.push(ListRow::Group {
                    vault: listed.vault,
                    group,
                    name,
                    count,
                    collapsed,
                });
            }
            if !collapsed {
                rows.push(ListRow::Entry(listed.vault, listed.entry));
            }
        }
        ListModel { rows }
    }

    /// The rows of the Entry list in `range`, drawn only as they scroll into view, so a long
    /// list scrolls as fast as a short one.
    fn render_list_rows(
        &mut self,
        range: std::ops::Range<usize>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let workspace = self.workspace.read(cx);
        let selected_entry = workspace.view().entry;
        let model = self.list_model(workspace);
        let label_vaults =
            workspace.view().scope.vault().is_none() && workspace.vaults().nth(1).is_some();
        let mut out = Vec::new();
        for ix in range {
            let Some(row) = model.rows.get(ix) else {
                break;
            };
            let element = match row {
                ListRow::Group {
                    vault,
                    group,
                    name,
                    count,
                    collapsed,
                } => {
                    let key = (*vault, *group);
                    div()
                        .id(("list-group", ix))
                        .h(px(ROW_HEIGHT))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .px(px(6.))
                                .h(px(24.))
                                .rounded(px(5.))
                                .cursor_pointer()
                                .text_micro()
                                .text_color(theme.fg_2)
                                .hover(|style| style.text_color(theme.fg_1))
                                .child(icons::ui_icon(
                                    if *collapsed {
                                        IconName::ChevronRight
                                    } else {
                                        IconName::ChevronDown
                                    },
                                    px(12.),
                                    theme.fg_2,
                                ))
                                .child(div().text_color(theme.fg_1).child(name.clone()))
                                .child(SharedString::from(count.to_string())),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.collapsed_groups.remove(&key) {
                                this.collapsed_groups.insert(key);
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                }
                ListRow::Entry(vault, entry) => {
                    let (vault, entry) = (*vault, *entry);
                    let id = entry.id();
                    let selected = selected_entry == Some((vault, id));
                    let expired = workspace.is_expired(entry);
                    let has_totp = entry.totp().is_some();
                    let has_files = !entry.attachments().is_empty();
                    let vault_name = label_vaults
                        .then(|| workspace.vault(vault).map(|open| open.display_name()))
                        .flatten();
                    let row = div()
                        .h_full()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .map(|this| {
                            if selected {
                                this.bg(theme.bg_active)
                            } else {
                                this.hover(|style| style.bg(theme.bg_hover))
                            }
                        })
                        .child(entry_avatar(workspace, vault, entry, 22., theme))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex_none()
                                        .max_w(relative_width(0.6))
                                        .truncate()
                                        .text_strong()
                                        .text_color(if expired { theme.fg_2 } else { theme.fg_0 })
                                        .when(expired, |this| this.line_through())
                                        .child(title_of(entry)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_body()
                                        .text_color(theme.fg_2)
                                        .child(entry.username().to_owned()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(6.))
                                .when_some(vault_name, |this, name| {
                                    this.child(
                                        div().text_micro().text_color(theme.fg_2).child(name),
                                    )
                                })
                                .when(expired, |this| {
                                    this.child(
                                        ui::Badge::new(strings::EXPIRED_BADGE)
                                            .tone(ui::Tone::Danger),
                                    )
                                })
                                .when(has_files, |this| {
                                    this.child(icons::ui_icon(
                                        IconName::Paperclip,
                                        px(13.),
                                        theme.fg_2,
                                    ))
                                })
                                .when(has_totp, |this| {
                                    this.child(icons::ui_icon(IconName::Timer, px(13.), theme.fg_2))
                                })
                                .when_some(entry.color(), |this, color| {
                                    this.child(ui::dot(theme.label(color), 8.))
                                }),
                        );
                    div()
                        .id(("entry", ix))
                        .h(px(ROW_HEIGHT))
                        .px(px(8.))
                        .py(px(1.))
                        .child(row)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // So the keyboard shortcuts act on the Entry just picked.
                            window.focus(&this.focus, cx);
                            this.select_entry(vault, id, cx)
                        }))
                        .into_any_element()
                }
            };
            out.push(element);
        }
        out
    }

    pub(super) fn render_entry_list(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        // Built first, as they need `cx` the rest borrows.
        let adding = self.target_group(cx);
        let add_menu = (self.menu == Some(ListMenu::Add))
            .then_some(adding)
            .flatten()
            .map(|(vault, group)| {
                self.render_menu(
                    vec![
                        MenuItem::new(strings::MENU_NEW_ENTRY, false, move |this, window, cx| {
                            this.open_editor(EditTarget::New(vault, group), window, cx)
                        }),
                        MenuItem::new(strings::MENU_NEW_GROUP, false, |this, window, cx| {
                            this.new_group(window, cx)
                        }),
                    ],
                    theme,
                    cx,
                )
            });
        let sort_menu = (self.menu == Some(ListMenu::Sort)).then(|| {
            let items = EntrySort::ALL
                .into_iter()
                .map(|sort| {
                    MenuItem::new(sort_label(sort), sort == self.sort, move |this, _, cx| {
                        this.sort_by(sort, cx)
                    })
                })
                .collect();
            self.render_menu(items, theme, cx)
        });
        let workspace = self.workspace.read(cx);
        let row_count = self.list_model(workspace).rows.len();
        let searching = !self.query.is_empty();

        div()
            .w(self.entry_list_width)
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(12.))
                    .pt(px(10.))
                    .pb(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .relative()
                            .key_context(actions::SEARCH_CONTEXT)
                            .child(self.search.clone())
                            .child(
                                div()
                                    .absolute()
                                    .right(px(7.))
                                    .top(px(6.))
                                    .child(Kbd::new(["/"])),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                ui::IconButton::new("sort", IconName::ArrowDownWideNarrow)
                                    .selected(sort_menu.is_some())
                                    .tooltip(strings::TOOLTIP_SORT)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.menu = Some(ListMenu::Sort);
                                        cx.notify();
                                    })),
                            )
                            .children(sort_menu),
                    )
                    .when(adding.is_some(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    ui::IconButton::new("add", IconName::Plus)
                                        .selected(add_menu.is_some())
                                        .tooltip(strings::TOOLTIP_ADD)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.menu = Some(ListMenu::Add);
                                            cx.notify();
                                        })),
                                )
                                .children(add_menu),
                        )
                    }),
            )
            .child(if row_count > 0 {
                uniform_list(
                    "entry-list",
                    row_count,
                    cx.processor(move |this, range, _, cx| this.render_list_rows(range, theme, cx)),
                )
                .flex_1()
                .pb(px(8.))
                .track_scroll(&self.entry_list_scroll)
                .into_any_element()
            } else {
                div()
                    .flex_1()
                    .child(ui::empty_state(
                        if searching {
                            IconName::Search
                        } else {
                            IconName::KeyRound
                        },
                        if searching {
                            strings::LIST_NO_MATCHES
                        } else {
                            strings::LIST_EMPTY
                        },
                        theme,
                    ))
                    .into_any_element()
            })
            .into_any_element()
    }

    /// The bar over the Entry list and detail: where the user is, how the Vault is doing, and
    /// the command palette.
    pub(super) fn render_toolbar(
        &self,
        palette_keys: Option<Kbd>,
        save_keys: Option<Kbd>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Detail::Editing { editor, target } = &self.detail {
            return self.render_editing_toolbar(editor.clone(), *target, save_keys, theme, cx);
        }
        let workspace = self.workspace.read(cx);
        let scope = workspace.view().scope;
        let vault_name = |vault: VaultId| {
            workspace
                .vault(vault)
                .map(|open| open.display_name())
                .or_else(|| {
                    workspace
                        .locked_vault(vault)
                        .map(|locked| locked.display_name())
                })
                .unwrap_or_default()
        };
        let crumb = |label: SharedString, last: bool| {
            div()
                .truncate()
                .text_color(if last { theme.fg_0 } else { theme.fg_1 })
                .when(last, |this| this.font_weight(gpui::FontWeight::MEDIUM))
                .child(label)
        };
        let separator = || icons::ui_icon(IconName::ChevronRight, px(13.), theme.fg_2);
        let (icon, trail): (AnyElement, Vec<AnyElement>) = match &scope {
            EntryScope::AllVaults => (
                icons::ui_icon(IconName::Layers, px(15.), theme.fg_1).into_any_element(),
                vec![crumb(strings::ALL_VAULTS.into(), true).into_any_element()],
            ),
            EntryScope::Vault(vault) => (
                icons::ui_icon(IconName::Vault, px(15.), theme.fg_1).into_any_element(),
                vec![crumb(vault_name(*vault).into(), true).into_any_element()],
            ),
            EntryScope::Group(vault, group) => (
                icons::ui_icon(IconName::Vault, px(15.), theme.fg_1).into_any_element(),
                vec![
                    crumb(vault_name(*vault).into(), false).into_any_element(),
                    separator().into_any_element(),
                    crumb(
                        workspace
                            .group(*vault, *group)
                            .map(|group| group.name().to_owned())
                            .unwrap_or_default()
                            .into(),
                        true,
                    )
                    .into_any_element(),
                ],
            ),
            EntryScope::Tag(tag) => (
                icons::ui_icon(IconName::Hash, px(15.), theme.fg_1).into_any_element(),
                vec![crumb(tag.clone().into(), true).into_any_element()],
            ),
            EntryScope::Color(color) => (
                ui::dot(theme.label(*color), 10.).into_any_element(),
                vec![crumb(super::color_name(*color).into(), true).into_any_element()],
            ),
        };
        let status = self
            .current_vault(cx)
            .filter(|vault| workspace.vault(*vault).is_some())
            .map(|vault| {
                let status = vault_status(workspace, vault, theme);
                let label = status.long_label;
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_secondary()
                    .text_color(theme.fg_1)
                    .child(ui::dot(status.color, 6.))
                    .child(label)
            });
        div()
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .border_b_1()
            .border_color(theme.border)
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(trail),
            )
            .when_some(status, |this, status| {
                this.child(status)
                    .child(div().w_px().h(px(16.)).mx(px(4.)).bg(theme.border))
            })
            .child(
                div()
                    .id("toolbar-palette")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .px(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.bg_hover))
                    .child(icons::ui_icon(IconName::Command, px(14.), theme.fg_1))
                    .children(palette_keys)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::ToggleCommandPalette), cx)
                    }),
            )
            .into_any_element()
    }

    /// The toolbar while an Entry is edited: what is edited, and the editor's Cancel and Done.
    fn render_editing_toolbar(
        &self,
        editor: Entity<EntryEditor>,
        target: EditTarget,
        save_keys: Option<Kbd>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let (EditTarget::Existing(vault, _) | EditTarget::New(vault, _)) = target;
        let vault_name = workspace
            .vault(vault)
            .map(|open| open.display_name())
            .unwrap_or_default();
        let title: SharedString = match target {
            EditTarget::Existing(vault, entry) => workspace
                .entry(vault, entry)
                .map(title_of)
                .unwrap_or_default(),
            EditTarget::New(..) => strings::NEW_ENTRY_TITLE.into(),
        };
        let cancel = editor.clone();
        div()
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .border_b_1()
            .border_color(theme.border)
            .child(icons::ui_icon(IconName::Vault, px(15.), theme.fg_1))
            .child(div().text_color(theme.fg_1).child(vault_name))
            .child(icons::ui_icon(IconName::ChevronRight, px(13.), theme.fg_2))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            )
            .child(ui::Badge::new(strings::EDITING).tone(ui::Tone::Accent))
            .child(div().flex_1())
            .child(
                Button::new("editor-cancel", strings::CANCEL)
                    .small()
                    .ghost()
                    .kbd(Some(Kbd::new([strings::KEY_ESC])))
                    .on_click(move |_, _, cx| cancel.update(cx, |editor, cx| editor.cancel(cx))),
            )
            .child(
                Button::new("editor-save", strings::EDITOR_DONE)
                    .small()
                    .primary()
                    .kbd(save_keys)
                    .on_click(move |_, _, cx| editor.update(cx, |editor, cx| editor.save(cx))),
            )
            .into_any_element()
    }

    /// What the list and detail show of a locked Vault, or of one about to lock: a card to
    /// unlock it.
    pub(super) fn render_locked(
        &self,
        vault: VaultId,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let locking = workspace.vault(vault).is_some_and(|open| open.is_locking());
        let name = workspace
            .vault(vault)
            .map(|open| open.display_name())
            .or_else(|| {
                workspace
                    .locked_vault(vault)
                    .map(|locked| locked.display_name())
            })
            .unwrap_or_default();
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(
                ui::card(theme)
                    .w(px(360.))
                    .p(px(24.))
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(44.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(10.))
                            .bg(theme.bg_2)
                            .child(icons::ui_icon(IconName::Lock, px(20.), theme.fg_1)),
                    )
                    .child(
                        div()
                            .text_dialog_title()
                            .child(strings::VAULT_IS_LOCKED.replace("{vault}", &name)),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_color(theme.fg_1)
                            .child(if locking {
                                strings::LOCKING_NOTE
                            } else {
                                strings::VAULT_LOCKED_NOTE
                            }),
                    )
                    .when(!locking, |this| {
                        this.child(
                            Button::new("unlock-vault", strings::UNLOCK_VAULT)
                                .primary()
                                .icon(IconName::LockOpen)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(UnlockLockedVault(vault))
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

fn relative_width(fraction: f32) -> gpui::DefiniteLength {
    gpui::relative(fraction)
}

/// The site of an Entry's URL, without `www.`, if it has one.
pub(super) fn site_host(entry: &Entry) -> Option<String> {
    let host = Site::parse(entry.url())?.host();
    Some(host.strip_prefix("www.").map(str::to_owned).unwrap_or(host))
}

/// What an Entry is drawn as, `size` square: its custom icon if it has one, else its site's
/// favicon once downloaded (when the user turned that on), else the KeePass icon it was given,
/// else a monogram of its site or title.
pub(super) fn entry_avatar(
    workspace: &Workspace,
    vault: VaultId,
    entry: &Entry,
    size: f32,
    theme: Theme,
) -> AnyElement {
    let inner = (size * 0.64).round();
    let Some(open) = workspace.vault(vault) else {
        return ui::monogram(&title_of(entry), size).into_any_element();
    };
    match entry.icon() {
        EntryIcon::Custom(_) => ui::picture_tile(
            icons::entry_icon(entry.icon(), open.vault(), px(inner), theme.fg_1),
            size,
            theme,
        )
        .into_any_element(),
        icon => {
            if let Some(favicon) = favicons::image(workspace, vault, entry.id(), inner) {
                return ui::picture_tile(favicon, size, theme).into_any_element();
            }
            if icon != EntryIcon::Standard(KEY_ICON) {
                return ui::picture_tile(
                    icons::entry_icon(icon, open.vault(), px(inner), theme.fg_1),
                    size,
                    theme,
                )
                .into_any_element();
            }
            let seed = match entry.title() {
                "" => site_host(entry).unwrap_or_default(),
                title => title.to_owned(),
            };
            ui::monogram(&seed, size).into_any_element()
        }
    }
}
