use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    PromptLevel, ScrollHandle, SharedString, Task, Window, div, prelude::*, px, relative,
};
use keefast_core::{EditError, Entry, EntryFields, EntryId, Group, GroupId, TotpCode};
use keefast_workspace::{
    CopyError, CopyField, EditVaultError, EntryScope, SaveStatus, VaultId, View, Workspace,
};
use secrecy::ExposeSecret;

use crate::actions::{
    self, CopyCustomField, CopyPassword, CopyTotp, CopyUsername, LockAll, OpenEntry, OpenVault,
    SaveAll, SelectFirstEntry, SelectLastEntry, SelectNextEntry, SelectPreviousEntry,
};
use crate::browser_integration::BrowserIntegration;
use crate::clipboard::AppClipboard;
use crate::entry_editor::{EntryEditor, EntryEditorEvent};
use crate::locking;
use crate::merge_notice;
use crate::saving;
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::totp_setup::{self, TotpSetup, TotpSetupEvent};
use crate::widgets::{button, tool_button};

/// Emitted when the user asks to open another Vault.
pub struct OpenAnotherVault;

/// Emitted when the user asks to unlock a locked Vault.
pub struct UnlockLockedVault(pub VaultId);

/// The KeeWeb-style three panes: Vaults and Groups sidebar → Entry list → Entry detail, with
/// commands to edit Entries and Groups and to save the Vaults.
pub struct VaultBrowser {
    workspace: Entity<Workspace>,
    integration: Entity<BrowserIntegration>,
    focus: FocusHandle,
    detail: Detail,
    /// The Group being renamed in the sidebar, with its name field.
    renaming: Option<(VaultId, GroupId, Entity<TextField>)>,
    /// The Entry whose TOTP code was copied last, to say so until another is selected.
    totp_copied: Option<(VaultId, EntryId)>,
    error: Option<SharedString>,
    /// What the last command did, such as copying a password.
    notice: Option<SharedString>,
    entry_list_scroll: ScrollHandle,
    /// Clears what was last copied from the clipboard when its time comes.
    clipboard_clear: Option<Task<()>>,
    /// One per custom field of the selected Entry, so each can be reached with Tab and copied.
    custom_field_focus: Vec<FocusHandle>,
    /// The selected Entry's protected custom fields being shown, by index.
    revealed_fields: Vec<usize>,
    /// The custom field of the selected Entry last copied, by index.
    copied_field: Option<usize>,
    /// What the latest Merges of changes made elsewhere changed, until dismissed or timed out.
    merge_notice: Option<SharedString>,
    /// Counts the notices shown, so a timeout only hides the one it was set for.
    merge_notice_generation: u64,
}

/// How long a Merge notice stays up, and how many Merges it lists at most.
const MERGE_NOTICE_TIME: Duration = Duration::from_secs(15);
const MERGE_NOTICE_LINES: usize = 3;

/// What the detail pane shows besides the selected Entry.
enum Detail {
    Entry,
    Editing {
        editor: Entity<EntryEditor>,
        target: EditTarget,
    },
    /// Picking the Group to move an Entry or Group to.
    Moving(Subject),
    /// Setting up an Entry's TOTP.
    Totp {
        setup: Entity<TotpSetup>,
        vault: VaultId,
        entry: EntryId,
    },
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
impl EventEmitter<UnlockLockedVault> for VaultBrowser {}

impl Focusable for VaultBrowser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl VaultBrowser {
    pub fn new(
        workspace: Entity<Workspace>,
        integration: Entity<BrowserIntegration>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&workspace, |this, workspace, cx| {
            this.show_merges(&workspace, cx);
            this.leave_locked_vaults(cx);
            cx.notify();
        })
        .detach();
        cx.observe(&integration, |_, _, cx| cx.notify()).detach();
        // Keeps the shown TOTP code and its countdown current.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let shown = this.update(cx, |this, cx| {
                    if this.shows_totp(cx) {
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            workspace,
            integration,
            focus: cx.focus_handle(),
            detail: Detail::Entry,
            renaming: None,
            totp_copied: None,
            error: None,
            notice: None,
            entry_list_scroll: ScrollHandle::new(),
            clipboard_clear: None,
            custom_field_focus: Vec::new(),
            revealed_fields: Vec::new(),
            copied_field: None,
            merge_notice: None,
            merge_notice_generation: 0,
        }
    }

