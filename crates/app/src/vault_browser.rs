use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, Context, DragMoveEvent, EmptyView, Entity, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, MouseButton, Pixels, PromptLevel, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, anchored, deferred, div,
    prelude::*, px,
};
use keefast_core::{
    EditError, Entry, EntryColor, EntryFields, EntryId, EntrySearch, EntrySort, Group, GroupId,
    TotpCode,
};
use keefast_workspace::{
    CopyError, CopyField, EditVaultError, EntryScope, ListedEntry, PaneWidths, VaultId, View,
    Workspace,
};
use secrecy::{ExposeSecret, SecretString};

use crate::actions::{
    self, ChangeMasterKey, CopyPassword, CopyTotp, CopyUsername, FocusSearch, LockAll, NewVault,
    OpenEntry, OpenVault, OpenVaultSettings, SaveAll, SelectFirstEntry, SelectLastEntry,
    SelectNextEntry, SelectPreviousEntry, ShowEntryHistory, SortByCreated, SortByModified,
    SortByTitle, SortByUrl, SortByUsername, SyncNow, ToggleCommandPalette,
};
use crate::attachments::{self, Preview};
use crate::browser_integration::BrowserIntegration;
use crate::clipboard::AppClipboard;
use crate::entry_editor::{EntryEditor, EntryEditorEvent};
use crate::entry_history::{EntryHistory, EntryHistoryEvent};
use crate::icons::{self, IconName};
use crate::locking;
use crate::merge_notice;
use crate::saving;
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::totp_setup::{TotpSetup, TotpSetupEvent};
use crate::ui::{self, Kbd, Typography};
use crate::vault_key::{VaultKeyEvent, VaultKeyForm};

mod detail;
mod entry_list;
mod sidebar;

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
    /// The field of the selected Entry being changed where it's shown.
    inline_edit: Option<InlineEdit>,
    /// The Entry whose TOTP code was copied last, to say so until another is selected.
    totp_copied: Option<(VaultId, EntryId)>,
    error: Option<SharedString>,
    /// What the last command did, such as copying a password, shown as a toast.
    notice: Option<Notice>,
    /// Counts the notices shown, so each toast runs its own countdown.
    notice_generation: u64,
    entry_list_scroll: UniformListScrollHandle,
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
    /// The menu open, if one is.
    menu: Option<ListMenu>,
    /// The tab of the Entry detail shown.
    detail_tab: DetailTab,
    /// Whether the selected Entry's password is shown rather than masked.
    password_revealed: bool,
    /// The Groups whose Entries the Entry list folds away.
    collapsed_groups: HashSet<(VaultId, GroupId)>,
    /// Whether the sidebar lists every tag rather than the first few.
    tags_expanded: bool,
    /// The Vault last shown, which the sidebar stays on while a tag or colour is shown.
    last_vault: Option<VaultId>,
}

/// The tabs of the Entry detail.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum DetailTab {
    #[default]
    Overview,
    Fields,
    Files,
    Access,
}

/// A toast saying what the last command did.
struct Notice {
    text: SharedString,
    /// How long until what it speaks of runs out, as a copied password is cleared.
    countdown: Option<Duration>,
    generation: u64,
}

/// What picking a menu's choice does.
type MenuAction = Rc<dyn Fn(&mut VaultBrowser, &mut Window, &mut Context<VaultBrowser>)>;

/// A choice in one of the Entry list's menus.
struct MenuItem {
    label: SharedString,
    checked: bool,
    danger: bool,
    action: MenuAction,
}

impl MenuItem {
    fn new(
        label: impl Into<SharedString>,
        checked: bool,
        action: impl Fn(&mut VaultBrowser, &mut Window, &mut Context<VaultBrowser>) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            checked,
            danger: false,
            action: Rc::new(action),
        }
    }

    /// Drawn in the danger colour, as deleting is.
    fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// The menus of the Entry list's buttons.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListMenu {
    /// What to add: an Entry or a Group.
    Add,
    Sort,
    /// The Vaults to switch to, under the Vault switcher.
    Vaults,
    /// The selected Entry's other commands.
    EntryMore,
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

