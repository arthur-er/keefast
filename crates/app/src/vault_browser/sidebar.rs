//! The sidebar: the Vault switcher, then the Vault's Groups, the tags and colours of every open
//! Vault, the other Vaults, the Recycle Bin, and the app's commands at the foot.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px,
};
use keefast_core::EntryColor;
use keefast_workspace::{EntryScope, SyncStatus, VaultId, VaultLocation, Workspace};

use super::{
    Detail, FOLDER_ICON, ListMenu, MenuItem, ShowVaultSettings, UnlockLockedVault, VaultBrowser,
    edit_mark, flatten_groups,
};
use crate::actions::{OpenSettings, ToggleCommandPalette};
use crate::icons::{self, IconName};
use crate::locking;
use crate::strings;
use crate::theme::Theme;
use crate::ui::{self, Typography};

/// How many tags the sidebar lists before "more".
const TAGS_SHOWN: usize = 6;

/// How a Vault is doing, as the sidebar and the toolbar say it.
pub(super) struct VaultStatus {
    /// As the sidebar says it.
    pub label: &'static str,
    /// As the toolbar says it, with room to spare.
    pub long_label: &'static str,
    pub color: gpui::Rgba,
}

/// Where a Vault is and how it is doing: Saved, Unsaved, Saving…, or its Sync status.
pub(super) fn vault_status(workspace: &Workspace, vault: VaultId, theme: Theme) -> VaultStatus {
    let Some(open) = workspace.vault(vault) else {
        return VaultStatus {
            label: strings::STATUS_LOCKED,
            long_label: strings::STATUS_LOCKED,
            color: theme.fg_2,
        };
    };
    let (label, long_label, color) = if open.is_saving() {
        (strings::STATUS_SAVING, strings::STATUS_SAVING, theme.fg_2)
    } else if open.save_failed() {
        (
            strings::STATUS_SAVE_FAILED,
            strings::STATUS_SAVE_FAILED,
            theme.danger,
        )
    } else if open.has_unsaved_changes() {
        (strings::VAULT_UNSAVED, strings::STATUS_UNSAVED, theme.warn)
    } else {
        let (label, color) = match open.sync_status() {
            Some(SyncStatus::Synced) => (strings::SYNC_SYNCED, theme.ok),
            Some(SyncStatus::Syncing) => (strings::SYNC_SYNCING, theme.fg_2),
            Some(SyncStatus::Offline) => (strings::SYNC_OFFLINE, theme.warn),
            Some(SyncStatus::Error) => (strings::SYNC_ERROR, theme.danger),
            None => {
                return VaultStatus {
                    label: strings::VAULT_SAVED,
                    long_label: strings::STATUS_SAVED,
                    color: theme.ok,
                };
            }
        };
        (label, label, color)
    };
    VaultStatus {
        label,
        long_label,
        color,
    }
}

fn location_name(location: &VaultLocation) -> &'static str {
    match location {
        VaultLocation::Local(_) => strings::LOCATION_LOCAL,
        _ => strings::LOCATION_DROPBOX,
    }
}

fn location_icon(location: &VaultLocation) -> IconName {
    match location {
        VaultLocation::Local(_) => IconName::HardDrive,
        _ => IconName::Cloud,
    }
}

impl VaultBrowser {
    pub(super) fn render_sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let scope = workspace.view().scope;
        let current = self.current_vault(cx);
        let icon = |icon: IconName, color| icons::ui_icon(icon, px(15.), color).into_any_element();
        let mut rows: Vec<AnyElement> = Vec::new();