    /// Tells the user about Merges of changes made elsewhere since last shown.
    fn show_merges(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        let merges = workspace.update(cx, |workspace, _| workspace.take_merges());
        if merges.is_empty() {
            return;
        }
        let workspace = workspace.read(cx);
        // Merges in quick succession are listed together, the latest few kept.
        let earlier = self.merge_notice.iter().flat_map(|notice| notice.lines());
        let lines: Vec<String> = earlier
            .map(str::to_owned)
            .chain(merges.iter().map(|merge| {
                // The Vault may have locked since it was Merged.
                let name = workspace
                    .vault(merge.vault)
                    .map(|open| open.display_name())
                    .or_else(|| {
                        workspace
                            .locked_vault(merge.vault)
                            .map(|locked| locked.display_name())
                    })
                    .unwrap_or_default();
                merge_notice::describe(&name, &merge.summary)
            }))
            .collect();
        let lines = &lines[lines.len().saturating_sub(MERGE_NOTICE_LINES)..];
        self.merge_notice = Some(lines.join("\n").into());
        self.merge_notice_generation += 1;
        let shown = self.merge_notice_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MERGE_NOTICE_TIME).await;
            let _ = this.update(cx, |this, cx| {
                if this.merge_notice_generation == shown {
                    this.merge_notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn render_merge_notice(&self, theme: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.merge_notice.clone()?;
        Some(
            div()
                .id("merge-notice")
                .absolute()
                .bottom_4()
                .right_4()
                .w(px(400.))
                .flex()
                .items_start()
                .gap_3()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.panel)
                .text_sm()
                .shadow_md()
                .child(div().flex_1().min_w_0().child(notice))
                .child(
                    div()
                        .id("dismiss-merge-notice")
                        .cursor_pointer()
                        .text_color(theme.muted)
                        .child(strings::MERGE_NOTICE_DISMISS)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.merge_notice = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    /// Closes what the detail pane and the sidebar had open of Vaults that have locked. An
    /// Entry open in the editor is closed with whatever was typed into it: only the Vault's
    /// own unsaved changes are asked about before it locks.
    fn leave_locked_vaults(&mut self, cx: &App) {
        let workspace = self.workspace.read(cx);
        // A Vault waiting to lock is hidden like a locked one, so its panes close too.
        let locked = |vault: VaultId| workspace.readable_vault(vault).is_none();
        let detail_vault = match &self.detail {
            Detail::Entry => None,
            Detail::Editing {
                target: EditTarget::Existing(vault, _) | EditTarget::New(vault, _),
                ..
            } => Some(*vault),
            Detail::Moving(subject) => Some(subject.vault()),
            Detail::Totp { vault, .. } => Some(*vault),
        };
        if detail_vault.is_some_and(locked) {
            self.detail = Detail::Entry;
        }
        if self
            .renaming
            .as_ref()
            .is_some_and(|(vault, ..)| locked(*vault))
        {
            self.renaming = None;
        }
        if self.totp_copied.is_some_and(|(vault, _)| locked(vault)) {
            self.totp_copied = None;
        }
        // Locking clears the clipboard early, which puts the notice about the copy out of date.
        if self.clipboard_clear.is_some() && workspace.clipboard_clears_in().is_none() {
            self.clipboard_clear = None;
            self.notice = None;
        }
    }

    /// The Group new Entries and Groups go into, if there is one for what the list shows.
    fn target_group(&self, cx: &App) -> Option<(VaultId, GroupId)> {
        self.workspace.read(cx).group_for_new_items(self.scope(cx))
    }

    /// Where the user is: what the Entry list shows and the Entry selected, kept by the
    /// Workspace so unlocking a Vault returns there.
    fn view(&self, cx: &App) -> View {
        self.workspace.read(cx).view()
    }

    fn scope(&self, cx: &App) -> EntryScope {
        self.view(cx).scope
    }

    fn selected_entry(&self, cx: &App) -> Option<(VaultId, EntryId)> {
        self.view(cx).entry
    }

    /// Moves the user to `view`. A browser pairs with the Vault shown.
    fn show(&mut self, view: View, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |workspace, _| workspace.show(view));
    }

    /// Shows `scope` in the Entry list.
    fn select(&mut self, scope: EntryScope, cx: &mut Context<Self>) {
        self.show(View::from(scope), cx);
        self.clear_detail(cx);
    }

    /// Shows the Entry selected, or the Vault unlocked, rather than what was open before, as
    /// after selecting or unlocking a Vault.
    pub fn clear_detail(&mut self, cx: &mut Context<Self>) {
        self.detail = Detail::Entry;
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    fn select_entry(&mut self, vault: VaultId, entry: EntryId, cx: &mut Context<Self>) {
        let scope = self.scope(cx);
        self.show(
            View {
                scope,
                entry: Some((vault, entry)),
            },
            cx,
        );
        self.detail = Detail::Entry;
        self.totp_copied = None;
        self.error = None;
        self.notice = None;
        self.revealed_fields.clear();
        self.copied_field = None;
        cx.notify();
    }

    /// Selects the Entry at the position `pick` chooses in the Entry list, given the selected
    /// Entry's position, if any, and the list's length, and scrolls it into view.
    fn select_in_list(
        &mut self,
        pick: impl FnOnce(Option<usize>, usize) -> usize,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.read(cx);
        let View { scope, entry } = workspace.view();
        let entries = workspace.visible_entries(scope);
        if entries.is_empty() {
            return;
        }
        let current = entries
            .iter()
            .position(|listed| Some((listed.vault, listed.entry.id())) == entry);
        let ix = pick(current, entries.len()).min(entries.len() - 1);
        let (vault, entry) = (entries[ix].vault, entries[ix].entry.id());
        self.select_entry(vault, entry, cx);
        self.entry_list_scroll.scroll_to_item(ix);
    }

    fn select_next(&mut self, _: &SelectNextEntry, _: &mut Window, cx: &mut Context<Self>) {
        self.select_in_list(|current, _| current.map_or(0, |ix| ix + 1), cx);
    }

    fn select_previous(&mut self, _: &SelectPreviousEntry, _: &mut Window, cx: &mut Context<Self>) {
        self.select_in_list(
            |current, len| current.map_or(len - 1, |ix| ix.saturating_sub(1)),
            cx,
        );
    }

    fn select_first(&mut self, _: &SelectFirstEntry, _: &mut Window, cx: &mut Context<Self>) {
        self.select_in_list(|_, _| 0, cx);
    }

    fn select_last(&mut self, _: &SelectLastEntry, _: &mut Window, cx: &mut Context<Self>) {
        self.select_in_list(|_, len| len - 1, cx);
    }

    fn open_selected(&mut self, _: &OpenEntry, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((vault, entry)) = self.selected_entry(cx) {
            self.open_editor(EditTarget::Existing(vault, entry), window, cx);
        }
    }

    fn copy_username(&mut self, _: &CopyUsername, _: &mut Window, cx: &mut Context<Self>) {
        self.copy(CopyField::Username, cx);
    }

    fn copy_password(&mut self, _: &CopyPassword, _: &mut Window, cx: &mut Context<Self>) {
        self.copy(CopyField::Password, cx);
    }

    fn copy_totp_code(&mut self, _: &CopyTotp, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_totp(cx);
    }

    /// Copies a field of the selected Entry, and clears it from the clipboard when its time
    /// comes.
    /// Returns whether something was copied.
    fn copy(&mut self, field: CopyField, cx: &mut Context<Self>) -> bool {
        let Some((vault, entry)) = self.selected_entry(cx) else {
            return false;
        };
        let result = self.workspace.update(cx, |workspace, cx| {
            workspace
                .copy_field(&mut AppClipboard(cx), vault, entry, field)
                .map(|()| workspace.clipboard_clears_in())
        });
        let clears_in = match result {
            Ok(clears_in) => clears_in,
            Err(CopyError::Empty) => {
                self.notice = Some(strings::NOTHING_TO_COPY.into());
                cx.notify();
                return false;
            }
            Err(CopyError::NotFound) => {
                self.error = Some(strings::ERROR_NOT_FOUND.into());
                cx.notify();
                return false;
            }
        };
        let copied = match field {
            CopyField::Username => strings::COPIED_USERNAME,
            CopyField::Password => strings::COPIED_PASSWORD,
            CopyField::TotpCode => strings::COPIED_TOTP,
            CopyField::CustomField(_) => strings::COPIED_FIELD,
        };
        self.notice = Some(match clears_in {
            Some(wait) => {
                // Rounded up: a moment has passed since copying.
                let seconds = wait.as_secs_f64().ceil().to_string();
                let clears = strings::COPIED_CLEARS_IN.replace("{seconds}", &seconds);
                format!("{copied} {clears}").into()
            }
            None => copied.into(),
        });
        self.error = None;
        // A new copy replaces the clear waiting for the last one.
        let workspace = self.workspace.clone();
        self.clipboard_clear = clears_in.map(|_| {
            cx.spawn(async move |this, cx| {
                while let Ok(Some(wait)) =
                    workspace.read_with(cx, |workspace, _| workspace.clipboard_clears_in())
                {
                    cx.background_executor().timer(wait).await;
                    let cleared = workspace.update(cx, |workspace, cx| {
                        workspace.clear_clipboard_if_due(&mut AppClipboard(cx))
                    });
                    if cleared.is_err() {
                        return;
                    }
                }
                // The notice about the copy is out of date now.
                this.update(cx, |this, cx| {
                    this.notice = None;
                    cx.notify();
                })
                .ok();
            })
        });
        cx.notify();
        true
    }

    /// Whether the selected Entry has TOTP, whose countdown needs redrawing.
    fn shows_totp(&self, cx: &App) -> bool {
        self.selected_entry(cx)
            .and_then(|(vault, entry)| self.workspace.read(cx).entry(vault, entry))
            .is_some_and(|entry| entry.totp().is_some_and(|totp| totp.is_ok()))
    }

    /// The selected Entry's TOTP code now, if it has TOTP that can be read.
    fn selected_totp(&self, cx: &App) -> Option<TotpCode> {
        let (vault, entry) = self.selected_entry(cx)?;
        self.workspace.read(cx).totp_code(vault, entry)?.ok()
    }

    /// Copies the selected Entry's current TOTP code, to be cleared from the clipboard like a
    /// copied password.
    fn copy_totp(&mut self, cx: &mut Context<Self>) {
        if self.selected_totp(cx).is_none() {
            return;
        }
        if self.copy(CopyField::TotpCode, cx) {
            self.totp_copied = self.selected_entry(cx);
        }
    }

    fn open_totp_setup(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.workspace.read(cx).entry(vault, entry) else {
            return;
        };
        let (title, username) = (current.title().to_owned(), current.username().to_owned());
        let totp = current.totp().and_then(Result::ok).cloned();
        let setup = cx.new(|cx| TotpSetup::new(totp.as_ref(), &title, &username, cx));
        cx.subscribe_in(&setup, window, Self::on_totp_setup_event)
            .detach();
        window.focus(&setup.focus_handle(cx));
        self.detail = Detail::Totp {
            setup,
            vault,
            entry,
        };
        self.error = None;
        cx.notify();
    }

    fn on_totp_setup_event(
        &mut self,
        _: &Entity<TotpSetup>,
        event: &TotpSetupEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Detail::Totp { vault, entry, .. } = self.detail else {
            return;
        };
        match event {
            TotpSetupEvent::Save(_) | TotpSetupEvent::Remove => {
                let totp = match event {
                    TotpSetupEvent::Save(totp) => Some(totp),
                    _ => None,
                };
                let saved = self.edit(cx, |workspace| workspace.set_totp(vault, entry, totp));
                if saved.is_some() {
                    self.detail = Detail::Entry;
                }
            }
            TotpSetupEvent::Cancel => self.detail = Detail::Entry,
        }
        window.focus(&self.focus);
        cx.notify();
    }

    /// Copies a custom field of the selected Entry.
    /// Copies a custom field of the selected Entry, to be cleared like a copied password.
    fn copy_custom_field(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.copy(CopyField::CustomField(index), cx) {
            self.copied_field = Some(index);
        }
    }

    fn toggle_field_reveal(&mut self, index: usize, cx: &mut Context<Self>) {
        match self
            .revealed_fields
            .iter()
            .position(|&shown| shown == index)
        {
            Some(position) => {
                self.revealed_fields.remove(position);
            }
            None => self.revealed_fields.push(index),
        }
        cx.notify();
    }

    /// Keys on a focused custom field besides the actions bound to it (Enter and Ctrl+C copy
    /// it): Space shows or hides it, and Escape goes back to the Entry list.
    fn on_custom_field_key(
        &mut self,
        index: usize,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        match keystroke.key.as_str() {
            "space" if !modifiers.modified() => self.toggle_field_reveal(index, cx),
            "escape" => window.focus(&self.focus),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Makes sure every custom field of the selected Entry has a focus handle.
    fn ensure_custom_field_focus(&mut self, cx: &mut Context<Self>) {
        let workspace = self.workspace.read(cx);
        let count = self
            .selected_entry(cx)
            .and_then(|(vault, id)| workspace.entry(vault, id))
            .map_or(0, |entry| entry.custom_fields().len());
        while self.custom_field_focus.len() < count {
            self.custom_field_focus
                .push(cx.focus_handle().tab_stop(true));
        }
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
        let editor = cx.new(|cx| EntryEditor::new(&fields, self.workspace.clone(), cx));
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
                    let scope = self.scope(cx);
                    self.show(
                        View {
                            scope,
                            entry: Some(selected),
                        },
                        cx,
                    );
                    self.detail = Detail::Entry;
                    self.revealed_fields.clear();
                    self.copied_field = None;
                }
            }
            EntryEditorEvent::Cancel => {
                self.detail = Detail::Entry;
                self.error = None;
            }
        }
        // A refused save leaves the form open to fix what the error says.
        match &self.detail {
            Detail::Editing { editor, .. } => window.focus(&editor.focus_handle(cx)),
            _ => window.focus(&self.focus),
        }
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
            let view = match item {
                Subject::Entry(..) => View::from(self.scope(cx)),
                Subject::Group(vault, _) => View {
                    scope: EntryScope::Vault(vault),
                    entry: self.selected_entry(cx),
                },
            };
            self.show(view, cx);
            self.detail = Detail::Entry;
        }
    }

    /// Saves every Vault with unsaved changes.
    fn save_all(&mut self, cx: &mut Context<Self>) {
        saving::save_all(&self.workspace, cx);
    }

    fn start_autosaves(&mut self, cx: &mut Context<Self>) {
        saving::autosave(&self.workspace, cx);
    }

    fn toggle_autosave(&mut self, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_autosave(!workspace.autosave());
            cx.notify();
        });
        self.start_autosaves(cx);
    }

    fn toggle_browser_integration(&mut self, cx: &mut Context<Self>) {
        self.integration.update(cx, |integration, cx| {
            integration.set_enabled(!integration.enabled(cx), cx)
        });
    }

    /// Tab moves between the Entry's custom fields and the other controls.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "tab" && !keystroke.modifiers.secondary() {
            if keystroke.modifiers.shift {
                window.focus_prev();
            } else {
                window.focus_next();
            }
            cx.stop_propagation();
        }
    }

    fn render_sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let scope = workspace.view().scope;
        let mut rows: Vec<AnyElement> = Vec::new();
        // Unlocked and locked Vaults, in the order they were opened.
        let mut vaults: Vec<VaultId> = workspace
            .vaults()
            .map(|open| open.id())
            .chain(workspace.locked_vaults().map(|locked| locked.id()))
            .collect();
        vaults.sort();
        if vaults.len() > 1 {
            rows.push(
                sidebar_row(
                    ("vault", rows.len()),
                    strings::SIDEBAR_ALL_VAULTS.into(),
                    0,
                    scope == EntryScope::AllVaults,
                    theme,
                )
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .on_click(cx.listener(|this, _, _, cx| this.select(EntryScope::AllVaults, cx)))
                .into_any_element(),
            );
        }
        for vault in vaults {
            let selected = scope == EntryScope::Vault(vault);
            let select =
                cx.listener(move |this, _, _, cx| this.select(EntryScope::Vault(vault), cx));
            let Some(open) = workspace.vault(vault) else {
                let name = workspace
                    .locked_vault(vault)
                    .map(|locked| locked.display_name())
                    .unwrap_or_default();
                rows.push(
                    sidebar_row(
                        ("vault", rows.len()),
                        format!("{name}{}", strings::LOCKED_MARKER).into(),
                        0,
                        selected,
                        theme,
                    )
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.muted)
                    .on_click(select)
                    .into_any_element(),
                );
                continue;
            };
            // A Vault waiting to lock shows nothing of what it holds.
            if open.is_locking() {
                rows.push(
                    sidebar_row(
                        ("vault", rows.len()),
                        format!("{}{}", open.display_name(), strings::LOCKING_MARKER).into(),
                        0,
                        false,
                        theme,
                    )
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.muted)
                    .on_click(select)
                    .into_any_element(),
                );
                continue;
            }
            let name = match open.has_unsaved_changes() {
                true => format!("{}{}", open.display_name(), strings::UNSAVED_MARKER),
                false => open.display_name(),
            };
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .pr_2()
                    .when(selected, |this| this.bg(theme.selection))
                    .child(
                        sidebar_row(("vault", rows.len()), name.into(), 0, false, theme)
                            .flex_1()
                            .min_w_0()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .on_click(select),
                    )
                    .child(
                        tool_button(("lock-vault", rows.len()), strings::LOCK, theme)
                            .text_xs()
                            .text_color(theme.muted)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                locking::lock_vault(&this.workspace, vault, cx)
                            })),
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
                let selected = scope == EntryScope::Group(vault, id);
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
        let can_lock = workspace.vaults().next().is_some();

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
                        div()
                            .flex()
                            .gap_1()
                            .when(can_lock, |this| {
                                this.child(
                                    tool_button("lock-all", strings::LOCK_ALL, theme).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            locking::lock_all(&this.workspace, cx)
                                        }),
                                    ),
                                )
                            })
                            .child(
                                tool_button("open-vault", strings::OPEN_VAULT, theme)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenAnotherVault))),
                            ),
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
        let browser_integration = workspace.browser_integration();
        let serve_error = self.integration.read(cx).serve_error().is_some();
        let browsers = match browser_integration {
            false => None,
            true if serve_error => Some((strings::BROWSER_ALREADY_SERVED.into(), true)),
            true => browser_hosts_summary(workspace),
        };

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
            .child(
                div()
                    .id("browser-integration")
                    .flex()
                    .gap_1()
                    .text_sm()
                    .cursor_pointer()
                    .child(if browser_integration {
                        strings::CHECKBOX_ON
                    } else {
                        strings::CHECKBOX_OFF
                    })
                    .child(strings::BROWSER_INTEGRATION)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_browser_integration(cx))),
            )
            .when_some(browsers, |this, (summary, failed)| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(if failed { theme.error } else { theme.muted })
                        .child(summary),
                )
            })
    }

    fn render_entry_list(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let View {
            scope,
            entry: selected_entry,
        } = workspace.view();
        let entries = workspace.visible_entries(scope);
        let locked = match scope {
            EntryScope::Vault(vault) => workspace.locked_vault(vault),
            _ => None,
        };
        // The Vault shown is waiting to lock, so the list has nothing in it.
        let locking = match scope {
            EntryScope::Vault(vault) | EntryScope::Group(vault, _) => {
                workspace.vault(vault).is_some_and(|open| open.is_locking())
            }
            _ => false,
        };
        let header = match scope {
            EntryScope::AllVaults => Some(strings::SIDEBAR_ALL_VAULTS.to_owned()),
            EntryScope::Vault(vault) => workspace
                .vault(vault)
                .map(|open| open.display_name())
                .or_else(|| locked.map(|locked| locked.display_name())),
            EntryScope::Group(vault, group) => workspace
                .group(vault, group)
                .map(|group| group.name().to_owned()),
        }
        .unwrap_or_default();
        // Across several Vaults, each Entry says which Vault it is from.
        let label_vaults = scope == EntryScope::AllVaults && workspace.vaults().nth(1).is_some();

        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(ix, listed)| {
                let (vault, entry) = (listed.vault, listed.entry);
                let id = entry.id();
                let selected = selected_entry == Some((vault, id));
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
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // So the keyboard shortcuts act on the Entry just picked.
                        window.focus(&this.focus);
                        this.select_entry(vault, id, cx)
                    }))
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
        if let EntryScope::Group(vault, group) = scope {
            let subject = Subject::Group(vault, group);
            let is_browser_group = workspace.browser_group(vault) == Some(group);
            let in_bin = workspace
                .vault(vault)
                .is_none_or(|open| open.vault().is_group_in_recycle_bin(group) != Ok(false));
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
                )
                .when(!in_bin, |toolbar| {
                    toolbar.child(match is_browser_group {
                        true => div()
                            .px_2()
                            .py_0p5()
                            .text_sm()
                            .text_color(theme.muted)
                            .child(strings::BROWSER_GROUP_MARK)
                            .into_any_element(),
                        false => tool_button("browser-group", strings::USE_AS_BROWSER_GROUP, theme)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.edit(cx, |workspace| {
                                    workspace.set_browser_group(vault, group)
                                });
                            }))
                            .into_any_element(),
                    })
                });
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
                    .track_scroll(&self.entry_list_scroll)
                    .when(rows.is_empty() && locked.is_none(), |this| {
                        this.p_3().text_color(theme.muted).child(if locking {
                            strings::LOCKING_NOTE
                        } else {
                            strings::LIST_EMPTY
                        })
                    })
                    .when_some(locked.map(|locked| locked.id()), |this, vault| {
                        this.p_3()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_2()
                            .child(div().text_color(theme.muted).child(strings::VAULT_LOCKED))
                            .child(
                                button("unlock-vault", strings::UNLOCK_VAULT, theme)
                                    .bg(theme.accent)
                                    .text_color(theme.accent_text)
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(UnlockLockedVault(vault))
                                    })),
                            )
                    })
                    .children(rows),
            )
    }

    fn render_detail(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let pane = div()
            .id("entry-detail")
            .flex_1()
            // Long values wrap rather than widen the pane past the window.
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .bg(theme.panel)
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.error).child(error))
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(div().text_sm().text_color(theme.muted).child(notice))
            });

        match &self.detail {
            Detail::Editing { editor, .. } => pane.child(editor.clone()).into_any_element(),
            Detail::Totp { setup, .. } => pane.child(setup.clone()).into_any_element(),
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
        let Some((vault, entry)) = workspace
            .view()
            .entry
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
                        .child(
                            tool_button("totp-entry", strings::TOTP_BUTTON, theme).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.open_totp_setup(vault, id, window, cx)
                                }),
                            ),
                        )
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
        .when_some(workspace.totp_code(vault, id), |this, code| {
            this.child(match code {
                Ok(code) => {
                    let copied = self.totp_copied == Some((vault, id));
                    self.render_totp(code, copied, theme, cx)
                }
                Err(error) => div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted)
                            .child(strings::DETAIL_TOTP),
                    )
                    .child(div().text_color(theme.error).child(
                        strings::TOTP_UNREADABLE.replace("{reason}", totp_setup::describe(error)),
                    ))
                    .into_any_element(),
            })
        })
        .child(field(strings::DETAIL_URL, entry.url().to_owned().into()))
        .child(field(
            strings::DETAIL_NOTES,
            entry.notes().to_owned().into(),
        ))
        .children(
            entry
                .custom_fields()
                .iter()
                .zip(&self.custom_field_focus)
                .enumerate()
                .map(|(index, (custom, focus))| {
                    let revealed = self.revealed_fields.contains(&index);
                    let value: SharedString = if custom.protected && !revealed {
                        strings::HIDDEN_PASSWORD.into()
                    } else {
                        custom.value.expose_secret().to_owned().into()
                    };
                    let link = |id: &'static str, label: &'static str| {
                        div()
                            .id((id, index))
                            .cursor_pointer()
                            .text_color(theme.accent)
                            .child(label)
                    };
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
                        .flex_col()
                        .gap_0p5()
                        .mx_neg_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .focus(|style| style.border_color(theme.accent))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .gap_2()
                                .text_sm()
                                .child(
                                    div()
                                        .text_color(theme.muted)
                                        .truncate()
                                        .child(custom.name.clone()),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .when(custom.protected, |this| {
                                            this.child(
                                                link(
                                                    "reveal-custom-field",
                                                    if revealed {
                                                        strings::HIDE_PASSWORD
                                                    } else {
                                                        strings::REVEAL_PASSWORD
                                                    },
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.toggle_field_reveal(index, cx)
                                                })),
                                            )
                                        })
                                        .child(
                                            link(
                                                "copy-custom-field",
                                                if self.copied_field == Some(index) {
                                                    strings::COPIED
                                                } else {
                                                    strings::COPY
                                                },
                                            )
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.copy_custom_field(index, cx)
                                                }),
                                            ),
                                        ),
                                ),
                        )
                        .child(div().child(value))
                }),
        )
        .into_any_element()
    }

    /// The current TOTP code with its countdown, copied when clicked.
    fn render_totp(
        &self,
        code: TotpCode,
        copied: bool,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let left = code.remaining() as f32 / code.period() as f32;
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(strings::DETAIL_TOTP)
                    .child(div().text_color(theme.accent).child(if copied {
                        strings::TOTP_COPIED
                    } else {
                        strings::TOTP_COPY_HINT
                    })),
            )
            .child(
                div()
                    .id("totp-code")
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.copy_totp(cx)))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(grouped(code.code())),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .h(px(4.))
                            .rounded_full()
                            .bg(theme.border)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(left))
                                    .rounded_full()
                                    .bg(theme.accent),
                            ),
                    )
                    .child(
                        div().text_sm().text_color(theme.muted).child(
                            strings::TOTP_SECONDS_LEFT
                                .replace("{seconds}", &code.remaining().to_string()),
                        ),
                    ),
            )
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
        self.ensure_custom_field_focus(cx);
        div()
            .id("vault-browser")
            .key_context(actions::CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::copy_username))
            .on_action(cx.listener(Self::copy_password))
            .on_action(cx.listener(Self::copy_totp_code))
            .on_action(cx.listener(|this, _: &SaveAll, _, cx| this.save_all(cx)))
            .on_action(cx.listener(|_, _: &OpenVault, _, cx| cx.emit(OpenAnotherVault)))
            .on_action(
                cx.listener(|this, _: &LockAll, _, cx| locking::lock_all(&this.workspace, cx)),
            )
            .size_full()
            .relative()
            .flex()
            .bg(theme.background)
            .text_color(theme.text)
            .child(self.render_sidebar(theme, cx))
            .child(self.render_entry_list(theme, cx))
            .child(self.render_detail(theme, cx))
            .children(self.render_merge_notice(theme, cx))
    }
}

