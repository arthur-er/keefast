use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    PromptLevel, SharedString, Window, div, prelude::*, px,
};
use keefast_core::{EditError, Entry, EntryFields, EntryId, Group, GroupId};
use keefast_workspace::{EditVaultError, EntryScope, PendingSave, SaveStatus, VaultId, Workspace};

use crate::entry_editor::{EntryEditor, EntryEditorEvent};
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::widgets::{button, tool_button};

/// Emitted when the user asks to open another Vault.
pub struct OpenAnotherVault;

/// The KeeWeb-style three panes: Vaults and Groups sidebar → Entry list → Entry detail, with
/// commands to edit Entries and Groups and to save the Vaults.
pub struct VaultBrowser {
    workspace: Entity<Workspace>,
    focus: FocusHandle,
    scope: EntryScope,
    selected_entry: Option<(VaultId, EntryId)>,
    detail: Detail,
    /// The Group being renamed in the sidebar, with its name field.
    renaming: Option<(VaultId, GroupId, Entity<TextField>)>,
    error: Option<SharedString>,
}

/// What the detail pane shows besides the selected Entry.
enum Detail {
    Entry,
    Editing {
        editor: Entity<EntryEditor>,
        target: EditTarget,
    },
    /// Picking the Group to move an Entry or Group to.
    Moving(Subject),
}

enum EditTarget {
    Existing(VaultId, EntryId),
    New(VaultId, GroupId),
}

#[derive(Clone, Copy)]
enum Subject {
    Entry(VaultId, EntryId),
    Group(VaultId, GroupId),
}

impl Subject {
    fn vault(self) -> VaultId {
        match self {
            Subject::Entry(vault, _) | Subject::Group(vault, _) => vault,
        }
    }
}

impl EventEmitter<OpenAnotherVault> for VaultBrowser {}

impl Focusable for VaultBrowser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl VaultBrowser {
    pub fn new(workspace: Entity<Workspace>, vault: VaultId, cx: &mut Context<Self>) -> Self {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        Self {
            workspace,
            focus: cx.focus_handle(),
            scope: EntryScope::Vault(vault),
            selected_entry: None,
            detail: Detail::Entry,
            renaming: None,
            error: None,
        }
    }

    /// Shows a Vault's Entries, as after opening it.
    pub fn show_vault(&mut self, vault: VaultId, cx: &mut Context<Self>) {
        self.select(EntryScope::Vault(vault), cx);
    }

    /// The Group new Entries and Groups go into, if there is one for what the list shows.
    fn target_group(&self, cx: &App) -> Option<(VaultId, GroupId)> {
        self.workspace.read(cx).group_for_new_items(self.scope)
    }

    fn select(&mut self, scope: EntryScope, cx: &mut Context<Self>) {
        self.scope = scope;
        self.selected_entry = None;
        self.detail = Detail::Entry;
        self.error = None;
        cx.notify();
    }

    fn select_entry(&mut self, vault: VaultId, entry: EntryId, cx: &mut Context<Self>) {
        self.selected_entry = Some((vault, entry));
        self.detail = Detail::Entry;
        self.error = None;
        cx.notify();
    }

    /// Runs an edit command, shows its error if it fails, and autosaves if it succeeds.
    fn edit<T>(
        &mut self,
        cx: &mut Context<Self>,
        command: impl FnOnce(&mut Workspace) -> Result<T, EditVaultError>,
    ) -> Option<T> {
        let result = self.workspace.update(cx, |workspace, cx| {
            let result = command(workspace);
            cx.notify();
            result
        });
        cx.notify();
        match result {
            Ok(value) => {
                self.error = None;
                self.start_autosaves(cx);
                Some(value)
            }
            Err(error) => {
                self.error = Some(describe(&error).into());
                None
            }
        }
    }

