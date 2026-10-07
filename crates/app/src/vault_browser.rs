use gpui::{AnyElement, Context, Entity, SharedString, Window, div, prelude::*, px};
use keefast_core::{Entry, EntryId, Group, GroupId};
use keefast_workspace::{VaultId, Workspace};

use crate::strings;
use crate::theme::Theme;

/// The KeeWeb-style three panes: Groups sidebar → Entry list → Entry detail.
pub struct VaultBrowser {
    workspace: Entity<Workspace>,
    selected_vault: VaultId,
    selected_group: Option<GroupId>,
    selected_entry: Option<EntryId>,
}

impl VaultBrowser {
    pub fn new(workspace: Entity<Workspace>, vault: VaultId, cx: &mut Context<Self>) -> Self {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        Self {
            workspace,
            selected_vault: vault,
            selected_group: None,
            selected_entry: None,
        }
    }

    fn select_group(&mut self, vault: VaultId, group: Option<GroupId>, cx: &mut Context<Self>) {
        self.selected_vault = vault;
        self.selected_group = group;
        self.selected_entry = None;
        cx.notify();
    }

    fn select_entry(&mut self, entry: EntryId, cx: &mut Context<Self>) {
        self.selected_entry = Some(entry);
        cx.notify();
    }

    fn render_sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        for open in workspace.vaults() {
            let vault = open.id();
            let selected = vault == self.selected_vault && self.selected_group.is_none();
            rows.push(
                sidebar_row(
                    ("vault", rows.len()),
                    open.display_name().into(),
                    0,
                    selected,
                    theme,
                )
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .on_click(cx.listener(move |this, _, _, cx| this.select_group(vault, None, cx)))
                .into_any_element(),
            );
            let mut groups = Vec::new();
            flatten_groups(open.vault().root().groups(), 1, &mut groups);
            for (group, depth) in groups {
                let id = group.id();
                let selected = vault == self.selected_vault && self.selected_group == Some(id);
                rows.push(
                    sidebar_row(
                        ("group", rows.len()),
                        group.name().to_owned().into(),
                        depth,
                        selected,
                        theme,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select_group(vault, Some(id), cx)),
                    )
                    .into_any_element(),
                );
            }
        }

        div()
            .id("sidebar")
            .w(px(220.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .py_2()
            .overflow_y_scroll()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .children(rows)
    }

    fn render_entry_list(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let entries = workspace.visible_entries(self.selected_vault, self.selected_group);
        let header = match self.selected_group {
            Some(group) => workspace
                .vault(self.selected_vault)
                .and_then(|open| open.vault().group(group))
                .map(|group| group.name().to_owned()),
            None => Some(strings::SIDEBAR_ALL_ENTRIES.to_owned()),
        }
        .unwrap_or_default();

        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(ix, entry)| {
                let id = entry.id();
                let selected = self.selected_entry == Some(id);
                div()
                    .id(("entry", ix))
                    .flex()
                    .flex_col()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .when(selected, |this| this.bg(theme.selection))
                    .child(div().truncate().child(title_of(entry)))
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .text_color(theme.muted)
                            .child(entry.username().to_owned()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_entry(id, cx)))
                    .into_any_element()
            })
            .collect();

        div()
            .w(px(320.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(header),
            )
            .child(
                div()
                    .id("entry-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |this| {
                        this.p_3()
                            .text_color(theme.muted)
                            .child(strings::LIST_EMPTY)
                    })
                    .children(rows),
            )
    }

    fn render_detail(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let entry = self.selected_entry.and_then(|id| {
            workspace
                .vault(self.selected_vault)
                .and_then(|open| open.vault().entry(id))
        });
        let pane = div()
            .id("entry-detail")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .bg(theme.panel);

        let Some(entry) = entry else {
            return pane
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted)
                .child(strings::DETAIL_EMPTY);
        };

        let field = |label: &'static str, value: SharedString| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().text_color(theme.muted).child(label))
                .child(div().child(value))
        };

        pane.flex()
            .flex_col()
            .gap_4()
            .p_5()
            .child(div().text_xl().child(title_of(entry)))
            .child(field(
                strings::DETAIL_USERNAME,
                entry.username().to_owned().into(),
            ))
            .child(field(
                strings::DETAIL_PASSWORD,
                strings::HIDDEN_PASSWORD.into(),
            ))
            .child(field(strings::DETAIL_URL, entry.url().to_owned().into()))
            .child(field(
                strings::DETAIL_NOTES,
                entry.notes().to_owned().into(),
            ))
    }
}

impl Render for VaultBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        div()
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.text)
            .child(self.render_sidebar(theme, cx))
            .child(self.render_entry_list(theme, cx))
            .child(self.render_detail(theme, cx))
    }
}

fn sidebar_row(
    id: (&'static str, usize),
    label: SharedString,
    depth: usize,
    selected: bool,
    theme: Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .pl(px(12. + 14. * depth as f32))
        .pr_3()
        .py_1()
        .truncate()
        .cursor_pointer()
        .when(selected, |this| this.bg(theme.selection))
        .child(label)
}

fn flatten_groups<'a>(groups: &'a [Group], depth: usize, out: &mut Vec<(&'a Group, usize)>) {
    for group in groups {
        out.push((group, depth));
        flatten_groups(group.groups(), depth + 1, out);
    }
}

fn title_of(entry: &Entry) -> SharedString {
    match entry.title() {
        "" => strings::UNTITLED.into(),
        title => title.to_owned().into(),
    }
}