        // Every Entry of the Vault, or of every Vault when they're all shown.
        let all_scope = match current {
            Some(vault) if scope != EntryScope::AllVaults => EntryScope::Vault(vault),
            _ => EntryScope::AllVaults,
        };
        let all_count = workspace.visible_entries(all_scope.clone()).len();
        rows.push(
            ui::nav_row("all-entries", scope == all_scope, theme)
                .child(icon(IconName::Layers, theme.fg_1))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .child(strings::SIDEBAR_ALL_ENTRIES),
                )
                .child(ui::nav_count(all_count, theme))
                .on_click(cx.listener(move |this, _, _, cx| this.select(all_scope.clone(), cx)))
                .into_any_element(),
        );

        // The Groups of the Vault shown, the Recycle Bin and what is in it aside.
        let open =
            current.and_then(|vault| workspace.vault(vault).filter(|open| !open.is_locking()));
        let mut bin_row = None;
        if let (Some(vault), Some(open)) = (current, open) {
            let bin = open.vault().recycle_bin();
            let target = self.target_group(cx);
            rows.push(
                sidebar_heading(strings::SIDEBAR_GROUPS, theme)
                    .when(target.is_some(), |this| {
                        this.child(
                            ui::IconButton::new("sidebar-new-group", IconName::Plus)
                                .small()
                                .tooltip(strings::TOOLTIP_NEW_GROUP)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.new_group(window, cx)),
                                ),
                        )
                    })
                    .into_any_element(),
            );
            let mut groups = Vec::new();
            flatten_groups(open.vault().root().groups(), 0, &mut groups);
            let mut in_bin_below = None;
            for (ix, (group, depth)) in groups.into_iter().enumerate() {
                let id = group.id();
                if in_bin_below.is_some_and(|bin_depth| depth > bin_depth) {
                    continue;
                }
                in_bin_below = None;
                if Some(id) == bin {
                    in_bin_below = Some(depth);
                    let selected = scope == EntryScope::Group(vault, id);
                    bin_row = Some(
                        ui::nav_row("recycle-bin", selected, theme)
                            .mx(px(8.))
                            .child(icon(IconName::Trash, theme.fg_1))
                            .child(div().flex_1().truncate().child(strings::SIDEBAR_TRASH))
                            .child(ui::nav_count(group.all_entries().count(), theme))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select(EntryScope::Group(vault, id), cx)
                            }))
                            .into_any_element(),
                    );
                    continue;
                }
                if let Some((.., field)) = self
                    .renaming
                    .as_ref()
                    .filter(|(in_vault, renaming, _)| (*in_vault, *renaming) == (vault, id))
                {
                    rows.push(
                        div()
                            .pl(px(8. + 14. * depth as f32))
                            .child(field.clone())
                            .into_any_element(),
                    );
                    continue;
                }
                let selected = scope == EntryScope::Group(vault, id);
                rows.push(
                    ui::nav_row(("group", ix), selected, theme)
                        .pl(px(8. + 14. * depth as f32))
                        .child(icons::standard_icon(FOLDER_ICON, px(15.), theme.fg_1))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(group.name().to_owned()),
                        )
                        .map(|this| {
                            if selected {
                                this.child(edit_mark(("group-actions", ix), theme).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.detail = Detail::Group(vault, id);
                                        cx.notify();
                                    }),
                                ))
                            } else {
                                this.child(ui::nav_count(group.all_entries().count(), theme))
                            }
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select(EntryScope::Group(vault, id), cx)
                        }))
                        .into_any_element(),
                );
            }
        }

        // The tags and colours of every open Vault.
        let tags = workspace.tags();
        if !tags.is_empty() {
            rows.push(sidebar_heading(strings::SIDEBAR_TAGS, theme).into_any_element());
            let hidden = tags.len().saturating_sub(TAGS_SHOWN);
            let shown = if self.tags_expanded || hidden <= 1 {
                tags.len()
            } else {
                TAGS_SHOWN
            };
            for (ix, tag) in tags.iter().take(shown).enumerate() {
                let tag_scope = EntryScope::Tag((*tag).to_owned());
                let count = workspace.visible_entries(tag_scope.clone()).len();
                rows.push(
                    ui::nav_row(("tag", ix), scope == tag_scope, theme)
                        .child(icon(IconName::Hash, theme.fg_2))
                        .child(div().flex_1().min_w_0().truncate().child((*tag).to_owned()))
                        .child(ui::nav_count(count, theme))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.select(tag_scope.clone(), cx)),
                        )
                        .into_any_element(),
                );
            }
            if hidden > 1 {
                let label: SharedString = if self.tags_expanded {
                    strings::SIDEBAR_FEWER_TAGS.into()
                } else {
                    strings::SIDEBAR_MORE_TAGS
                        .replace("{n}", &hidden.to_string())
                        .into()
                };
                rows.push(
                    ui::nav_row("more-tags", false, theme)
                        .text_color(theme.fg_2)
                        .child(icon(IconName::Ellipsis, theme.fg_2))
                        .child(label)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.tags_expanded = !this.tags_expanded;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        let dots = EntryColor::ALL.into_iter().map(|color| {
            let selected = scope == EntryScope::Color(color);
            div()
                .id(("color", color as usize))
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .when(selected, |this| this.bg(theme.bg_active))
                .hover(|style| style.bg(theme.bg_hover))
                .child(ui::dot(theme.label(color), 10.))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.select(EntryScope::Color(color), cx)),
                )
        });
        rows.push(
            div()
                .mt(px(10.))
                .px(px(16.))
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .mr(px(8.))
                        .text_micro()
                        .text_color(theme.fg_2)
                        .child(strings::SIDEBAR_COLORS),
                )
                .children(dots)
                .into_any_element(),
        );

        // The Vaults besides the one shown: unlocked ones say how they are doing, locked ones
        // unlock when picked.
        let others: Vec<(VaultId, String, Option<IconName>)> = workspace
            .vaults()
            .map(|open| {
                (
                    open.id(),
                    open.display_name(),
                    Some(location_icon(open.location())),
                )
            })
            .chain(
                workspace
                    .locked_vaults()
                    .map(|locked| (locked.id(), locked.display_name(), None)),
            )
            .filter(|(vault, ..)| Some(*vault) != current)
            .collect();
        if !others.is_empty() {
            rows.push(sidebar_heading(strings::SIDEBAR_OTHER_VAULTS, theme).into_any_element());
            for (ix, (vault, name, location)) in others.into_iter().enumerate() {
                let status = vault_status(workspace, vault, theme);
                let locked = location.is_none();
                rows.push(
                    ui::nav_row(("other-vault", ix), false, theme)
                        .child(icon(location.unwrap_or(IconName::Lock), theme.fg_1))
                        .child(div().flex_1().min_w_0().truncate().child(name))
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(5.))
                                .text_micro()
                                .text_color(theme.fg_2)
                                .when(!locked, |this| this.child(ui::dot(status.color, 6.)))
                                .child(status.label),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if locked {
                                cx.emit(UnlockLockedVault(vault));
                            } else {
                                this.select(EntryScope::Vault(vault), cx);
                            }
                        }))
                        .into_any_element(),
                );
            }
        }

        div()
            .w(self.sidebar_width)
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.bg_sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(self.render_vault_switcher(current, theme, cx))
            .child(
                div()
                    .id("sidebar")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .px(px(8.))
                    .pb(px(8.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .children(bin_row.map(|row| {
                div()
                    .py(px(6.))
                    .border_t_1()
                    .border_color(theme.border)
                    .child(row)
            }))
            .child(self.render_sidebar_footer(theme, cx))
            .into_any_element()
    }

    /// The Vault shown, at the top of the sidebar, opening a menu of the open Vaults.
    fn render_vault_switcher(
        &self,
        current: Option<VaultId>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let all = workspace.view().scope == EntryScope::AllVaults;
        let (name, location): (String, Option<&VaultLocation>) = match current {
            _ if all => (strings::ALL_VAULTS.to_owned(), None),
            Some(vault) => match workspace.vault(vault) {
                Some(open) => (open.display_name(), Some(open.location())),
                None => match workspace.locked_vault(vault) {
                    Some(locked) => (locked.display_name(), Some(locked.location())),
                    None => (String::new(), None),
                },
            },
            None => (strings::APP_NAME.to_owned(), None),
        };
        let detail: Option<String> = match (current, location) {
            (Some(vault), Some(location)) if !all => Some(format!(
                "{} · {}",
                location_name(location),
                vault_status(workspace, vault, theme).label
            )),
            _ => None,
        };
        let menu = (self.menu == Some(ListMenu::Vaults))
            .then(|| self.render_vault_menu(current, theme, cx));
        div()
            .flex()
            .flex_col()
            .px(px(8.))
            .pt(px(8.))
            .pb(px(6.))
            .child(
                div()
                    .id("vault-switcher")
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(44.))
                    .px(px(8.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(menu.is_some(), |this| this.bg(theme.bg_active))
                    .hover(|style| style.bg(theme.bg_hover))
                    .child(
                        div()
                            .size(px(28.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .bg(theme.accent_bg)
                            .child(icons::ui_icon(IconName::Vault, px(16.), theme.accent_fg)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_heading().child(name))
                            .children(detail.map(|detail| {
                                div()
                                    .truncate()
                                    .text_micro()
                                    .text_color(theme.fg_2)
                                    .child(detail)
                            })),
                    )
                    .child(icons::ui_icon(
                        IconName::ChevronsUpDown,
                        px(14.),
                        theme.fg_2,
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu = Some(ListMenu::Vaults);
                        cx.notify();
                    })),
            )
            .children(menu)
    }

    /// The open and locked Vaults to switch to, and the settings of the one shown.
    fn render_vault_menu(
        &self,
        current: Option<VaultId>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let all = workspace.view().scope == EntryScope::AllVaults;
        let mut items: Vec<MenuItem> = workspace
            .vaults()
            .map(|open| {
                let vault = open.id();
                MenuItem::new(
                    open.display_name(),
                    !all && Some(vault) == current,
                    move |this, _, cx| this.select(EntryScope::Vault(vault), cx),
                )
            })
            .chain(workspace.locked_vaults().map(|locked| {
                let vault = locked.id();
                MenuItem::new(
                    strings::VAULT_LOCKED_NAME.replace("{vault}", &locked.display_name()),
                    false,
                    move |_, _, cx| cx.emit(UnlockLockedVault(vault)),
                )
            }))
            .collect();
        if workspace.vaults().nth(1).is_some() {
            items.push(MenuItem::new(strings::ALL_VAULTS, all, |this, _, cx| {
                this.select(EntryScope::AllVaults, cx)
            }));
        }
        if let Some(vault) = current.filter(|vault| workspace.vault(*vault).is_some()) {
            items.push(MenuItem::new(
                strings::MENU_VAULT_SETTINGS,
                false,
                move |_, _, cx| cx.emit(ShowVaultSettings(vault)),
            ));
        }
        self.render_menu(items, theme, cx)
    }

    /// The foot of the sidebar: opening or creating a Vault, the command palette, the settings
    /// and locking.
    pub fn render_sidebar_footer(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let can_lock = self.workspace.read(cx).vaults().next().is_some();
        let command = |id: &'static str, icon: IconName, tooltip: &'static str| {
            ui::IconButton::new(id, icon).small().tooltip(tooltip)
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .h(px(44.))
            .px(px(8.))
            .border_t_1()
            .border_color(theme.border)
            .child(
                ui::nav_row("footer-open", false, theme)
                    .flex_1()
                    .child(icons::ui_icon(IconName::Plus, px(15.), theme.fg_1))
                    .child(strings::FOOTER_OPEN)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(super::OpenAnotherVault))),
            )
            .child(
                command("footer-help", IconName::Command, strings::TOOLTIP_COMMANDS).on_click(
                    |_, window, cx| window.dispatch_action(Box::new(ToggleCommandPalette), cx),
                ),
            )
            .child(
                command(
                    "footer-settings",
                    IconName::Settings,
                    strings::OPEN_SETTINGS,
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
            )
            .when(can_lock, |this| {
                this.child(
                    command("footer-lock", IconName::Lock, strings::TOOLTIP_LOCK_ALL).on_click(
                        cx.listener(|this, _, _, cx| locking::lock_all(&this.workspace, cx)),
                    ),
                )
            })
            .into_any_element()
    }
}

/// A heading of a section of the sidebar, with room for a button at its end.
fn sidebar_heading(label: &'static str, theme: Theme) -> gpui::Div {
    div()
        .mt(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .pr(px(2.))
        .child(ui::section_heading(label, theme))
}