    fn open_editor(&mut self, target: EditTarget, window: &mut Window, cx: &mut Context<Self>) {
        let fields = match &target {
            EditTarget::Existing(vault, id) => self
                .workspace
                .read(cx)
                .entry(*vault, *id)
                .map(EntryFields::from)
                .unwrap_or_default(),
            EditTarget::New(..) => EntryFields::default(),
        };
        let editor = cx.new(|cx| EntryEditor::new(&fields, cx));
        cx.subscribe_in(&editor, window, Self::on_editor_event)
            .detach();
        window.focus(&editor.focus_handle(cx));
        self.detail = Detail::Editing { editor, target };
        self.error = None;
        cx.notify();
    }

    fn on_editor_event(
        &mut self,
        _: &Entity<EntryEditor>,
        event: &EntryEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Detail::Editing { target, .. } = &self.detail else {
            return;
        };
        match event {
            EntryEditorEvent::Save(fields) => {
                let saved = match *target {
                    EditTarget::Existing(vault, id) => self
                        .edit(cx, |workspace| workspace.update_entry(vault, id, fields))
                        .map(|()| (vault, id)),
                    EditTarget::New(vault, group) => self
                        .edit(cx, |workspace| workspace.create_entry(vault, group, fields))
                        .map(|id| (vault, id)),
                };
                if let Some(selected) = saved {
                    self.selected_entry = Some(selected);
                    self.detail = Detail::Entry;
                }
            }
            EntryEditorEvent::Cancel => self.detail = Detail::Entry,
        }
        window.focus(&self.focus);
        cx.notify();
    }