#[derive(Clone, Copy)]
enum EditTarget {
    Existing(VaultId, EntryId),
    New(VaultId, GroupId),
}

/// A field of an Entry changed where the Entry detail shows it, KeeWeb's way: clicked, it
/// turns into a text field, a password shown, and is saved on Enter or on clicking away.
struct InlineEdit {
    vault: VaultId,
    entry: EntryId,
    field: InlineField,
    input: Entity<TextField>,
    _blur: Subscription,
}

/// The fields of an Entry that can be changed where they're shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineField {
    Title,
    Username,
    Password,
    Url,
    Notes,
    /// The value of a custom field, by index.
    Custom(usize),
}

impl InlineField {
    fn value(self, entry: &Entry) -> Option<&str> {
        Some(match self {
            Self::Title => entry.title(),
            Self::Username => entry.username(),
            Self::Password => entry.password().expose_secret(),
            Self::Url => entry.url(),
            Self::Notes => entry.notes(),
            Self::Custom(index) => entry.custom_fields().get(index)?.value.expose_secret(),
        })
    }

    fn set(self, fields: &mut EntryFields, value: String) {
        match self {
            Self::Title => fields.title = value,
            Self::Username => fields.username = value,
            Self::Password => fields.password = SecretString::from(value),
            Self::Url => fields.url = value,
            Self::Notes => fields.notes = value,
            Self::Custom(index) => {
                if let Some(custom) = fields.custom_fields.get_mut(index) {
                    custom.value = SecretString::from(value);
                }
            }
        }
    }
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
        let search = cx.new(|cx| {
            TextField::new(strings::SEARCH_PLACEHOLDER, cx)
                .with_icon(IconName::Search)
                .search()
        });
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
            inline_edit: None,
            totp_copied: None,
            error: None,
            notice: None,
            notice_generation: 0,
            entry_list_scroll: UniformListScrollHandle::new(),
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
            detail_tab: DetailTab::default(),
            password_revealed: false,
            collapsed_groups: HashSet::new(),
            tags_expanded: false,
            last_vault: None,
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

