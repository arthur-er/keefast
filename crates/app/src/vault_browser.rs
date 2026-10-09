use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, Context, DragMoveEvent, EmptyView, Entity, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, MouseButton, Pixels, PromptLevel, ScrollHandle,
    SharedString, Task, Window, anchored, deferred, div, prelude::*, px, relative,
};
use keefast_core::{
    AccessDecision, AttachmentPreview, EditError, Entry, EntryColor, EntryFields, EntryIcon,
    EntryId, EntrySearch, EntrySort, Group, GroupId, TotpCode,
};
use keefast_workspace::{
    CopyError, CopyField, EditVaultError, EntryScope, ListedEntry, PaneWidths, SyncStatus, VaultId,
    View, Workspace,
};
use secrecy::ExposeSecret;

use crate::actions::{
    self, ChangeMasterKey, CopyCustomField, CopyPassword, CopyTotp, CopyUsername, FocusSearch,
    LockAll, NewVault, OpenEntry, OpenSettings, OpenVault, OpenVaultSettings, SaveAll,
    SelectFirstEntry, SelectLastEntry, SelectNextEntry, SelectPreviousEntry, ShowEntryHistory,
    SortByCreated, SortByModified, SortByTitle, SortByUrl, SortByUsername, SyncNow,
    ToggleCommandPalette,
};
use crate::attachments::{self, Preview};
use crate::browser_integration::BrowserIntegration;
use crate::clipboard::AppClipboard;
use crate::entry_editor::{EntryEditor, EntryEditorEvent};
use crate::entry_history::{EntryHistory, EntryHistoryEvent};
use crate::favicons;
use crate::icons::{self, UiIcon};
use crate::locking;
use crate::merge_notice;
use crate::saving;
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::totp_setup::{self, TotpSetup, TotpSetupEvent};
use crate::vault_key::{VaultKeyEvent, VaultKeyForm};
use crate::widgets::{button, tool_button};

/// Emitted when the user asks to open another Vault.
pub struct OpenAnotherVault;

/// Emitted when the user asks to create a new Vault.
pub struct CreateVault;

/// Emitted when the user asks to unlock a locked Vault.
pub struct UnlockLockedVault(pub VaultId);

/// Emitted when the user asks for a Vault's settings, which the settings screen shows.
pub struct ShowVaultSettings(pub VaultId);

/// The KeeWeb-style three panes: Vaults and Groups sidebar → Entry list → Entry detail, over a
/// footer with the Vaults and the app's commands.
pub struct VaultBrowser {
    workspace: Entity<Workspace>,
    focus: FocusHandle,
    /// Narrows the Entry list to the Entries matching what is typed in it.
    search: Entity<TextField>,
    /// What `search` holds, as last searched for.
    query: EntrySearch,
    sort: EntrySort,
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
    /// The attachment of the selected Entry being previewed.
    attachment_preview: Option<Preview>,
    /// What the latest Merges of changes made elsewhere changed, until dismissed or timed out.
    merge_notice: Option<SharedString>,
    /// Counts the notices shown, so a timeout only hides the one it was set for.
    merge_notice_generation: u64,
    /// How wide the sidebar and the Entry list are, as dragged.
    sidebar_width: Pixels,
    entry_list_width: Pixels,
    /// Remembers the pane widths once the user stops dragging them.
    pane_widths_save: Option<Task<()>>,
    /// The Entry list's menu open, if one is.
    menu: Option<ListMenu>,
}

/// What picking a menu's choice does.
type MenuAction = Rc<dyn Fn(&mut VaultBrowser, &mut Window, &mut Context<VaultBrowser>)>;

/// A choice in one of the Entry list's menus.
struct MenuItem {
    label: &'static str,
    checked: bool,
    action: MenuAction,
}

impl MenuItem {
    fn new(
        label: &'static str,
        checked: bool,
        action: impl Fn(&mut VaultBrowser, &mut Window, &mut Context<VaultBrowser>) + 'static,
    ) -> Self {
        Self {
            label,
            checked,
            action: Rc::new(action),
        }
    }
}

/// The menus of the Entry list's buttons.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListMenu {
    /// What to add: an Entry or a Group.
    Add,
    Sort,
}

/// The edge between two panes, dragged to resize the pane on its left.
#[derive(Clone, Copy)]
enum PaneEdge {
    Sidebar,
    EntryList,
}

/// How narrow and wide the sidebar and Entry list can be dragged, and how much the Entry detail
/// keeps of the window.
const SIDEBAR_WIDTHS: (f32, f32) = (160., 480.);
const ENTRY_LIST_WIDTHS: (f32, f32) = (220., 720.);
const DETAIL_MIN_WIDTH: f32 = 320.;
/// How wide the grab area of a pane edge is, centred on the edge.
const PANE_EDGE_GRAB: f32 = 6.;
/// How long after the last drag of a pane edge the widths are remembered.
const PANE_WIDTHS_SAVE_DELAY: Duration = Duration::from_millis(500);

/// How wide the column of labels in the Entry detail is.
const DETAIL_LABEL_WIDTH: Pixels = px(90.);