/// Which browsers the native host is registered with, and whether registering failed.
fn browser_hosts_summary(workspace: &Workspace) -> Option<(SharedString, bool)> {
    let summary = match workspace.browser_hosts_status()? {
        Err(_) => return Some((strings::BROWSER_HOSTS_FAILED.into(), true)),
        Ok(browsers) if browsers.is_empty() => strings::BROWSER_HOSTS_NONE.to_owned(),
        Ok(browsers) => {
            let names: Vec<&str> = browsers.iter().map(|browser| browser.name()).collect();
            format!("{}{}", strings::BROWSER_HOSTS_FOR, names.join(", "))
        }
    };
    Some((summary.into(), false))
}

pub fn describe(error: &EditVaultError) -> &'static str {
    match error {
        EditVaultError::Edit(EditError::IntoItself) => strings::ERROR_MOVE_INTO_ITSELF,
        EditVaultError::Edit(EditError::RootGroup) => strings::ERROR_ROOT_GROUP,
        EditVaultError::Edit(EditError::EntryNotFound | EditError::GroupNotFound)
        | EditVaultError::VaultNotOpen => strings::ERROR_NOT_FOUND,
        EditVaultError::Edit(EditError::PairingNameEmpty) => strings::ERROR_PAIRING_NAME_EMPTY,
        EditVaultError::Edit(EditError::PairingNameTaken) => strings::ERROR_PAIRING_NAME_TAKEN,
        EditVaultError::NotPaired => strings::ERROR_NOT_PAIRED,
        EditVaultError::Edit(EditError::InRecycleBin) => strings::ERROR_IN_RECYCLE_BIN,
        EditVaultError::Edit(EditError::FieldNameEmpty) => strings::ERROR_FIELD_NAME_EMPTY,
        EditVaultError::Edit(EditError::FieldNameTaken) => strings::ERROR_FIELD_NAME_TAKEN,
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

pub fn flatten_groups<'a>(groups: &'a [Group], depth: usize, out: &mut Vec<(&'a Group, usize)>) {
    for group in groups {
        out.push((group, depth));
        flatten_groups(group.groups(), depth + 1, out);
    }
}

/// A code split in two halves to read it more easily: `123 456`.
fn grouped(code: &str) -> SharedString {
    if code.len() < 6 {
        return code.to_owned().into();
    }
    let (first, second) = code.split_at(code.len() / 2);
    format!("{first} {second}").into()
}

fn title_of(entry: &Entry) -> SharedString {
    match entry.title() {
        "" => strings::UNTITLED.into(),
        title => title.to_owned().into(),
    }
}