    /// What the latest Merges changed, under the toolbar until dismissed.
    fn render_merge_notice(&self, theme: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.merge_notice.clone()?;
        Some(
            div()
                .id("merge-notice")
                .flex_none()
                .flex()
                .items_start()
                .gap(px(10.))
                .px(px(16.))
                .py(px(9.))
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.bg_1)
                .child(div().pt(px(2.)).child(icons::ui_icon(
                    IconName::GitMerge,
                    px(15.),
                    theme.fg_1,
                )))
                .child(div().flex_1().min_w_0().text_body().child(notice))
                .child(
                    ui::IconButton::new("dismiss-merge-notice", IconName::X)
                        .small()
                        .tooltip(strings::MERGE_NOTICE_DISMISS)
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
        if self
            .inline_edit
            .as_ref()
            .is_some_and(|edit| locked(edit.vault))
        {
            self.inline_edit = None;
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

    /// The Vault the sidebar shows the Groups of: the one shown, else the selected Entry's,
    /// else the one last shown, else the first.
    fn current_vault(&self, cx: &App) -> Option<VaultId> {
        let workspace = self.workspace.read(cx);
        let exists = |vault: &VaultId| {
            workspace.vault(*vault).is_some() || workspace.locked_vault(*vault).is_some()
        };
        let view = workspace.view();
        view.scope
            .vault()
            .or(view.entry.map(|(vault, _)| vault))
            .or(self.last_vault)
            .filter(exists)
            .or_else(|| workspace.vaults().next().map(|open| open.id()))
            .or_else(|| workspace.locked_vaults().next().map(|locked| locked.id()))
    }

    /// The Group an Entry is in, with the name to show for it: the Vault's for its top Group.
    fn group_of(
        &self,
        vault: VaultId,
        entry: EntryId,
        cx: &App,
    ) -> Option<(GroupId, SharedString)> {
        let open = self.workspace.read(cx).vault(vault)?;
        let root = open.vault().root();
        let mut groups = vec![(root, 0)];
        flatten_groups(root.groups(), 1, &mut groups);
        groups
            .into_iter()
            .find(|(group, _)| group.entries().iter().any(|found| found.id() == entry))
            .map(|(group, depth)| {
                let name = if depth == 0 {
                    open.display_name()
                } else {
                    group.name().to_owned()
                };
                (group.id(), name.into())
            })
    }

    /// Moves the user to `view`. A browser pairs with the Vault shown.
    fn show(&mut self, view: View, cx: &mut Context<Self>) {
        if let Some(vault) = view.scope.vault() {
            self.last_vault = Some(vault);
        }
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
        window.focus(&self.search.focus_handle(cx), cx);
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
            self.entry_list_scroll
                .scroll_to_item(0, ScrollStrategy::Top);
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
                window.focus(&self.focus, cx);
            }
            TextFieldEvent::Cancel => {
                if self.search.read(cx).text().is_empty() {
                    window.focus(&self.focus, cx);
                }
                self.search.update(cx, |search, cx| search.set_text("", cx));
            }
        }
    }

    fn sort_by(&mut self, sort: EntrySort, cx: &mut Context<Self>) {
        self.sort = sort;
        if let Some(ix) = self.selected_position(cx) {
            self.entry_list_scroll
                .scroll_to_item(ix, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// The row the selected Entry is drawn in, if it is listed.
    fn selected_position(&self, cx: &App) -> Option<usize> {
        let workspace = self.workspace.read(cx);
        self.list_model(workspace).row_of(self.selected_entry(cx)?)
    }

    fn select_entry(&mut self, vault: VaultId, entry: EntryId, cx: &mut Context<Self>) {
        self.finish_inline_edit(cx);
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
        self.password_revealed = false;
        self.detail_tab = DetailTab::Overview;
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
        let model = self.list_model(workspace);
        let entries: Vec<(VaultId, EntryId)> = model
            .entries()
            .map(|(vault, found)| (vault, found.id()))
            .collect();
        if entries.is_empty() {
            return;
        }
        let current = entries.iter().position(|listed| Some(*listed) == entry);
        let ix = pick(current, entries.len()).min(entries.len() - 1);
        let (vault, entry) = entries[ix];
        let row = model.row_of((vault, entry)).unwrap_or(ix);
        self.select_entry(vault, entry, cx);
        self.entry_list_scroll
            .scroll_to_item(row, ScrollStrategy::Top);
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
                self.show_notice(strings::NOTHING_TO_COPY, None);
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
        let text: SharedString = match clears_in {
            Some(wait) => {
                // Rounded up: a moment has passed since copying.
                let seconds = wait.as_secs_f64().ceil().to_string();
                let clears = strings::COPIED_CLEARS_IN.replace("{seconds}", &seconds);
                format!("{copied} {clears}").into()
            }
            None => copied.into(),
        };
        self.show_notice(text, clears_in);
        self.error = None;
        // A new copy replaces the clear waiting for the last one.
        let workspace = self.workspace.clone();
        self.clipboard_clear = clears_in.map(|_| {
            cx.spawn(async move |this, cx| {
                while let Some(wait) =
                    workspace.read_with(cx, |workspace, _| workspace.clipboard_clears_in())
                {
                    cx.background_executor().timer(wait).await;
                    workspace.update(cx, |workspace, cx| {
                        workspace.clear_clipboard_if_due(&mut AppClipboard(cx))
                    });
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

    fn show_notice(&mut self, text: impl Into<SharedString>, countdown: Option<Duration>) {
        self.notice_generation += 1;
        self.notice = Some(Notice {
            text: text.into(),
            countdown,
            generation: self.notice_generation,
        });
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
        window.focus(&setup.focus_handle(cx), cx);
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
        window.focus(&self.focus, cx);
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
        window.focus(&history.focus_handle(cx), cx);
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
                self.show_notice(strings::HISTORY_RESTORED, None);
                self.revealed_fields.clear();
                self.copied_field = None;
            }
            EntryHistoryEvent::Close => self.detail = Detail::Entry,
        }
        window.focus(&self.focus, cx);
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
                        this.show_notice(notice, None);
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
            "escape" => window.focus(&self.focus, cx),
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

    /// Turns a field of the selected Entry into a text field to change it with, saving the one
    /// changed before.
    fn start_inline_edit(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        field: InlineField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_inline_edit(cx);
        let workspace = self.workspace.read(cx);
        let Some(value) = workspace
            .entry(vault, entry)
            .and_then(|entry| field.value(entry))
        else {
            return;
        };
        let value = zeroize::Zeroizing::new(value.to_owned());
        let input = cx.new(|cx| {
            let mut input = TextField::new("", cx);
            if field == InlineField::Notes {
                input = input.multiline();
            }
            input.set_text(&value, cx);
            input
        });
        cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            match event {
                TextFieldEvent::Submit => this.finish_inline_edit(cx),
                TextFieldEvent::Cancel => this.inline_edit = None,
            }
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        // Clicking away saves, as Enter does.
        let blur = cx.on_blur(&input.focus_handle(cx), window, |this, _, cx| {
            this.finish_inline_edit(cx)
        });
        window.focus(&input.focus_handle(cx), cx);
        self.inline_edit = Some(InlineEdit {
            vault,
            entry,
            field,
            input,
            _blur: blur,
        });
        cx.notify();
    }

    /// Saves the field being changed in place, if it was changed.
    fn finish_inline_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.inline_edit.take() else {
            return;
        };
        cx.notify();
        let value = edit.input.read(cx).text().to_owned();
        let Some(entry) = self.workspace.read(cx).entry(edit.vault, edit.entry) else {
            return;
        };
        if edit.field.value(entry) == Some(value.as_str()) {
            return;
        }
        let mut fields = EntryFields::from(entry);
        edit.field.set(&mut fields, value);
        self.edit(cx, |workspace| {
            workspace.update_entry(edit.vault, edit.entry, &fields)
        });
    }

    /// The text field of `field` of the Entry shown, if it's being changed in place.
    fn inline_input(
        &self,
        vault: VaultId,
        entry: EntryId,
        field: InlineField,
    ) -> Option<Entity<TextField>> {
        self.inline_edit
            .as_ref()
            .filter(|edit| (edit.vault, edit.entry, edit.field) == (vault, entry, field))
            .map(|edit| edit.input.clone())
    }

    /// `value` shown as a field of the Entry detail, which a click turns into a text field
    /// to change it with.
    fn editable_value(
        &self,
        vault: VaultId,
        entry: EntryId,
        field: InlineField,
        value: impl IntoElement,
        theme: Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        // Both offset by the text field's padding and border, so the text stays put.
        let place = div().flex_1().min_w_0().mx(px(-9.)).my(px(-7.));
        if let Some(input) = self.inline_input(vault, entry, field) {
            return place.child(input).into_any_element();
        }
        let id = match field {
            InlineField::Title => ("edit-title", 0),
            InlineField::Username => ("edit-username", 0),
            InlineField::Password => ("edit-password", 0),
            InlineField::Url => ("edit-url", 0),
            InlineField::Notes => ("edit-notes", 0),
            InlineField::Custom(index) => ("edit-custom-field", index),
        };
        place
            .id(id)
            .px_2()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .hover(|style| style.border_color(theme.border))
            .cursor_text()
            .child(value)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.start_inline_edit(vault, entry, field, window, cx)
            }))
            .into_any_element()
    }

    fn open_editor(&mut self, target: EditTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.inline_edit = None;
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
        window.focus(&editor.focus_handle(cx), cx);
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
            Detail::Editing { editor, .. } => window.focus(&editor.focus_handle(cx), cx),
            _ => window.focus(&self.focus, cx),
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
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        window.focus(&field.focus_handle(cx), cx);
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
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        window.focus(&form.focus_handle(cx), cx);
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
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
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

    /// A menu under the button it belongs to, drawn over the panes.
    fn render_menu(&self, items: Vec<MenuItem>, theme: Theme, cx: &Context<Self>) -> AnyElement {
        let rows = items.into_iter().enumerate().map(|(ix, item)| {
            let action = item.action;
            ui::MenuItem::new(("menu-item", ix), item.label)
                .when(item.checked, |this| this.checked(true))
                .when(item.danger, |this| this.danger())
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
                    anchored()
                        .snap_to_window_with_margin(px(8.))
                        .child(ui::appear(
                            "list-menu-appear",
                            ui::menu(theme)
                                .id("list-menu")
                                .occlude()
                                .mt_1()
                                .children(rows),
                        )),
                )
                .with_priority(1),
            )
            .into_any_element()
    }
}

impl Render for VaultBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        self.ensure_custom_field_focus(cx);
        let palette_keys = Kbd::for_action(&ToggleCommandPalette, cx);
        let edit_keys = Kbd::for_action(&OpenEntry, cx);
        let save_keys = Kbd::for_action(&crate::actions::SaveEntry, cx);
        // A locked Vault, or one about to lock, shows a card to unlock it instead of its Entries.
        let locked = {
            let workspace = self.workspace.read(cx);
            workspace
                .view()
                .scope
                .vault()
                .filter(|vault| workspace.readable_vault(*vault).is_none())
        };
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
            .bg(theme.bg_0)
            .text_color(theme.fg_0)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .on_drag_move(cx.listener(Self::on_pane_edge_drag))
                    .child(self.render_sidebar(theme, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(self.render_toolbar(palette_keys, save_keys, theme, cx))
                            .children(self.render_merge_notice(theme, cx))
                            .child(match locked {
                                Some(vault) => self.render_locked(vault, theme, cx),
                                None => div()
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .child(self.render_entry_list(theme, cx))
                                    .child(self.render_detail(edit_keys, theme, cx))
                                    .into_any_element(),
                            }),
                    )
                    // After the panes, so the edges are grabbed over the panes either side of
                    // them.
                    .child(self.render_pane_edge(PaneEdge::Sidebar, self.sidebar_width, theme, cx))
                    .when(locked.is_none(), |this| {
                        this.child(self.render_pane_edge(
                            PaneEdge::EntryList,
                            self.sidebar_width + self.entry_list_width,
                            theme,
                            cx,
                        ))
                    })
                    .when_some(self.notice.as_ref(), |this, notice| {
                        this.child(
                            div()
                                .absolute()
                                .bottom(px(24.))
                                .left(self.sidebar_width)
                                .right_0()
                                .flex()
                                .justify_center()
                                .child(
                                    ui::Toast::new(
                                        ("notice", notice.generation),
                                        notice.text.clone(),
                                    )
                                    .countdown(notice.countdown),
                                ),
                        )
                    }),
            )
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

pub fn sidebar_separator(theme: Theme) -> AnyElement {
    div()
        .h_px()
        .my(px(6.))
        .mx(px(8.))
        .bg(theme.border)
        .into_any_element()
}

/// The pencil at the end of a picked sidebar row, opening what can be done with it.
fn edit_mark(id: (&'static str, usize), theme: Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .p(px(3.))
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|this| this.bg(theme.bg_hover))
        .child(icons::ui_icon(IconName::Pencil, px(12.), theme.fg_2))
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
    ui::nav_row(id, selected, theme)
        .mx(px(8.))
        .pl(px(8. + 14. * depth as f32))
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
pub fn grouped(code: &str) -> SharedString {
    if code.len() < 6 {
        return code.to_owned().into();
    }
    let (first, second) = code.split_at(code.len() / 2);
    format!("{first} {second}").into()
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

/// A colour's name, as the Entry detail and the editor say it.
pub fn color_name(color: EntryColor) -> &'static str {
    match color {
        EntryColor::Red => strings::COLOR_RED,
        EntryColor::Orange => strings::COLOR_ORANGE,
        EntryColor::Yellow => strings::COLOR_YELLOW,
        EntryColor::Green => strings::COLOR_GREEN,
        EntryColor::Blue => strings::COLOR_BLUE,
        EntryColor::Violet => strings::COLOR_PURPLE,
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
        assert_eq!(widths(&browser, cx), (232., 340.));

        drag(cx, 232., 300.);
        assert_eq!(widths(&browser, cx), (300., 340.));
        // The Entry list's edge moved with the sidebar's.
        drag(cx, 640., 700.);
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

        drag(cx, 232., 20.);
        assert_eq!(widths(&browser, cx).0, SIDEBAR_WIDTHS.0);
        drag(cx, 500., 1190.);
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
        drag(cx, 232., 300.);

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
        assert_eq!(widths(&browser, cx), (232., 340.));
    }

    const FIXTURE: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");

    /// A browser over the fixture Vault with its first Entry selected, and that Entry.
    fn open_entry<'a>(
        cx: &'a mut TestAppContext,
        dir: &std::path::Path,
    ) -> (
        Entity<VaultBrowser>,
        VaultId,
        EntryId,
        &'a mut VisualTestContext,
    ) {
        let path = dir.join("Personal.kdbx");
        std::fs::write(&path, FIXTURE).unwrap();
        let mut workspace = Workspace::new(Arc::new(LocalStorage));
        let vault = workspace
            .open_vault(
                &keefast_workspace::VaultLocation::Local(path),
                &keefast_workspace::UnlockSecrets::password("fixture-password"),
            )
            .unwrap();
        let entry = workspace.find_entries(
            EntryScope::Vault(vault),
            &EntrySearch::default(),
            EntrySort::default(),
        )[0]
        .entry
        .id();
        let workspace = cx.new(|_| workspace);
        let (browser, cx) = cx.add_window_view(move |window, cx| {
            let integration = cx.new(|cx| BrowserIntegration::new(workspace.clone(), cx));
            VaultBrowser::new(workspace, integration, window, cx)
        });
        cx.update(|window, cx| {
            // Focus changes are reported only in the active window.
            window.activate_window();
            browser.update(cx, |browser, cx| browser.select_entry(vault, entry, cx));
        });
        cx.run_until_parked();
        (browser, vault, entry, cx)
    }

    fn edit_in_place(
        browser: &Entity<VaultBrowser>,
        (vault, entry): (VaultId, EntryId),
        field: InlineField,
        cx: &mut VisualTestContext,
    ) {
        browser.update_in(cx, |browser, window, cx| {
            browser.start_inline_edit(vault, entry, field, window, cx)
        });
        cx.run_until_parked();
    }

    fn password(
        browser: &Entity<VaultBrowser>,
        at: (VaultId, EntryId),
        cx: &mut VisualTestContext,
    ) -> String {
        browser.read_with(cx, |browser, cx| {
            let entry = browser.workspace.read(cx).entry(at.0, at.1).unwrap();
            entry.password().expose_secret().to_owned()
        })
    }

    #[gpui::test]
    fn a_password_changed_in_place_is_shown_while_typed_and_saved_on_enter(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (browser, vault, entry, cx) = open_entry(cx, dir.path());
        let before = password(&browser, (vault, entry), cx);
        edit_in_place(&browser, (vault, entry), InlineField::Password, cx);
        let input = browser.read_with(cx, |browser, _| {
            browser
                .inline_input(vault, entry, InlineField::Password)
                .unwrap()
        });
        assert_eq!(
            input.read_with(cx, |input, _| input.text().to_owned()),
            before
        );

        cx.simulate_keystrokes("ctrl-a n e w enter");
        assert_eq!(password(&browser, (vault, entry), cx), "new");
        assert!(browser.read_with(cx, |browser, _| browser.inline_edit.is_none()));
    }

    #[gpui::test]
    fn escape_leaves_a_field_as_it_was_and_clicking_away_saves_it(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (browser, vault, entry, cx) = open_entry(cx, dir.path());
        let before = password(&browser, (vault, entry), cx);
        edit_in_place(&browser, (vault, entry), InlineField::Password, cx);
        cx.simulate_keystrokes("x escape");
        assert_eq!(password(&browser, (vault, entry), cx), before);

        edit_in_place(&browser, (vault, entry), InlineField::Password, cx);
        cx.simulate_keystrokes("x");
        cx.update(|window, cx| {
            let focus = browser.read(cx).focus.clone();
            window.focus(&focus, cx)
        });
        cx.run_until_parked();
        assert_eq!(password(&browser, (vault, entry), cx), format!("{before}x"));
    }

    /// The Entry list as drawn: a Group heading as `# name`, an Entry as its title.
    fn drawn(browser: &Entity<VaultBrowser>, cx: &mut VisualTestContext) -> Vec<String> {
        browser.read_with(cx, |browser, cx| {
            let workspace = browser.workspace.read(cx);
            browser
                .list_model(workspace)
                .rows
                .iter()
                .map(|row| match row {
                    entry_list::ListRow::Group { name, .. } => format!("# {name}"),
                    entry_list::ListRow::Entry(_, entry) => entry.title().to_owned(),
                })
                .collect()
        })
    }

    fn selected_title(browser: &Entity<VaultBrowser>, cx: &mut VisualTestContext) -> String {
        browser.read_with(cx, |browser, cx| {
            let (vault, entry) = browser.selected_entry(cx).unwrap();
            let workspace = browser.workspace.read(cx);
            workspace.entry(vault, entry).unwrap().title().to_owned()
        })
    }

    #[gpui::test]
    fn the_entry_list_heads_each_group_and_down_follows_the_list_as_drawn(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (browser, vault, _, cx) = open_entry(cx, dir.path());
        browser.update(cx, |browser, cx| {
            browser.select(EntryScope::Vault(vault), cx)
        });
        let rows = drawn(&browser, cx);
        assert!(rows[0].starts_with("# "), "{rows:?}");
        assert!(rows.iter().filter(|row| row.starts_with("# ")).count() > 1);

        let titles: Vec<String> = rows
            .into_iter()
            .filter(|row| !row.starts_with("# "))
            .collect();
        cx.update(|window, cx| {
            actions::bind_keys(cx);
            let focus = browser.read(cx).focus.clone();
            window.focus(&focus, cx)
        });
        let mut visited = Vec::new();
        for _ in 0..titles.len() {
            cx.simulate_keystrokes("down");
            visited.push(selected_title(&browser, cx));
        }
        assert_eq!(visited, titles);
    }

    #[gpui::test]
    fn a_folded_group_hides_its_entries_from_the_list_and_the_keys(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (browser, vault, _, cx) = open_entry(cx, dir.path());
        browser.update(cx, |browser, cx| {
            browser.select(EntryScope::Vault(vault), cx)
        });
        let first_group = browser.read_with(cx, |browser, cx| {
            let workspace = browser.workspace.read(cx);
            match &browser.list_model(workspace).rows[0] {
                entry_list::ListRow::Group { vault, group, .. } => (*vault, *group),
                entry_list::ListRow::Entry(..) => panic!("the list starts with a heading"),
            }
        });
        let before = drawn(&browser, cx);
        browser.update(cx, |browser, _| {
            browser.collapsed_groups.insert(first_group)
        });
        let after = drawn(&browser, cx);

        assert_eq!(after[0], before[0]);
        assert!(after[1].starts_with("# "), "{after:?}");
        let hidden = before
            .iter()
            .skip(1)
            .take_while(|row| !row.starts_with("# "))
            .count();
        assert_eq!(after.len(), before.len() - hidden);
        cx.update(|window, cx| {
            actions::bind_keys(cx);
            let focus = browser.read(cx).focus.clone();
            window.focus(&focus, cx)
        });
        cx.simulate_keystrokes("home");
        assert_eq!(selected_title(&browser, cx), after[2]);
    }

    #[gpui::test]
    fn ctrl_h_shows_the_selected_entrys_history(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (browser, _, _, cx) = open_entry(cx, dir.path());
        cx.update(|window, cx| {
            actions::bind_keys(cx);
            let focus = browser.read(cx).focus.clone();
            window.focus(&focus, cx)
        });
        cx.simulate_keystrokes("ctrl-h");
        assert!(browser.read_with(cx, |browser, _| matches!(
            browser.detail,
            Detail::History { .. }
        )));
    }
}