/// The standard icons of a key, drawn for a Vault, and of a folder, drawn for a Group.
const KEY_ICON: u32 = 0;
const FOLDER_ICON: u32 = 48;

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
    /// An Entry's earlier versions.
    History {
        view: Entity<EntryHistory>,
        vault: VaultId,
        entry: EntryId,
    },
    /// Setting up an Entry's TOTP.
    Totp {
        setup: Entity<TotpSetup>,
        vault: VaultId,
        entry: EntryId,
    },
    /// What can be done with a Group: renaming, moving and deleting it, and more.
    Group(VaultId, GroupId),
    /// Changing a Vault's master key and encryption.
    VaultKey {
        form: Entity<VaultKeyForm>,
        vault: VaultId,
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
impl EventEmitter<CreateVault> for VaultBrowser {}
impl EventEmitter<ShowVaultSettings> for VaultBrowser {}

impl Focusable for VaultBrowser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl VaultBrowser {
    pub fn new(
        workspace: Entity<Workspace>,
        integration: Entity<BrowserIntegration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&workspace, |this, workspace, cx| {
            this.show_merges(&workspace, cx);
            this.leave_locked_vaults(cx);
            cx.notify();
        })
        .detach();
        cx.observe(&integration, |_, _, cx| cx.notify()).detach();
        // Keeps the shown TOTP code and its countdown current, and flags Entries as they
        // expire within a minute.
        cx.spawn(async move |this, cx| {
            for second in 1u64.. {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let shown = this.update(cx, |this, cx| {
                    if this.shows_totp(cx) || second % 60 == 0 {
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
        let search =
            cx.new(|cx| TextField::new(strings::SEARCH_PLACEHOLDER, cx).with_icon(UiIcon::Search));
        cx.observe(&search, Self::on_search_changed).detach();
        cx.subscribe_in(&search, window, Self::on_search_event)
            .detach();
        let widths = workspace.read(cx).pane_widths();
        Self {
            workspace,
            focus: cx.focus_handle(),
            search,
            query: EntrySearch::default(),
            sort: EntrySort::default(),
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
            attachment_preview: None,
            merge_notice: None,
            merge_notice_generation: 0,
            sidebar_width: px(widths.sidebar as f32),
            entry_list_width: px(widths.entry_list as f32),
            pane_widths_save: None,
            menu: None,
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
            Detail::Totp { vault, .. } | Detail::VaultKey { vault, .. } => Some(*vault),
            Detail::History { vault, .. } => Some(*vault),
            Detail::Group(vault, _) => Some(*vault),
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
        // An attachment's preview shows decrypted contents, so it goes with its Vault.
        if self.attachment_preview.is_some()
            && self
                .selected_entry(cx)
                .is_none_or(|(vault, _)| locked(vault))
        {
            self.attachment_preview = None;
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

    /// The Entry list: the Entries in scope matching the search, in the chosen order.
    fn listed_entries<'a>(&self, workspace: &'a Workspace) -> Vec<ListedEntry<'a>> {
        workspace.find_entries(workspace.view().scope, &self.query, self.sort)
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search.focus_handle(cx));
    }

    /// A new search selects its best match, unless an Entry is being edited or set up.
    fn on_search_changed(&mut self, search: Entity<TextField>, cx: &mut Context<Self>) {
        let query = EntrySearch::new(search.read(cx).text());
        if query == self.query {
            return;
        }
        self.query = query;
        if matches!(self.detail, Detail::Entry) {
            let workspace = self.workspace.read(cx);
            let best = self
                .listed_entries(workspace)
                .first()
                .map(|listed| (listed.vault, listed.entry.id()));
            match best {
                Some((vault, entry)) => self.select_entry(vault, entry, cx),
                None => {
                    let scope = self.scope(cx);
                    self.show(View { scope, entry: None }, cx);
                }
            }
            self.entry_list_scroll.scroll_to_item(0);
        }
        cx.notify();
    }

    /// Enter in the search field goes to the Entry list, with the best match selected unless
    /// a listed Entry is; Escape clears the search.
    fn on_search_event(
        &mut self,
        _: &Entity<TextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit => {
                self.select_in_list(|current, _| current.unwrap_or(0), cx);
                window.focus(&self.focus);
            }
            TextFieldEvent::Cancel => {
                if self.search.read(cx).text().is_empty() {
                    window.focus(&self.focus);
                }
                self.search.update(cx, |search, cx| search.set_text("", cx));
            }
        }
    }

    fn sort_by(&mut self, sort: EntrySort, cx: &mut Context<Self>) {
        self.sort = sort;
        if let Some(ix) = self.selected_position(cx) {
            self.entry_list_scroll.scroll_to_item(ix);
        }
        cx.notify();
    }

    /// Where the selected Entry is in the Entry list, if it is listed.
    fn selected_position(&self, cx: &App) -> Option<usize> {
        let workspace = self.workspace.read(cx);
        self.listed_entries(workspace)
            .iter()
            .position(|listed| Some((listed.vault, listed.entry.id())) == self.selected_entry(cx))
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
        self.attachment_preview = None;
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
        let View { entry, .. } = workspace.view();
        let entries = self.listed_entries(workspace);
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

    fn show_history(&mut self, _: &ShowEntryHistory, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((vault, entry)) = self.selected_entry(cx) {
            self.open_history(vault, entry, window, cx);
        }
    }

    fn open_history(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let history = cx.new(|cx| EntryHistory::new(workspace, vault, entry, cx));
        cx.subscribe_in(&history, window, Self::on_history_event)
            .detach();
        window.focus(&history.focus_handle(cx));
        self.detail = Detail::History {
            view: history,
            vault,
            entry,
        };
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    /// Asks for the settings of the Vault shown, or of the selected Entry's, or of the first
    /// Vault.
    fn open_settings(&mut self, _: &OpenVaultSettings, _: &mut Window, cx: &mut Context<Self>) {
        let vault = self.scope(cx).vault().or_else(|| {
            self.selected_entry(cx).map(|(vault, _)| vault).or_else(|| {
                self.workspace
                    .read(cx)
                    .vaults()
                    .next()
                    .map(|open| open.id())
            })
        });
        if let Some(vault) = vault {
            cx.emit(ShowVaultSettings(vault));
        }
    }

    fn on_history_event(
        &mut self,
        _: &Entity<EntryHistory>,
        event: &EntryHistoryEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Detail::History { vault, entry, .. } = self.detail else {
            return;
        };
        match event {
            EntryHistoryEvent::Restore(version) => {
                let restored = self.edit(cx, |workspace| {
                    workspace.restore_entry_version(vault, entry, *version)
                });
                if restored.is_none() {
                    return;
                }
                self.detail = Detail::Entry;
                self.notice = Some(strings::HISTORY_RESTORED.into());
                self.revealed_fields.clear();
                self.copied_field = None;
            }
            EntryHistoryEvent::Close => self.detail = Detail::Entry,
        }
        window.focus(&self.focus);
        cx.notify();
    }

    /// Forgets the Access decision remembered on the Entry for `host`, or for every host.
    fn forget_access(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        host: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.edit(cx, |workspace| {
            workspace.forget_access(vault, entry, host.as_ref().map(SharedString::as_ref))
        });
    }

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

    /// An attachment of the selected Entry, by index.
    fn selected_attachment(&self, index: usize, cx: &App) -> Option<keefast_core::Attachment> {
        let workspace = self.workspace.read(cx);
        let (vault, id) = self.selected_entry(cx)?;
        workspace
            .entry(vault, id)?
            .attachments()
            .get(index)
            .cloned()
    }

    /// Shows a preview of an attachment of the selected Entry, or hides the one shown.
    fn toggle_attachment_preview(&mut self, index: usize, cx: &mut Context<Self>) {
        let attachment = self.selected_attachment(index, cx);
        let shown = self
            .attachment_preview
            .as_ref()
            .zip(attachment.as_ref())
            .is_some_and(|(preview, attachment)| preview.shows(index, attachment));
        self.attachment_preview = attachment
            .filter(|_| !shown)
            .map(|attachment| Preview::new(index, &attachment));
        cx.notify();
    }

    /// Asks where to save an attachment of the selected Entry, and saves it there.
    fn save_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(attachment) = self.selected_attachment(index, cx) else {
            return;
        };
        let saving = attachments::save_to_disk(attachment, cx);
        cx.spawn(async move |this, cx| {
            let Some(result) = saving.await else {
                return;
            };
            this.update(cx, |this, cx| {
                match result {
                    Ok(notice) => {
                        this.notice = Some(notice.into());
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        let (EditTarget::Existing(vault, _) | EditTarget::New(vault, _)) = target;
        let fields = match &target {
            EditTarget::Existing(vault, id) => self
                .workspace
                .read(cx)
                .entry(*vault, *id)
                .map(EntryFields::from)
                .unwrap_or_default(),
            // An Entry made while a tag or colour is shown has it, so it shows up there.
            EditTarget::New(..) => {
                let scope = self.scope(cx);
                EntryFields {
                    tags: match &scope {
                        EntryScope::Tag(tag) => vec![tag.clone()],
                        _ => Vec::new(),
                    },
                    color: match scope {
                        EntryScope::Color(color) => Some(color),
                        _ => None,
                    },
                    ..EntryFields::default()
                }
            }
        };
        let editor = cx.new(|cx| EntryEditor::new(&fields, vault, self.workspace.clone(), cx));
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
                    self.attachment_preview = None;
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

    /// Opens the form for the master key and encryption of `vault`, or else of the Vault
    /// shown, in the detail pane.
    pub fn open_vault_key(
        &mut self,
        vault: Option<VaultId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(vault) = vault.or_else(|| self.target_group(cx).map(|(vault, _)| vault)) else {
            return;
        };
        let form = cx.new(|cx| VaultKeyForm::for_vault(self.workspace.clone(), vault, cx));
        cx.subscribe_in(&form, window, |this, _, _: &VaultKeyEvent, window, cx| {
            this.detail = Detail::Entry;
            window.focus(&this.focus);
            cx.notify();
        })
        .detach();
        window.focus(&form.focus_handle(cx));
        self.detail = Detail::VaultKey { form, vault };
        self.error = None;
        cx.notify();
    }

    /// Saves every Vault with unsaved changes.
    fn save_all(&mut self, cx: &mut Context<Self>) {
        saving::save_all(&self.workspace, cx);
    }

    fn start_autosaves(&mut self, cx: &mut Context<Self>) {
        saving::autosave(&self.workspace, cx);
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

    /// Resizes the pane left of the dragged edge to follow the mouse, leaving the panes right
    /// of it room to show.
    fn on_pane_edge_drag(
        &mut self,
        event: &DragMoveEvent<PaneEdge>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(event.event.position.x - event.bounds.left());
        let width = f32::from(event.bounds.size.width);
        let sidebar = f32::from(self.sidebar_width);
        let entry_list = f32::from(self.entry_list_width);
        match *event.drag(cx) {
            PaneEdge::Sidebar => {
                let room = width - entry_list - DETAIL_MIN_WIDTH;
                self.sidebar_width = px(clamp(x, SIDEBAR_WIDTHS, room));
            }
            PaneEdge::EntryList => {
                let room = width - sidebar - DETAIL_MIN_WIDTH;
                self.entry_list_width = px(clamp(x - sidebar, ENTRY_LIST_WIDTHS, room));
            }
        }
        self.save_pane_widths_soon(cx);
        cx.notify();
    }

    /// Puts a pane back to its default width.
    fn reset_pane_width(&mut self, edge: PaneEdge, cx: &mut Context<Self>) {
        let defaults = PaneWidths::default();
        match edge {
            PaneEdge::Sidebar => self.sidebar_width = px(defaults.sidebar as f32),
            PaneEdge::EntryList => self.entry_list_width = px(defaults.entry_list as f32),
        }
        self.save_pane_widths_soon(cx);
        cx.notify();
    }

    /// Remembers the pane widths in the app config once they have stopped changing, so a drag
    /// doesn't write the config at every step.
    fn save_pane_widths_soon(&mut self, cx: &mut Context<Self>) {
        self.pane_widths_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PANE_WIDTHS_SAVE_DELAY).await;
            this.update(cx, |this, cx| {
                let widths = PaneWidths {
                    sidebar: f32::from(this.sidebar_width).round() as u32,
                    entry_list: f32::from(this.entry_list_width).round() as u32,
                };
                this.workspace
                    .update(cx, |workspace, _| workspace.set_pane_widths(widths));
            })
            .ok();
        }));
    }

    /// The grab area of the edge right of a pane, which ends `right` from the window's left.
    fn render_pane_edge(
        &self,
        edge: PaneEdge,
        right: Pixels,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = match edge {
            PaneEdge::Sidebar => "sidebar-edge",
            PaneEdge::EntryList => "entry-list-edge",
        };
        div()
            .id(id)
            .absolute()
            .top_0()
            .bottom_0()
            .left(right - px(PANE_EDGE_GRAB / 2.))
            .w(px(PANE_EDGE_GRAB))
            .cursor_ew_resize()
            .hover(|this| this.bg(theme.border))
            .on_drag(edge, |_, _, _, cx| cx.new(|_| EmptyView))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() == 2 {
                    this.reset_pane_width(edge, cx);
                }
            }))
    }

    /// KeeWeb's menu: All Items, Colours and Tags, then each Vault's Groups, and the Trash.
    fn render_sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let scope = workspace.view().scope;
        let icon =
            |icon: UiIcon, color| Some(icons::ui_icon(icon, px(14.), color).into_any_element());
        let folder = |color| Some(icons::standard_icon(FOLDER_ICON, px(14.), color));
        let mut rows: Vec<AnyElement> = Vec::new();

        rows.push(
            sidebar_row(
                ("all-items", 0),
                strings::SIDEBAR_ALL_ITEMS.into(),
                0,
                scope == EntryScope::AllVaults,
                icon(UiIcon::AllItems, theme.text),
                theme,
            )
            .on_click(cx.listener(|this, _, _, cx| this.select(EntryScope::AllVaults, cx)))
            .into_any_element(),
        );

        rows.push(sidebar_separator(theme));
        rows.push(sidebar_heading(
            strings::SIDEBAR_COLORS,
            UiIcon::Colors,
            theme,
        ));
        let dots = EntryColor::ALL.into_iter().map(|color| {
            let selected = scope == EntryScope::Color(color);
            div()
                .id(("color", color as usize))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .when(selected, |this| this.bg(theme.sidebar_selection))
                .child(icons::color_dot(color, px(12.)))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.select(EntryScope::Color(color), cx)),
                )
        });
        rows.push(
            div()
                .flex()
                .gap_1()
                .pl(px(30.))
                .pb_1()
                .children(dots)
                .into_any_element(),
        );

        rows.push(sidebar_separator(theme));
        rows.push(sidebar_heading(strings::SIDEBAR_TAGS, UiIcon::Tags, theme));
        for tag in workspace.tags() {
            let tag_scope = EntryScope::Tag(tag.to_owned());
            let selected = scope == tag_scope;
            rows.push(
                sidebar_row(
                    ("tag", rows.len()),
                    tag.to_owned().into(),
                    1,
                    selected,
                    None,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select(tag_scope.clone(), cx)))
                .into_any_element(),
            );
        }

        // Unlocked and locked Vaults, in the order they were opened.
        let mut vaults: Vec<VaultId> = workspace
            .vaults()
            .map(|open| open.id())
            .chain(workspace.locked_vaults().map(|locked| locked.id()))
            .collect();
        vaults.sort();
        let mut bins = Vec::new();
        for vault in vaults {
            rows.push(sidebar_separator(theme));
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
                        name.into(),
                        0,
                        selected,
                        icon(UiIcon::Lock, theme.muted),
                        theme,
                    )
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
                        icon(UiIcon::Lock, theme.muted),
                        theme,
                    )
                    .text_color(theme.muted)
                    .into_any_element(),
                );
                continue;
            }
            rows.push(
                sidebar_row(
                    ("vault", rows.len()),
                    open.display_name().into(),
                    0,
                    selected,
                    Some(icons::standard_icon(KEY_ICON, px(14.), theme.text)),
                    theme,
                )
                .when(selected, |this| {
                    this.child(edit_mark(("vault-settings", rows.len()), theme).on_click(
                        cx.listener(move |_, _, _, cx| {
                            // Not the row's click as well.
                            cx.stop_propagation();
                            cx.emit(ShowVaultSettings(vault))
                        }),
                    ))
                })
                .on_click(select)
                .into_any_element(),
            );
            let bin = open.vault().recycle_bin();
            bins.extend(bin.map(|bin| (vault, bin, open.display_name())));
            let mut groups = Vec::new();
            flatten_groups(open.vault().root().groups(), 1, &mut groups);
            let mut in_bin_below = None;
            for (group, depth) in groups {
                let id = group.id();
                // The Trash and what it holds are at the bottom of the sidebar instead.
                if in_bin_below.is_some_and(|bin_depth| depth > bin_depth) {
                    continue;
                }
                in_bin_below = None;
                if Some(id) == bin {
                    in_bin_below = Some(depth);
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
                        folder(theme.text),
                        theme,
                    )
                    .when(selected, |this| {
                        this.child(edit_mark(("group-actions", rows.len()), theme).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.detail = Detail::Group(vault, id);
                                cx.notify();
                            }),
                        ))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select(EntryScope::Group(vault, id), cx)
                    }))
                    .into_any_element(),
                );
            }
        }

        let several_bins = bins.len() > 1;
        let any_bin = !bins.is_empty();
        let trash =
            bins.into_iter()
                .enumerate()
                .map(|(ix, (vault, bin, name))| {
                    let label = match several_bins {
                        true => strings::SIDEBAR_TRASH_OF.replace("{vault}", &name),
                        false => strings::SIDEBAR_TRASH.to_owned(),
                    };
                    sidebar_row(
                        ("trash", ix),
                        label.into(),
                        0,
                        scope == EntryScope::Group(vault, bin),
                        icon(UiIcon::Trash, theme.text),
                        theme,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select(EntryScope::Group(vault, bin), cx)
                    }))
                });

        div()
            .w(self.sidebar_width)
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("sidebar")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .py_1p5()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .when(any_bin, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .py_1p5()
                        .border_t_1()
                        .border_color(theme.border)
                        .children(trash),
                )
            })
    }

    /// KeeWeb's footer: the Vaults, each opening its settings, and the app's commands.
    pub fn render_footer(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let mut vaults: Vec<(VaultId, bool)> = workspace
            .vaults()
            .map(|open| (open.id(), true))
            .chain(workspace.locked_vaults().map(|locked| (locked.id(), false)))
            .collect();
        vaults.sort();
        let items = vaults
            .into_iter()
            .enumerate()
            .map(|(ix, (vault, unlocked))| {
                let item = div()
                    .id(("footer-vault", ix))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|this| this.bg(theme.sidebar_selection));
                let Some(open) = workspace.vault(vault).filter(|_| unlocked) else {
                    let name = workspace
                        .locked_vault(vault)
                        .map(|locked| locked.display_name())
                        .unwrap_or_default();
                    return item
                        .text_color(theme.muted)
                        .child(icons::ui_icon(UiIcon::Lock, px(14.), theme.muted))
                        .child(name)
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(UnlockLockedVault(vault))))
                        .into_any_element();
                };
                let name = match open.has_unsaved_changes() {
                    true => format!("{}{}", open.display_name(), strings::UNSAVED_MARKER),
                    false => open.display_name(),
                };
                let status = if open.is_saving() {
                    Some((strings::STATUS_SAVING, theme.muted))
                } else if open.save_failed() {
                    Some((strings::STATUS_SAVE_FAILED, theme.error))
                } else {
                    open.sync_status().map(|status| match status {
                        SyncStatus::Synced => (strings::SYNC_SYNCED, theme.muted),
                        SyncStatus::Syncing => (strings::SYNC_SYNCING, theme.muted),
                        SyncStatus::Offline => (strings::SYNC_OFFLINE, theme.muted),
                        SyncStatus::Error => (strings::SYNC_ERROR, theme.error),
                    })
                };
                item.child(icons::ui_icon(UiIcon::Unlocked, px(14.), theme.text))
                    .child(name)
                    .children(
                        status
                            .map(|(status, color)| div().text_xs().text_color(color).child(status)),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(ShowVaultSettings(vault))))
                    .into_any_element()
            });
        let can_lock = workspace.vaults().next().is_some();
        let command = |id: &'static str, icon: UiIcon, color| {
            div()
                .id(id)
                .p_1p5()
                .rounded_md()
                .cursor_pointer()
                .hover(|this| this.bg(theme.sidebar_selection))
                .child(icons::ui_icon(icon, px(16.), color))
        };

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .text_sm()
            .bg(theme.sidebar)
            .text_color(theme.text)
            .border_t_1()
            .border_color(theme.border)
            .children(items)
            .child(
                div()
                    .id("footer-open")
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(theme.muted)
                    .hover(|this| this.bg(theme.sidebar_selection))
                    .child(icons::ui_icon(UiIcon::Open, px(14.), theme.muted))
                    .child(strings::FOOTER_OPEN)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenAnotherVault))),
            )
            .child(div().flex_1())
            .child(
                command("footer-help", UiIcon::Help, theme.text).on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(ToggleCommandPalette), cx)
                }),
            )
            .child(
                command("footer-settings", UiIcon::Settings, theme.text)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
            )
            .when(can_lock, |this| {
                this.child(
                    command("footer-lock", UiIcon::LockAll, theme.text).on_click(
                        cx.listener(|this, _, _, cx| locking::lock_all(&this.workspace, cx)),
                    ),
                )
            })
    }

    /// A menu under the button it belongs to, drawn over the panes.
    fn render_menu(
        &self,
        items: Vec<MenuItem>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = items.into_iter().enumerate().map(|(ix, item)| {
            let action = item.action;
            div()
                .id(("menu-item", ix))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|this| this.bg(theme.sidebar_selection))
                .child(
                    div()
                        .w(px(12.))
                        .when(item.checked, |this| this.child(strings::MENU_CHECKED)),
                )
                .child(item.label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.menu = None;
                    action(this, window, cx);
                    cx.notify();
                }))
        });
        // Anchored where this empty box sits, under the button: an anchored element alone would
        // sit at the top of its parent.
        div()
            .child(
                deferred(
                    anchored().snap_to_window_with_margin(px(8.)).child(
                        div()
                            .id("list-menu")
                            .occlude()
                            .mt_1()
                            .min_w(px(160.))
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.panel)
                            .text_color(theme.text)
                            .text_sm()
                            .shadow_md()
                            .children(rows),
                    ),
                )
                .with_priority(1),
            )
            .into_any_element()
    }

    /// What can be done with a Group, from its pencil in the sidebar.
    fn render_group(
        &self,
        vault: VaultId,
        group: GroupId,
        theme: Theme,
        cx: &mut Context<Self>,
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
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icons::standard_icon(FOLDER_ICON, px(32.), theme.text))
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .truncate()
                            .child(found.name().to_owned()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        tool_button("group-new-entry", strings::NEW_ENTRY, theme).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open_editor(EditTarget::New(vault, group), window, cx)
                            }),
                        ),
                    )
                    .child(
                        tool_button("group-new-group", strings::NEW_GROUP, theme).on_click(
                            cx.listener(|this, _, window, cx| this.new_group(window, cx)),
                        ),
                    )
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
                            cx.listener(move |this, _, window, cx| {
                                this.delete(subject, window, cx)
                            }),
                        ),
                    ),
            )
            .when(!in_bin, |this| {
                this.child(match is_browser_group {
                    true => div()
                        .text_sm()
                        .text_color(theme.muted)
                        .child(strings::BROWSER_GROUP_MARK)
                        .into_any_element(),
                    false => div()
                        .flex()
                        .child(
                            tool_button("browser-group", strings::USE_AS_BROWSER_GROUP, theme)
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
                button("close-group", strings::EDITOR_DONE, theme).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.detail = Detail::Entry;
                        window.focus(&this.focus);
                        cx.notify();
                    },
                )),
            ))
            .into_any_element()
    }

    fn render_entry_list(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        // Built first, as they need `cx` the rest borrows.
        let adding = self.target_group(cx);
        let icon_button = |id: &'static str, icon: UiIcon, open: bool| {
            div()
                .id(id)
                .flex_none()
                .p_1p5()
                .rounded_md()
                .cursor_pointer()
                .when(open, |this| this.bg(theme.sidebar_selection))
                .hover(|this| this.bg(theme.sidebar_selection))
                .child(icons::ui_icon(icon, px(16.), theme.text))
        };
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
        let View {
            scope,
            entry: selected_entry,
        } = workspace.view();
        let entries = self.listed_entries(workspace);
        let searching = !self.query.is_empty();
        let locked = match &scope {
            EntryScope::Vault(vault) => workspace.locked_vault(*vault),
            _ => None,
        };
        // The Vault shown is waiting to lock, so the list has nothing in it.
        let locking = match &scope {
            EntryScope::Vault(vault) | EntryScope::Group(vault, _) => workspace
                .vault(*vault)
                .is_some_and(|open| open.is_locking()),
            _ => false,
        };
        // Across several Vaults, each Entry says which Vault it is from.
        let label_vaults = scope.vault().is_none() && workspace.vaults().nth(1).is_some();

        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(ix, listed)| {
                let (vault, entry) = (listed.vault, listed.entry);
                let id = entry.id();
                let selected = selected_entry == Some((vault, id));
                let open = workspace.vault(vault);
                let vault_name = label_vaults
                    .then(|| open.map(|open| open.display_name()))
                    .flatten();
                let expired = workspace.is_expired(entry);
                let tags = (!entry.tags().is_empty()).then(|| entry.tags().join(", "));
                // The picked Entry is drawn all in the selection's text colour.
                let muted = if selected {
                    theme.selection_text
                } else {
                    theme.muted
                };
                div()
                    .id(("entry", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .mx_1p5()
                    .my_0p5()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |this| {
                        this.bg(theme.selection).text_color(theme.selection_text)
                    })
                    .children(entry_mark(workspace, vault, entry, px(18.), muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .when(expired, |this| {
                                                this.line_through().text_color(theme.muted)
                                            })
                                            .child(title_of(entry)),
                                    )
                                    .when(expired, |this| this.child(expired_badge(theme)))
                                    .when_some(entry.color(), |this, color| {
                                        this.child(icons::color_dot(color, px(8.)))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .child(entry.username().to_owned()),
                                    )
                                    .when_some(tags, |this, tags| {
                                        this.child(
                                            div()
                                                .flex_none()
                                                .max_w(px(120.))
                                                .truncate()
                                                .text_xs()
                                                .when(!selected, |this| {
                                                    this.text_color(theme.accent)
                                                })
                                                .child(tags),
                                        )
                                    })
                                    .when_some(vault_name, |this, name| {
                                        this.child(div().flex_none().text_xs().child(name))
                                    }),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // So the keyboard shortcuts act on the Entry just picked.
                        window.focus(&this.focus);
                        this.select_entry(vault, id, cx)
                    }))
                    .into_any_element()
            })
            .collect();

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
                    .gap_1()
                    .px_2()
                    .py_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .key_context(actions::SEARCH_CONTEXT)
                            .child(self.search.clone()),
                    )
                    .when(adding.is_some(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    icon_button("add", UiIcon::Open, add_menu.is_some()).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.menu = Some(ListMenu::Add);
                                            cx.notify();
                                        }),
                                    ),
                                )
                                .children(add_menu),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                icon_button("sort", UiIcon::Sort, sort_menu.is_some()).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.menu = Some(ListMenu::Sort);
                                        cx.notify();
                                    }),
                                ),
                            )
                            .children(sort_menu),
                    ),
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
                        } else if searching {
                            strings::LIST_NO_MATCHES
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
            .bg(theme.background)
            .flex()
            .flex_col()
            .gap_3()
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
            Detail::History { view, .. } => pane.child(view.clone()).into_any_element(),
            Detail::Group(vault, group) => pane
                .child(self.render_group(*vault, *group, theme, cx))
                .into_any_element(),
            Detail::VaultKey { form, .. } => pane.child(form.clone()).into_any_element(),
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

        // KeeWeb's way: each label right-aligned in a column before its value.
        let labelled = |label: &'static str| {
            div().flex().gap_4().child(
                div()
                    .w(DETAIL_LABEL_WIDTH)
                    .flex_none()
                    .text_right()
                    .text_color(theme.muted)
                    .child(label),
            )
        };
        let field = |label: &'static str, value: SharedString| {
            labelled(label).child(div().flex_1().min_w_0().child(value))
        };
        let expired = workspace.is_expired(entry);
        let icon = entry_mark(workspace, vault, entry, px(32.), theme.text);

        pane.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .min_w_0()
                        .children(icon)
                        .child(
                            div()
                                .text_3xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .truncate()
                                .child(title_of(entry)),
                        )
                        .when_some(entry.color(), |this, color| {
                            this.child(icons::color_dot(color, px(10.)))
                        }),
                )
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
                        .child(
                            tool_button("history-entry", strings::HISTORY_BUTTON, theme).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.open_history(vault, id, window, cx)
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
        .when(!entry.tags().is_empty(), |this| {
            let tags = entry.tags().iter().enumerate().map(|(ix, tag)| {
                let scope = EntryScope::Tag(tag.clone());
                div()
                    .id(("detail-tag", ix))
                    .px_2()
                    .rounded_full()
                    .bg(theme.sidebar_selection)
                    .text_sm()
                    .cursor_pointer()
                    .child(tag.clone())
                    .on_click(cx.listener(move |this, _, _, cx| this.select(scope.clone(), cx)))
            });
            this.child(
                labelled(strings::DETAIL_TAGS)
                    .child(div().flex_1().flex().flex_wrap().gap_1().children(tags)),
            )
        })
        .when_some(entry.expires(), |this, expires| {
            this.child(
                labelled(strings::DETAIL_EXPIRES).child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(expired, |this| this.text_color(theme.error))
                        .child(expires.to_date())
                        .when(expired, |this| this.child(expired_badge(theme))),
                ),
            )
        })
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
        .when_some(
            self.render_access(vault, entry, theme, cx),
            |this, access| this.child(access),
        )
        .when(!entry.attachments().is_empty(), |this| {
            this.child(self.render_attachments(entry, theme, cx))
        })
        .into_any_element()
    }

    /// The Access decisions remembered on the Entry, each of which can be forgotten, if it has
    /// any.
    fn render_access(
        &self,
        vault: VaultId,
        entry: &Entry,
        theme: Theme,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let remembered = entry.remembered_access();
        if remembered.is_empty() {
            return None;
        }
        let id = entry.id();
        let link = |element_id: (&'static str, usize), label: &'static str| {
            div()
                .id(element_id)
                .cursor_pointer()
                .text_sm()
                .text_color(theme.accent)
                .child(label)
        };
        let rows = remembered.into_iter().enumerate().map(|(ix, access)| {
            let host: SharedString = access.host.into();
            let (decision, color) = match access.decision {
                AccessDecision::Allow => (strings::ACCESS_ALLOWED, theme.muted),
                AccessDecision::Deny => (strings::ACCESS_DENIED, theme.error),
            };
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().flex_1().truncate().child(host.clone()))
                .child(div().text_sm().text_color(color).child(decision))
                .child(
                    link(("forget-access", ix), strings::ACCESS_FORGET).on_click(cx.listener(
                        move |this, _, _, cx| this.forget_access(vault, id, Some(host.clone()), cx),
                    )),
                )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted)
                                .child(strings::DETAIL_BROWSER_ACCESS),
                        )
                        .child(
                            link(("forget-all-access", 0), strings::ACCESS_FORGET_ALL).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.forget_access(vault, id, None, cx)
                                }),
                            ),
                        ),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    /// The selected Entry's attachments, each with its size, a preview when it is text or an
    /// image, and a way to save it to disk.
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
                let link = |id: &'static str, label: &'static str| {
                    div()
                        .id((id, index))
                        .cursor_pointer()
                        .text_color(theme.accent)
                        .child(label)
                };
                let previewable = attachment.preview() != AttachmentPreview::None;
                let links = div()
                    .flex()
                    .gap_3()
                    .when(previewable, |this| {
                        let label = if preview.is_some() {
                            strings::HIDE_PREVIEW
                        } else {
                            strings::PREVIEW_ATTACHMENT
                        };
                        this.child(link("preview-attachment", label).on_click(cx.listener(
                            move |this, _, _, cx| this.toggle_attachment_preview(index, cx),
                        )))
                    })
                    .child(link("save-attachment", strings::SAVE_ATTACHMENT).on_click(
                        cx.listener(move |this, _, _, cx| this.save_attachment(index, cx)),
                    ));
                div()
                    .flex()
                    .flex_col()
                    .child(attachments::row(attachment, links, theme))
                    .when_some(preview, |this, preview| this.child(preview.render(theme)))
            });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(strings::ATTACHMENTS),
            )
            .children(rows)
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
                sidebar_row(("move-target", ix), name, depth, false, None, theme)
                    .rounded_md()
                    .hover(|this| this.bg(theme.sidebar_selection))
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
        let theme = Theme::for_window(window, cx);
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
            .on_action(cx.listener(Self::show_history))
            .on_action(cx.listener(|this, _: &SaveAll, _, cx| this.save_all(cx)))
            .on_action(cx.listener(|_, _: &OpenVault, _, cx| cx.emit(OpenAnotherVault)))
            .on_action(cx.listener(|_, _: &NewVault, _, cx| cx.emit(CreateVault)))
            .on_action(cx.listener(|this, _: &ChangeMasterKey, window, cx| {
                this.open_vault_key(None, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &LockAll, _, cx| locking::lock_all(&this.workspace, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SyncNow, _, cx| saving::sync_now(&this.workspace, cx)),
            )
            .on_action(cx.listener(Self::focus_search))
            .on_action(
                cx.listener(|this, _: &SortByTitle, _, cx| this.sort_by(EntrySort::Title, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SortByUsername, _, cx| {
                    this.sort_by(EntrySort::Username, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SortByUrl, _, cx| this.sort_by(EntrySort::Url, cx)))
            .on_action(
                cx.listener(|this, _: &SortByCreated, _, cx| this.sort_by(EntrySort::Created, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SortByModified, _, cx| {
                    this.sort_by(EntrySort::Modified, cx)
                }),
            )
            .on_action(cx.listener(Self::open_settings))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.text)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .on_drag_move(cx.listener(Self::on_pane_edge_drag))
                    .child(self.render_sidebar(theme, cx))
                    .child(self.render_entry_list(theme, cx))
                    .child(self.render_detail(theme, cx))
                    // After the panes, so the edges are grabbed over the panes either side of
                    // them.
                    .child(self.render_pane_edge(PaneEdge::Sidebar, self.sidebar_width, theme, cx))
                    .child(self.render_pane_edge(
                        PaneEdge::EntryList,
                        self.sidebar_width + self.entry_list_width,
                        theme,
                        cx,
                    ))
                    .children(self.render_merge_notice(theme, cx)),
            )
            .child(self.render_footer(theme, cx))
            .when(self.menu.is_some(), |this| {
                // Clicking anywhere beside an open menu closes it.
                this.child(
                    div()
                        .id("menu-backdrop")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            }),
                        ),
                )
            })
    }
}