    fn new_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((vault, parent)) = self.target_group(cx) else {
            return;
        };
        let created = self.edit(cx, |workspace| {
            workspace.create_group(vault, parent, strings::NEW_GROUP_NAME)
        });
        if let Some(group) = created {
            self.select(EntryScope::Group(vault, group), cx);
            self.start_rename(vault, group, window, cx);
        }
    }

    fn start_rename(
        &mut self,
        vault: VaultId,
        group: GroupId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .workspace
            .read(cx)
            .group(vault, group)
            .map(|group| group.name().to_owned())
            .unwrap_or_default();
        let field = cx.new(|cx| {
            let mut field = TextField::new(strings::GROUP_NAME_PLACEHOLDER, cx);
            field.set_text(&name, cx);
            field
        });
        cx.subscribe_in(&field, window, move |this, field, event, window, cx| {
            if let TextFieldEvent::Submit = event {
                let name = field.read(cx).text().to_owned();
                this.edit(cx, |workspace| workspace.rename_group(vault, group, &name));
            }
            this.renaming = None;
            window.focus(&this.focus);
            cx.notify();
        })
        .detach();
        window.focus(&field.focus_handle(cx));
        self.renaming = Some((vault, group, field));
        cx.notify();
    }

    fn move_to(&mut self, item: Subject, to: GroupId, cx: &mut Context<Self>) {
        let moved = self.edit(cx, |workspace| match item {
            Subject::Entry(vault, entry) => workspace.move_entry(vault, entry, to),
            Subject::Group(vault, group) => workspace.move_group(vault, group, to),
        });
        if moved.is_some() {
            self.detail = Detail::Entry;
        }
    }

    /// Deletes an Entry or Group, asking first when it would be deleted for good rather than
    /// moved to the Recycle Bin.
    fn delete(&mut self, item: Subject, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.workspace.read(cx).vault(item.vault()) else {
            return;
        };
        let vault = open.vault();
        let for_good = match item {
            Subject::Entry(_, entry) => vault.deletes_entry_for_good(entry),
            Subject::Group(_, group) => vault.deletes_group_for_good(group),
        };
        match for_good {
            Ok(false) => self.delete_now(item, cx),
            Ok(true) => {
                let question = match item {
                    Subject::Entry(..) => strings::CONFIRM_DELETE_ENTRY,
                    Subject::Group(..) => strings::CONFIRM_DELETE_GROUP,
                };
                let answer = window.prompt(
                    PromptLevel::Warning,
                    question,
                    Some(strings::CONFIRM_DELETE_DETAIL),
                    &[strings::DELETE, strings::CANCEL],
                    cx,
                );
                cx.spawn(async move |this, cx| {
                    if answer.await == Ok(0) {
                        this.update(cx, |this, cx| this.delete_now(item, cx)).ok();
                    }
                })
                .detach();
            }
            Err(error) => {
                self.error = Some(describe(&EditVaultError::Edit(error)).into());
                cx.notify();
            }
        }
    }

    fn delete_now(&mut self, item: Subject, cx: &mut Context<Self>) {
        let deleted = self.edit(cx, |workspace| match item {
            Subject::Entry(vault, entry) => workspace.delete_entry(vault, entry),
            Subject::Group(vault, group) => workspace.delete_group(vault, group),
        });
        if deleted.is_some() {
            match item {
                Subject::Entry(..) => self.selected_entry = None,
                Subject::Group(vault, _) => self.scope = EntryScope::Vault(vault),
            }
            self.detail = Detail::Entry;
        }
    }

    /// Saves every Vault with unsaved changes.
    fn save_all(&mut self, cx: &mut Context<Self>) {
        let saves = self
            .workspace
            .update(cx, |workspace, _| workspace.begin_saves());
        self.run_saves(saves, cx);
    }

    fn start_autosaves(&mut self, cx: &mut Context<Self>) {
        let saves = self
            .workspace
            .update(cx, |workspace, _| workspace.begin_autosaves());
        self.run_saves(saves, cx);
    }

    fn toggle_autosave(&mut self, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_autosave(!workspace.autosave());
            cx.notify();
        });
        self.start_autosaves(cx);
    }

    /// Encrypts and writes off the UI thread; edits made meanwhile are autosaved afterwards.
    fn run_saves(&mut self, saves: Vec<PendingSave>, cx: &mut Context<Self>) {
        for save in saves {
            let saving = cx.background_executor().spawn(async move { save.run() });
            cx.spawn(async move |this, cx| {
                let finished = saving.await;
                this.update(cx, |this, cx| {
                    // A failure shows in the save status; the Vault stays unsaved.
                    let _ = this.workspace.update(cx, |workspace, cx| {
                        cx.notify();
                        workspace.finish_save(finished)
                    });
                    this.start_autosaves(cx);
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if !keystroke.modifiers.secondary() {
            return;
        }
        match keystroke.key.as_str() {
            "s" => self.save_all(cx),
            "o" => cx.emit(OpenAnotherVault),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        if workspace.vaults().nth(1).is_some() {
            rows.push(
                sidebar_row(
                    ("vault", rows.len()),
                    strings::SIDEBAR_ALL_VAULTS.into(),
                    0,
                    self.scope == EntryScope::AllVaults,
                    theme,
                )
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .on_click(cx.listener(|this, _, _, cx| this.select(EntryScope::AllVaults, cx)))
                .into_any_element(),
            );
        }
        for open in workspace.vaults() {
            let vault = open.id();
            let selected = self.scope == EntryScope::Vault(vault);
            let name = match open.has_unsaved_changes() {
                true => format!("{}{}", open.display_name(), strings::UNSAVED_MARKER),
                false => open.display_name(),
            };
            rows.push(
                sidebar_row(("vault", rows.len()), name.into(), 0, selected, theme)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.select(EntryScope::Vault(vault), cx)
                        }),
                    )
                    .into_any_element(),
            );
            let mut groups = Vec::new();
            flatten_groups(open.vault().root().groups(), 1, &mut groups);
            for (group, depth) in groups {
                let id = group.id();
                if let Some((.., field)) = self
                    .renaming
                    .as_ref()
                    .filter(|(in_vault, renaming, _)| (*in_vault, *renaming) == (vault, id))
                {
                    rows.push(
                        div()
                            .pl(px(8. + 14. * depth as f32))
                            .pr_2()
                            .child(field.clone())
                            .into_any_element(),
                    );
                    continue;
                }
                let selected = self.scope == EntryScope::Group(vault, id);
                rows.push(
                    sidebar_row(
                        ("group", rows.len()),
                        group.name().to_owned().into(),
                        depth,
                        selected,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select(EntryScope::Group(vault, id), cx)
                    }))
                    .into_any_element(),
                );
            }
        }

        div()
            .w(px(220.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .pt_2()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(strings::SIDEBAR_VAULTS)
                    .child(
                        tool_button("open-vault", strings::OPEN_VAULT, theme)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenAnotherVault))),
                    ),
            )
            .child(
                div()
                    .id("sidebar")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .py_2()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(self.render_save_bar(theme, cx))
    }

    fn render_save_bar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let status = workspace.save_status();
        let failed = status == SaveStatus::Failed;
        let status = match status {
            SaveStatus::Saving => strings::STATUS_SAVING,
            SaveStatus::Failed => strings::STATUS_SAVE_FAILED,
            SaveStatus::Unsaved => strings::STATUS_UNSAVED,
            SaveStatus::Saved => strings::STATUS_SAVED,
        };
        let autosave = workspace.autosave();
        let config_error = workspace.config_error().is_some();

        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("save-status")
                    .text_sm()
                    .text_color(if failed { theme.error } else { theme.muted })
                    .child(status),
            )
            .when(config_error, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted)
                        .child(strings::CONFIG_ERROR),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("autosave")
                            .flex()
                            .gap_1()
                            .text_sm()
                            .cursor_pointer()
                            .child(if autosave {
                                strings::CHECKBOX_ON
                            } else {
                                strings::CHECKBOX_OFF
                            })
                            .child(strings::AUTOSAVE)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_autosave(cx))),
                    )
                    .child(
                        tool_button("save", strings::SAVE, theme)
                            .on_click(cx.listener(|this, _, _, cx| this.save_all(cx))),
                    ),
            )
    }

    fn render_entry_list(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let entries = workspace.visible_entries(self.scope);
        let header = match self.scope {
            EntryScope::AllVaults => Some(strings::SIDEBAR_ALL_VAULTS.to_owned()),
            EntryScope::Vault(vault) => workspace.vault(vault).map(|open| open.display_name()),
            EntryScope::Group(vault, group) => workspace
                .group(vault, group)
                .map(|group| group.name().to_owned()),
        }
        .unwrap_or_default();
        // Across several Vaults, each Entry says which Vault it is from.
        let label_vaults =
            self.scope == EntryScope::AllVaults && workspace.vaults().nth(1).is_some();

        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(ix, listed)| {
                let (vault, entry) = (listed.vault, listed.entry);
                let id = entry.id();
                let selected = self.selected_entry == Some((vault, id));
                let vault_name = label_vaults
                    .then(|| workspace.vault(vault).map(|open| open.display_name()))
                    .flatten();
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
                            .flex()
                            .gap_2()
                            .text_sm()
                            .text_color(theme.muted)
                            .child(div().flex_1().truncate().child(entry.username().to_owned()))
                            .when_some(vault_name, |this, name| {
                                this.child(div().flex_none().text_xs().child(name))
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_entry(vault, id, cx)))
                    .into_any_element()
            })
            .collect();

        let mut toolbar = div().flex().flex_wrap().gap_1();
        if let Some((vault, group)) = self.target_group(cx) {
            toolbar = toolbar
                .child(
                    tool_button("new-entry", strings::NEW_ENTRY, theme).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.open_editor(EditTarget::New(vault, group), window, cx)
                        },
                    )),
                )
                .child(
                    tool_button("new-group", strings::NEW_GROUP, theme)
                        .on_click(cx.listener(|this, _, window, cx| this.new_group(window, cx))),
                );
        }
        if let EntryScope::Group(vault, group) = self.scope {
            let subject = Subject::Group(vault, group);
            toolbar = toolbar
                .child(
                    tool_button("rename-group", strings::RENAME, theme).on_click(cx.listener(
                        move |this, _, window, cx| this.start_rename(vault, group, window, cx),
                    )),
                )
                .child(
                    tool_button("move-group", strings::MOVE, theme).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.detail = Detail::Moving(subject);
                            cx.notify();
                        },
                    )),
                )
                .child(
                    tool_button("delete-group", strings::DELETE, theme).on_click(
                        cx.listener(move |this, _, window, cx| this.delete(subject, window, cx)),
                    ),
                );
        }

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
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(header),
                    )
                    .child(toolbar),
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

    fn render_detail(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let pane = div()
            .id("entry-detail")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .bg(theme.panel)
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.error).child(error))
            });

        match &self.detail {
            Detail::Editing { editor, .. } => pane.child(editor.clone()).into_any_element(),
            Detail::Moving(item) => pane
                .child(self.render_move_picker(*item, theme, cx))
                .into_any_element(),
            Detail::Entry => self.render_entry(pane, theme, cx),
        }
    }

    fn render_entry(
        &self,
        pane: gpui::Stateful<gpui::Div>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = self.workspace.read(cx);
        let Some((vault, entry)) = self
            .selected_entry
            .and_then(|(vault, id)| workspace.entry(vault, id).map(|entry| (vault, entry)))
        else {
            return pane
                .items_center()
                .justify_center()
                .text_color(theme.muted)
                .child(strings::DETAIL_EMPTY)
                .into_any_element();
        };
        let id = entry.id();

        let field = |label: &'static str, value: SharedString| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().text_color(theme.muted).child(label))
                .child(div().child(value))
        };

        pane.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().text_xl().truncate().child(title_of(entry)))
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(tool_button("edit-entry", strings::EDIT, theme).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open_editor(EditTarget::Existing(vault, id), window, cx)
                            }),
                        ))
                        .child(tool_button("move-entry", strings::MOVE, theme).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.detail = Detail::Moving(Subject::Entry(vault, id));
                                cx.notify();
                            }),
                        ))
                        .child(
                            tool_button("delete-entry", strings::DELETE, theme).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.delete(Subject::Entry(vault, id), window, cx)
                                }),
                            ),
                        ),
                ),
        )
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
        .into_any_element()
    }

    /// Every Group of the Vault, to pick where an Entry or Group moves to.
    fn render_move_picker(
        &self,
        item: Subject,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                sidebar_row(("move-target", ix), name, depth, false, theme)
                    .rounded_md()
                    .hover(|this| this.bg(theme.selection))
                    .on_click(cx.listener(move |this, _, _, cx| this.move_to(item, to, cx)))
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_lg().child(strings::MOVE_TO))
            .child(div().flex().flex_col().children(rows))
            .child(div().flex().justify_end().child(
                button("cancel-move", strings::CANCEL, theme).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.detail = Detail::Entry;
                        cx.notify();
                    },
                )),
            ))
            .into_any_element()
    }
}

impl Render for VaultBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        div()
            .id("vault-browser")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.text)
            .child(self.render_sidebar(theme, cx))
            .child(self.render_entry_list(theme, cx))
            .child(self.render_detail(theme, cx))
    }
}

fn describe(error: &EditVaultError) -> &'static str {
    match error {
        EditVaultError::Edit(EditError::IntoItself) => strings::ERROR_MOVE_INTO_ITSELF,
        EditVaultError::Edit(EditError::RootGroup) => strings::ERROR_ROOT_GROUP,
        EditVaultError::Edit(EditError::EntryNotFound | EditError::GroupNotFound)
        | EditVaultError::VaultNotOpen => strings::ERROR_NOT_FOUND,
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