/// `width` within `(min, max)` and no wider than `room`, though never under `min`.
fn clamp(width: f32, (min, max): (f32, f32), room: f32) -> f32 {
    width.min(max).min(room).max(min)
}

pub fn describe(error: &EditVaultError) -> &'static str {
    match error {
        EditVaultError::Edit(EditError::IntoItself) => strings::ERROR_MOVE_INTO_ITSELF,
        EditVaultError::Edit(EditError::RootGroup) => strings::ERROR_ROOT_GROUP,
        EditVaultError::Edit(EditError::EntryNotFound | EditError::GroupNotFound)
        | EditVaultError::VaultNotOpen => strings::ERROR_NOT_FOUND,
        EditVaultError::Edit(EditError::PairingNameEmpty) => strings::ERROR_PAIRING_NAME_EMPTY,
        EditVaultError::Edit(EditError::PairingNameTaken) => strings::ERROR_PAIRING_NAME_TAKEN,
        EditVaultError::Edit(EditError::PairingNotFound) => strings::ERROR_PAIRING_NOT_FOUND,
        EditVaultError::NotPaired => strings::ERROR_NOT_PAIRED,
        EditVaultError::Edit(EditError::InRecycleBin) => strings::ERROR_IN_RECYCLE_BIN,
        EditVaultError::Edit(EditError::FieldNameEmpty) => strings::ERROR_FIELD_NAME_EMPTY,
        EditVaultError::Edit(EditError::FieldNameTaken) => strings::ERROR_FIELD_NAME_TAKEN,
        EditVaultError::Edit(EditError::IconNotFound) => strings::ERROR_ICON_NOT_FOUND,
        EditVaultError::Edit(EditError::IconNotPng) => strings::ERROR_ICON_NOT_PNG,
        EditVaultError::Edit(EditError::IconTooLarge) => strings::ERROR_ICON_TOO_LARGE,
        EditVaultError::Edit(EditError::VersionNotFound) => strings::ERROR_VERSION_NOT_FOUND,
        EditVaultError::Edit(EditError::AttachmentNameEmpty) => {
            strings::ERROR_ATTACHMENT_NAME_EMPTY
        }
        EditVaultError::Edit(EditError::AttachmentNameTaken) => {
            strings::ERROR_ATTACHMENT_NAME_TAKEN
        }
    }
}

/// A heading above a section of the sidebar.
/// A heading of the sidebar, with its icon.
fn sidebar_heading(label: &'static str, icon: UiIcon, theme: Theme) -> AnyElement {
    div()
        .mx_1p5()
        .px_1p5()
        .py_1()
        .flex()
        .items_center()
        .gap_2()
        .child(icons::ui_icon(icon, px(14.), theme.text))
        .child(label)
        .into_any_element()
}

pub fn sidebar_separator(theme: Theme) -> AnyElement {
    div().h_px().my_1p5().bg(theme.border).into_any_element()
}

/// The pencil at the end of a picked sidebar row, opening what can be done with it.
fn edit_mark(id: (&'static str, usize), theme: Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .p_0p5()
        .rounded_sm()
        .cursor_pointer()
        .hover(|this| this.bg(theme.border))
        .child(icons::ui_icon(UiIcon::Edit, px(12.), theme.muted))
}

fn expired_badge(theme: Theme) -> impl IntoElement {
    div()
        .flex_none()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(theme.error)
        .text_xs()
        .text_color(theme.error)
        .child(strings::EXPIRED_BADGE)
}

/// A row of the sidebar, `depth` Groups in, after its icon.
pub fn sidebar_row(
    id: (&'static str, usize),
    label: SharedString,
    depth: usize,
    selected: bool,
    icon: Option<AnyElement>,
    theme: Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .mx_1p5()
        .pl(px(6. + 14. * depth as f32))
        .pr_1p5()
        .py_1()
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .cursor_pointer()
        .when(selected, |this| this.bg(theme.sidebar_selection))
        .children(icon)
        .child(div().flex_1().min_w_0().truncate().child(label))
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

/// What marks an Entry in the list and above its details: its custom icon if it has one, else
/// its site's favicon once downloaded (when the user turned that on), else its standard icon.
fn entry_mark(
    workspace: &Workspace,
    vault: VaultId,
    entry: &Entry,
    size: gpui::Pixels,
    color: gpui::Rgba,
) -> Option<AnyElement> {
    let open = workspace.vault(vault)?;
    if !matches!(entry.icon(), EntryIcon::Custom(_))
        && let Some(favicon) = favicons::image(workspace, vault, entry.id(), f32::from(size))
    {
        return Some(favicon);
    }
    Some(icons::entry_icon(entry.icon(), open.vault(), size, color))
}

fn sort_label(sort: EntrySort) -> &'static str {
    match sort {
        EntrySort::Title => strings::SORT_TITLE,
        EntrySort::Username => strings::SORT_USERNAME,
        EntrySort::Url => strings::SORT_URL,
        EntrySort::Created => strings::SORT_CREATED,
        EntrySort::Modified => strings::SORT_MODIFIED,
    }
}

pub fn title_of(entry: &Entry) -> SharedString {
    match entry.title() {
        "" => strings::UNTITLED.into(),
        title => title.to_owned().into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext,
        point, size,
    };
    use keefast_workspace::{ConfigStore, FileConfigStore, LocalStorage};

    use super::*;

    fn open_browser(
        cx: &mut TestAppContext,
        config: PathBuf,
    ) -> (Entity<VaultBrowser>, &mut VisualTestContext) {
        let store = Arc::new(FileConfigStore::new(config));
        let workspace = cx.new(|_| Workspace::new(Arc::new(LocalStorage)).with_config(store));
        let (browser, cx) = cx.add_window_view(move |window, cx| {
            let integration = cx.new(|cx| BrowserIntegration::new(workspace.clone(), cx));
            VaultBrowser::new(workspace, integration, window, cx)
        });
        cx.simulate_resize(size(px(1200.), px(800.)));
        cx.run_until_parked();
        (browser, cx)
    }

    fn drag(cx: &mut VisualTestContext, from: f32, to: f32) {
        let at = |x: f32| point(px(x), px(400.));
        cx.simulate_mouse_down(at(from), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(from + 5.), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(to), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at(to), MouseButton::Left, Modifiers::none());
    }

    fn widths(browser: &Entity<VaultBrowser>, cx: &mut VisualTestContext) -> (f32, f32) {
        browser.read_with(cx, |browser, _| {
            (
                f32::from(browser.sidebar_width),
                f32::from(browser.entry_list_width),
            )
        })
    }

    #[gpui::test]
    fn dragging_a_pane_edge_resizes_the_pane_and_is_remembered(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let (browser, cx) = open_browser(cx, config.clone());
        assert_eq!(widths(&browser, cx), (220., 320.));

        drag(cx, 220., 300.);
        assert_eq!(widths(&browser, cx), (300., 320.));
        // The Entry list's edge moved with the sidebar's.
        drag(cx, 620., 700.);
        assert_eq!(widths(&browser, cx), (300., 400.));

        cx.executor().advance_clock(PANE_WIDTHS_SAVE_DELAY);
        cx.run_until_parked();
        assert_eq!(
            FileConfigStore::new(config.clone())
                .load()
                .unwrap()
                .pane_widths,
            PaneWidths {
                sidebar: 300,
                entry_list: 400,
            }
        );

        // A Vault browser opened later starts at the remembered widths.
        let (browser, cx) = open_browser(cx, config);
        assert_eq!(widths(&browser, cx), (300., 400.));
    }

    #[gpui::test]
    fn panes_stay_within_their_widths_and_leave_the_entry_detail_room(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (browser, cx) = open_browser(cx, dir.path().join("config.toml"));

        drag(cx, 220., 20.);
        assert_eq!(widths(&browser, cx).0, SIDEBAR_WIDTHS.0);
        drag(cx, 480., 1190.);
        assert_eq!(widths(&browser, cx), (160., 720.));
        // The window is 1200 wide, so the 720 of the Entry list and the 320 the Entry detail
        // keeps leave the sidebar no room to widen.
        drag(cx, 160., 1190.);
        assert_eq!(widths(&browser, cx), (160., 720.));
    }

    #[gpui::test]
    fn double_clicking_a_pane_edge_puts_the_pane_back_to_its_default_width(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (browser, cx) = open_browser(cx, dir.path().join("config.toml"));
        drag(cx, 220., 300.);

        let position = point(px(300.), px(400.));
        for click_count in [1, 2] {
            cx.simulate_event(MouseDownEvent {
                position,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count,
            });
        }
        assert_eq!(widths(&browser, cx), (220., 320.));
    }
}
