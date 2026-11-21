//! The Entry list and the Entries themselves: reading, creating, editing, moving, deleting.
//!
//! Passwords and protected custom fields never leave in an Entry's DTO: the UI asks for one with
//! `reveal_password` or `reveal_custom_field` when the user shows it, and edits send only what
//! the user changed, so an untouched secret never crosses the boundary. Plugins' facets come with
//! the labels their Plugins derived, which are the one thing shown about a protected value
//! without a reveal (ADR 0009).

use std::collections::HashMap;
use std::sync::Arc;

use keefast_core::{
    self as core, AccessDecision, AttachmentPreview, CustomField, Entry, EntrySearch, EntrySort,
    Group, PasswordStrength, Site, Timestamp, Totp,
};
use keefast_plugins::{Capability, EntryField, FieldKind, FoundFacet, PluginHost};
use keefast_workspace::{
    self as ws, CustomFieldEdit, CustomFieldValue, EntryEdit, TagsEdit, Workspace,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::Core;
use super::facets::FacetEditorView;
use super::generator::StrengthView;
use crate::core::AppCore;
use crate::dto::{
    Color, EntryId, GroupId, IconChoice, IconView, Scope, VaultId, icon_view, seconds, title_of,
};
use crate::error::{ApiError, ApiResult, ErrorKind, describe_totp};
use crate::icon_protocol;

/// The orders the Entry list sorts in. Text sorts A to Z ignoring case, dates newest first;
/// Entries without the value come last.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SortOrder {
    #[default]
    Title,
    Username,
    Url,
    Created,
    Modified,
}

impl From<SortOrder> for EntrySort {
    fn from(sort: SortOrder) -> Self {
        match sort {
            SortOrder::Title => EntrySort::Title,
            SortOrder::Username => EntrySort::Username,
            SortOrder::Url => EntrySort::Url,
            SortOrder::Created => EntrySort::Created,
            SortOrder::Modified => EntrySort::Modified,
        }
    }
}

/// What the Entry list shows: the Entries in `scope` matching `search` (best match first, small
/// typos allowed, passwords and protected fields never searched), equally good ones in `sort`
/// order.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryQuery {
    pub scope: Scope,
    pub search: String,
    pub sort: SortOrder,
}

/// An Entry as the list shows it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub vault: VaultId,
    pub id: EntryId,
    /// The title, or "(untitled)".
    pub title: String,
    pub username: String,
    /// What the list shows under the title in place of a username, when the Entry has none: the
    /// label a Plugin derived for one of its facets, such as "Visa •••• 1111" (ADR 0009). A
    /// username always wins, so this is `None` when there is one. Never a protected value.
    pub label: Option<String>,
    pub url: String,
    pub tags: Vec<String>,
    pub color: Option<Color>,
    pub icon: IconView,
    /// The site's favicon, once downloaded (with favicon download turned on).
    pub favicon_url: Option<String>,
    /// Seconds since 1970, UTC.
    pub expires: Option<f64>,
    pub expired: bool,
    pub created: Option<f64>,
    pub modified: Option<f64>,
    pub has_password: bool,
    pub has_totp: bool,
    pub attachment_count: u32,
    pub group: GroupId,
    pub group_name: String,
    pub vault_name: String,
    /// Whether it is in the Recycle Bin.
    pub in_recycle_bin: bool,
}

/// A custom field. A protected value is left out until revealed.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomFieldView {
    pub name: String,
    pub protected: bool,
    /// `None` when protected: ask `reveal_custom_field` for it.
    pub value: Option<String>,
    /// Whether the value is empty, which a hidden value doesn't otherwise show.
    pub is_empty: bool,
    /// Whether it is one of the fields TOTP settings are kept in (`otp`, `TOTP Seed`,
    /// `TimeOtp-*`…), which the UI shows as the Entry's TOTP rather than as a field.
    pub totp_storage: bool,
}

/// How an attachment can be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    Text,
    Image,
    None,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub index: u32,
    pub name: String,
    /// In bytes.
    pub size: u32,
    pub preview: PreviewKind,
}

/// An Entry's TOTP: its settings, or why they can't be read. The code itself comes from
/// `get_totp_code`.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TotpView {
    Ready {
        digits: u32,
        period: u32,
        algorithm: crate::commands::totp::TotpAlgorithmChoice,
        steam: bool,
    },
    Unreadable {
        reason: String,
    },
}

/// An Access decision remembered on an Entry, for one host.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RememberedAccessView {
    pub host: String,
    pub allowed: bool,
}

/// What an Owned field holds, which says how the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum OwnedFieldKind {
    Text,
    /// Digits, shown in a monospace font.
    Digits,
    /// A month as `MM/YY`, such as a card's expiry; the UI says when it has passed or is near.
    MonthYear,
}

impl From<FieldKind> for OwnedFieldKind {
    fn from(kind: FieldKind) -> Self {
        match kind {
            FieldKind::Text => Self::Text,
            FieldKind::Digits => Self::Digits,
            FieldKind::MonthYear => Self::MonthYear,
        }
    }
}

/// One of a facet's Owned fields on an Entry. A protected value is left out, as a custom field's
/// is: reveal and copy it by `index`, as any custom field.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnedFieldView {
    /// How its Plugin names it.
    pub key: String,
    /// The custom field's name, such as `Card number`.
    pub name: String,
    /// What the section calls it, such as "Number".
    pub label: String,
    pub kind: OwnedFieldKind,
    /// Its index in the Entry's custom fields; `None` when the Entry hasn't got it.
    pub index: Option<u32>,
    /// Whether the Entry keeps it protected (or, when missing, whether its Plugin would).
    pub protected: bool,
    /// `None` when protected or missing.
    pub value: Option<String>,
    pub is_empty: bool,
    /// What its Plugin derived to show in place of the value, without a reveal, such as
    /// "Visa •••• 1111".
    pub text: Option<String>,
    /// What its input shows while empty in the Entry editor, such as "MM/YY".
    pub placeholder: Option<String>,
}

/// A facet a Plugin found on an Entry, such as its Card: a section of the detail.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FacetView {
    /// The Plugin's id, such as `keefast.card`.
    pub plugin: String,
    /// The facet's id within the Plugin, such as `card`.
    pub facet: String,
    /// The section's title, such as "Card".
    pub title: String,
    /// One of the kit's icons, by name (`credit-card`).
    pub icon: Option<String>,
    /// The label its Plugin derived, such as "Visa •••• 1111".
    pub label: Option<String>,
    /// Whether the Plugin has the Plugin capability to have its fields copied.
    pub can_copy: bool,
    /// The names of the fields an Entry has the facet by.
    pub detect: Vec<String>,
    /// Its section in the Entry editor; `None` when its Owned fields are edited as custom fields.
    pub editor: Option<FacetEditorView>,
    pub fields: Vec<OwnedFieldView>,
    /// Its commands in the command palette's Entry section. Empty without the `copy` Plugin
    /// capability, which every v1 command needs.
    pub commands: Vec<FacetCommandView>,
}

/// A facet's command palette command, copying one of its Owned fields with `copy_field`.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FacetCommandView {
    /// Unique within the facet, such as `copy-number`.
    pub id: String,
    /// As the palette lists it: "Copy card number".
    pub title: String,
    /// The key of the Owned field it copies, as in `fields`.
    pub field: String,
    /// The keys that run it, such as "Ctrl Shift C"; `None` when the Plugin declared none, or
    /// keefast or another Plugin has them.
    pub shortcut: Option<String>,
}

/// Everything about an Entry the detail shows, except its secrets.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetail {
    #[serde(flatten)]
    pub summary: EntrySummary,
    /// The title as stored, possibly empty.
    pub raw_title: String,
    pub notes: String,
    /// Other URLs the Entry is for, as KeePassXC keeps them (`KP2A_URL…` fields).
    pub additional_urls: Vec<String>,
    pub custom_fields: Vec<CustomFieldView>,
    /// The facets Plugins found on it, in the order the Plugins were loaded.
    pub facets: Vec<FacetView>,
    pub attachments: Vec<AttachmentView>,
    pub totp: Option<TotpView>,
    /// How strong the password is, `None` without one. Estimated here, so showing it needs no
    /// reveal.
    pub strength: Option<StrengthView>,
    /// How many earlier versions the Entry's history keeps.
    pub history_count: u32,
    pub remembered_access: Vec<RememberedAccessView>,
    /// The Groups from the top Group (the Vault) down to the Entry's.
    pub group_path: Vec<String>,
    /// Whether deleting it deletes it for good rather than moving it to the Recycle Bin.
    pub deletes_for_good: bool,
}

/// A custom field to give an Entry. A `value` of `None` keeps the value the field had at
/// `original` (its index in the Entry's custom fields), so an unrevealed protected value never
/// needs to cross to the UI and back.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomFieldInput {
    pub name: String,
    pub value: Option<String>,
    pub protected: bool,
    pub original: Option<u32>,
}

/// The fields of an Entry, as the UI edits them. `password: None` keeps the password the Entry
/// has (empty for a new Entry). Attachments aren't here: they have commands of their own, and
/// are kept as they are.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    pub title: String,
    pub username: String,
    pub password: Option<String>,
    pub url: String,
    pub notes: String,
    pub custom_fields: Vec<CustomFieldInput>,
    /// Each split further on commas and semicolons, as KeePass does.
    pub tags: Vec<String>,
    pub color: Option<Color>,
    pub icon: IconChoice,
    /// The expiry date as `YYYY-MM-DD` (the start of that day, UTC), or `None` for none. A date
    /// the Entry's expiry already falls on keeps its time.
    pub expires: Option<String>,
}

impl EntryInput {
    /// The edit the editor saves: every part it shows, so what it leaves as it was is only what
    /// it never revealed.
    fn to_edit(&self) -> ApiResult<EntryEdit> {
        Ok(EntryEdit {
            title: Some(self.title.clone()),
            username: Some(self.username.clone()),
            password: self.password.as_deref().map(SecretString::from),
            url: Some(self.url.clone()),
            notes: Some(self.notes.clone()),
            custom_fields: Some(
                self.custom_fields
                    .iter()
                    .map(CustomFieldInput::to_edit)
                    .collect(),
            ),
            tags: Some(TagsEdit::Replace(self.tags.clone())),
            color: Some(self.color.map(Into::into)),
            icon: Some(self.icon.to_core()?),
            expires: Some(self.expires.clone()),
            attachments: Vec::new(),
        })
    }
}

impl CustomFieldInput {
    fn to_edit(&self) -> CustomFieldEdit {
        let value = match (&self.value, self.original) {
            (Some(value), _) => CustomFieldValue::New(SecretString::from(value.as_str())),
            (None, Some(original)) => CustomFieldValue::Kept(original as usize),
            (None, None) => CustomFieldValue::Empty,
        };
        CustomFieldEdit {
            name: self.name.clone(),
            value,
            protected: self.protected,
        }
    }
}

/// Applies `edit` to an Entry and writes it under one lock of the Workspace, so a save's Merge
/// can't land in between and be undone; then autosaves. `seen` is the Entry's last modification
/// time as the editor read it, if the edit must not be saved over a newer version.
/// `Ok(false)` when the Entry was left as it was.
pub(crate) fn edit_entry(
    core: &Arc<AppCore>,
    vault: VaultId,
    entry: &EntryId,
    seen: Option<Timestamp>,
    edit: &EntryEdit,
) -> ApiResult<bool> {
    let entry = entry.resolve()?;
    core.edit(|workspace| {
        let vault = vault.resolve_open(workspace)?;
        Ok::<_, ApiError>(workspace.edit_entry(vault, entry, seen, edit)?)
    })
}

/// The Entry's `modified` as the editor had it, in the seconds [`seconds`] gives the UI. Entry
/// times are whole seconds, so any other number matches no version of the Entry.
fn seen_at(seen: f64) -> ApiResult<Timestamp> {
    let whole = seen as i64;
    if whole as f64 == seen {
        Ok(Timestamp::from_unix_seconds(whole))
    } else {
        Err(ApiError::entry_changed())
    }
}

/// Which Group each Entry of a Vault is in, with the path of Group names to it.
pub(crate) struct Places<'a> {
    groups: HashMap<core::EntryId, (&'a Group, Vec<&'a str>)>,
}

impl<'a> Places<'a> {
    pub fn of(vault: &'a core::Vault) -> Self {
        fn walk<'a>(
            group: &'a Group,
            path: &mut Vec<&'a str>,
            out: &mut HashMap<core::EntryId, (&'a Group, Vec<&'a str>)>,
        ) {
            path.push(group.name());
            for entry in group.entries() {
                out.insert(entry.id(), (group, path.clone()));
            }
            for child in group.groups() {
                walk(child, path, out);
            }
            path.pop();
        }
        let mut groups = HashMap::new();
        walk(vault.root(), &mut Vec::new(), &mut groups);
        Self { groups }
    }

    pub fn group(&self, entry: core::EntryId) -> Option<&'a Group> {
        self.groups.get(&entry).map(|(group, _)| *group)
    }

    fn path(&self, entry: core::EntryId) -> Vec<String> {
        self.groups
            .get(&entry)
            .map(|(_, path)| path.iter().map(|name| (*name).to_owned()).collect())
            .unwrap_or_default()
    }
}

pub(crate) fn summary(
    workspace: &Workspace,
    open: &ws::OpenVault,
    places: &Places,
    entry: &Entry,
) -> EntrySummary {
    let vault = open.id();
    let group = places.group(entry.id());
    let favicon_url = workspace
        .favicon(vault, entry.id())
        .and_then(|_| Site::of_entry_url(entry.url()))
        .map(|site| icon_protocol::favicon_url(vault, entry.id(), &site.origin()));
    EntrySummary {
        vault: vault.into(),
        id: entry.id().into(),
        title: title_of(entry),
        username: entry.username().to_owned(),
        label: None,
        url: entry.url().to_owned(),
        tags: entry.tags().to_vec(),
        color: entry.color().map(Into::into),
        icon: icon_view(vault, entry.icon(), open.vault()),
        favicon_url,
        expires: entry.expires().map(seconds),
        expired: workspace.is_expired(entry),
        created: entry.created().map(seconds),
        modified: entry.last_modified().map(seconds),
        has_password: !entry.password().expose_secret().is_empty(),
        has_totp: entry.totp().is_some(),
        attachment_count: entry.attachments().len() as u32,
        group: group
            .map_or_else(|| open.vault().root().id(), Group::id)
            .into(),
        group_name: group
            .map(|group| group.name().to_owned())
            .unwrap_or_default(),
        vault_name: open.display_name(),
        in_recycle_bin: group
            .is_some_and(|group| open.vault().is_group_in_recycle_bin(group.id()) == Ok(true)),
    }
}

pub(crate) fn totp_view(totp: Result<&Totp, keefast_core::TotpError>) -> TotpView {
    match totp {
        Ok(totp) => TotpView::Ready {
            digits: totp.digits(),
            period: totp.period(),
            algorithm: totp.algorithm().into(),
            steam: totp.is_steam(),
        },
        Err(error) => TotpView::Unreadable {
            reason: describe_totp(error).to_owned(),
        },
    }
}

pub(crate) fn custom_field_views(entry: &Entry) -> Vec<CustomFieldView> {
    entry
        .custom_fields()
        .iter()
        .map(custom_field_view)
        .collect()
}

fn custom_field_view(field: &CustomField) -> CustomFieldView {
    CustomFieldView {
        name: field.name.clone(),
        protected: field.protected,
        value: (!field.protected).then(|| field.value.expose_secret().to_owned()),
        is_empty: field.value.expose_secret().is_empty(),
        totp_storage: keefast_core::is_totp_field(&field.name),
    }
}

/// The facets Plugins find on an Entry. Their hooks run under fuel and memory limits, so this is
/// quick even with the Workspace held.
pub(crate) fn facet_views(plugins: &PluginHost, custom_fields: &[CustomField]) -> Vec<FacetView> {
    let fields: Vec<_> = custom_fields
        .iter()
        .map(|field| EntryField {
            name: &field.name,
            value: field.value.expose_secret(),
            protected: field.protected,
        })
        .collect();
    plugins
        .facets(&fields)
        .into_iter()
        .map(|found| facet_view(found, custom_fields))
        .collect()
}

fn facet_view(found: FoundFacet, custom_fields: &[CustomField]) -> FacetView {
    FacetView {
        plugin: found.plugin.id.clone(),
        facet: found.facet.id.clone(),
        title: found.facet.title.clone(),
        icon: found.facet.icon.clone(),
        label: found.label,
        can_copy: found.plugin.has(Capability::Copy),
        detect: found.facet.detect.fields.clone(),
        editor: found.facet.editor.as_ref().map(FacetEditorView::from),
        commands: found
            .commands
            .iter()
            .filter(|_| found.plugin.has(Capability::Copy))
            .map(|found| FacetCommandView {
                id: found.command.id.clone(),
                title: found.command.title.clone(),
                field: found.owned.key.clone(),
                shortcut: found.shortcut.map(str::to_owned),
            })
            .collect(),
        fields: found
            .fields
            .into_iter()
            .map(|field| {
                let kept = field.index.and_then(|index| custom_fields.get(index));
                let protected = kept.map_or(field.owned.protected, |kept| kept.protected);
                OwnedFieldView {
                    key: field.owned.key.clone(),
                    name: field.owned.name.clone(),
                    label: field.owned.label.clone(),
                    kind: field.owned.kind.into(),
                    index: field.index.map(|index| index as u32),
                    protected,
                    value: kept
                        .filter(|_| !protected)
                        .map(|kept| kept.value.expose_secret().to_owned()),
                    is_empty: kept.is_none_or(|kept| kept.value.expose_secret().is_empty()),
                    text: field.text,
                    placeholder: field.owned.placeholder.clone(),
                }
            })
            .collect(),
    }
}

/// An Entry of an unlocked Vault that isn't waiting to lock.
pub(crate) fn find<'a>(
    workspace: &'a Workspace,
    vault: VaultId,
    entry: &EntryId,
) -> ApiResult<(&'a ws::OpenVault, &'a Entry)> {
    let vault = vault.resolve_open(workspace)?;
    let open = workspace
        .readable_vault(vault)
        .ok_or_else(ApiError::vault_not_found)?;
    let entry = open
        .vault()
        .entry(entry.resolve()?)
        .ok_or_else(ApiError::not_found)?;
    Ok((open, entry))
}

/// The custom fields of an Entry with no username that a Plugin may derive its list subtitle
/// from, copied out so the Workspace can be let go of before the Plugin runs.
struct ListedFields(Vec<(String, SecretString, bool)>);

impl ListedFields {
    /// `None` when the list shows the Entry's username, or none of its fields is one a subtitle
    /// is derived from.
    fn of(plugins: &PluginHost, entry: &Entry) -> Option<Self> {
        if !entry.username().is_empty() {
            return None;
        }
        let fields: Vec<_> = entry
            .custom_fields()
            .iter()
            .filter(|field| plugins.lists_by(&field.name))
            .map(|field| (field.name.clone(), field.value.clone(), field.protected))
            .collect();
        (!fields.is_empty()).then_some(Self(fields))
    }

    fn label(&self, plugins: &PluginHost) -> Option<String> {
        let fields: Vec<_> = self
            .0
            .iter()
            .map(|(name, value, protected)| EntryField {
                name,
                value: value.expose_secret(),
                protected: *protected,
            })
            .collect();
        plugins.list_label(&fields)
    }
}

/// The Entry list. Plugins derive subtitles for Entries without a username after the Workspace
/// is let go of, so a slow `derive` never holds it.
#[tauri::command]
#[specta::specta]
pub async fn list_entries(core: Core<'_>, query: EntryQuery) -> ApiResult<Vec<EntrySummary>> {
    entry_list(&core, &query)
}

fn entry_list(core: &AppCore, query: &EntryQuery) -> ApiResult<Vec<EntrySummary>> {
    let plugins = core.plugins();
    let listed = core.read(|workspace| {
        let scope = query.scope.to_workspace(workspace)?;
        let search = EntrySearch::new(&query.search);
        let listed = workspace.find_entries(scope, &search, query.sort.into());
        let mut places: HashMap<ws::VaultId, Places> = HashMap::new();
        ApiResult::Ok(
            listed
                .iter()
                .filter_map(|listed| {
                    let open = workspace.vault(listed.vault)?;
                    let places = places
                        .entry(listed.vault)
                        .or_insert_with(|| Places::of(open.vault()));
                    Some((
                        summary(workspace, open, places, listed.entry),
                        ListedFields::of(&plugins, listed.entry),
                    ))
                })
                .collect::<Vec<_>>(),
        )
    })?;
    Ok(with_labels(&plugins, listed))
}

/// The summaries with the subtitles Plugins derive from their fields.
fn with_labels(
    plugins: &PluginHost,
    listed: Vec<(EntrySummary, Option<ListedFields>)>,
) -> Vec<EntrySummary> {
    listed
        .into_iter()
        .map(|(mut summary, fields)| {
            summary.label = fields.and_then(|fields| fields.label(plugins));
            summary
        })
        .collect()
}

/// The most Entries the command palette lists for a search.
const PALETTE_ENTRIES: usize = 5;

/// What the command palette lists for a search: how each of its commands' names matches, and
/// the best Entries outside the Recycle Bin of every unlocked Vault. The Entry search matches
/// both and scores them alike, so commands and Entries rank together.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PaletteMatches {
    /// For each name asked about, in order, how it matches, or `None` if it doesn't.
    pub names: Vec<Option<PaletteMatch>>,
    /// Best first, and equally good ones by title.
    pub entries: Vec<PaletteEntry>,
}

/// How a name, or an Entry, matched.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PaletteMatch {
    /// Higher is better.
    pub score: u32,
    /// The indexes, in UTF-16 code units, of the name's or title's characters matched, to
    /// highlight them.
    pub positions: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PaletteEntry {
    pub entry: EntrySummary,
    /// Its `positions` are empty when the search matched none of the title.
    pub found: PaletteMatch,
}

/// The command palette's search: the names of its commands, and the Entries.
#[tauri::command]
#[specta::specta]
pub async fn search_palette(
    core: Core<'_>,
    search: String,
    names: Vec<String>,
) -> ApiResult<PaletteMatches> {
    Ok(palette_matches(&core, &EntrySearch::new(&search), &names))
}

fn palette_matches(core: &AppCore, search: &EntrySearch, names: &[String]) -> PaletteMatches {
    let names = names
        .iter()
        .map(|name| {
            search.match_name(name).map(|found| PaletteMatch {
                score: found.score,
                positions: utf16_positions(name, &found.positions),
            })
        })
        .collect();
    let plugins = core.plugins();
    let listed = core.read(|workspace| {
        let mut places: HashMap<ws::VaultId, Places> = HashMap::new();
        workspace
            .find_entries(ws::EntryScope::AllVaults, search, EntrySort::Title)
            .iter()
            .filter_map(|listed| {
                let open = workspace.vault(listed.vault)?;
                let places = places
                    .entry(listed.vault)
                    .or_insert_with(|| Places::of(open.vault()));
                let summary = summary(workspace, open, places, listed.entry);
                (!summary.in_recycle_bin).then_some((summary, listed.entry))
            })
            .take(PALETTE_ENTRIES)
            .map(|(summary, entry)| {
                // The title shown is trimmed, or "(untitled)": only the Entry's own lines up.
                let positions = if summary.title == entry.title() {
                    utf16_positions(entry.title(), &search.highlights(entry.title()))
                } else {
                    Vec::new()
                };
                let score = search.relevance(entry).unwrap_or_default();
                let fields = ListedFields::of(&plugins, entry);
                ((summary, fields), PaletteMatch { score, positions })
            })
            .collect::<Vec<_>>()
    });
    let (listed, found): (Vec<_>, Vec<_>) = listed.into_iter().unzip();
    let entries = with_labels(&plugins, listed)
        .into_iter()
        .zip(found)
        .map(|(entry, found)| PaletteEntry { entry, found })
        .collect();
    PaletteMatches { names, entries }
}

/// Byte offsets in `text` as indexes in UTF-16 code units, as the UI slices strings.
fn utf16_positions(text: &str, offsets: &[usize]) -> Vec<u32> {
    offsets
        .iter()
        .map(|&offset| text[..offset].encode_utf16().count() as u32)
        .collect()
}

/// An Entry, without its password and protected custom fields.
#[tauri::command]
#[specta::specta]
pub async fn get_entry(core: Core<'_>, vault: VaultId, entry: EntryId) -> ApiResult<EntryDetail> {
    entry_detail(&core, vault, &entry)
}

fn entry_detail(core: &AppCore, vault: VaultId, entry: &EntryId) -> ApiResult<EntryDetail> {
    let plugins = core.plugins();
    core.read(|workspace| {
        let (open, found) = find(workspace, vault, entry)?;
        let places = Places::of(open.vault());
        let mut summary = summary(workspace, open, &places, found);
        // As the list shows it, from the same Plugins.
        summary.label = ListedFields::of(&plugins, found).and_then(|fields| fields.label(&plugins));
        Ok(EntryDetail {
            summary,
            raw_title: found.title().to_owned(),
            notes: found.notes().to_owned(),
            additional_urls: found.additional_urls().to_vec(),
            custom_fields: custom_field_views(found),
            facets: facet_views(&plugins, found.custom_fields()),
            attachments: found
                .attachments()
                .iter()
                .enumerate()
                .map(|(index, attachment)| AttachmentView {
                    index: index as u32,
                    name: attachment.name.clone(),
                    size: u32::try_from(attachment.size()).unwrap_or(u32::MAX),
                    preview: match attachment.preview() {
                        AttachmentPreview::Text(_) => PreviewKind::Text,
                        AttachmentPreview::Image(_) => PreviewKind::Image,
                        AttachmentPreview::None => PreviewKind::None,
                    },
                })
                .collect(),
            totp: found.totp().map(totp_view),
            strength: Some(found.password().expose_secret())
                .filter(|password| !password.is_empty())
                .map(|password| PasswordStrength::estimate(password).into()),
            history_count: found.history().len() as u32,
            remembered_access: found
                .remembered_access()
                .into_iter()
                .map(|access| RememberedAccessView {
                    host: access.host,
                    allowed: access.decision == AccessDecision::Allow,
                })
                .collect(),
            group_path: places.path(found.id()),
            deletes_for_good: open
                .vault()
                .deletes_entry_for_good(found.id())
                .unwrap_or(false),
        })
    })
}

/// An Entry's password, for the user to see. Copying doesn't need it: use `copy_field`.
#[tauri::command]
#[specta::specta]
pub async fn reveal_password(core: Core<'_>, vault: VaultId, entry: EntryId) -> ApiResult<String> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        Ok(found.password().expose_secret().to_owned())
    })
}

/// A custom field's value, protected or not, for the user to see. The field is asked for by
/// name, so a Merge that adds or reorders fields can't reveal another one.
#[tauri::command]
#[specta::specta]
pub async fn reveal_custom_field(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    name: String,
) -> ApiResult<String> {
    core.read(|workspace| custom_field_value(workspace, vault, &entry, &name))
}

fn custom_field_value(
    workspace: &Workspace,
    vault: VaultId,
    entry: &EntryId,
    name: &str,
) -> ApiResult<String> {
    let (_, found) = find(workspace, vault, entry)?;
    found
        .custom_fields()
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.expose_secret().to_owned())
        .ok_or_else(ApiError::not_found)
}

/// Makes a new Entry in `group`, and returns it.
#[tauri::command]
#[specta::specta]
pub async fn create_entry(
    core: Core<'_>,
    vault: VaultId,
    group: GroupId,
    input: EntryInput,
) -> ApiResult<EntryId> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let group = group.resolve()?;
    let fields = input.to_edit()?.apply(None)?;
    core.edit(|workspace| workspace.create_entry(vault, group, &fields))
        .map(Into::into)
}

/// Sets an Entry's fields, keeping the version it replaces in the Entry's history. Fields left
/// as they were record no history.
///
/// `seenModified` is the Entry's `modified` as the editor had it. If the Entry changed since,
/// as when a Merge brought in a change made elsewhere, nothing is saved and the error's kind is
/// `entryChanged`, so the editor never silently overwrites it. Without it the edit is saved over
/// whatever the Entry is now, and that version goes to the history.
#[tauri::command]
#[specta::specta]
pub async fn update_entry(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    input: EntryInput,
    seen_modified: Option<f64>,
) -> ApiResult<()> {
    update_entry_with(&core, vault, &entry, &input, seen_modified)
}

fn update_entry_with(
    core: &Arc<AppCore>,
    vault: VaultId,
    entry: &EntryId,
    input: &EntryInput,
    seen_modified: Option<f64>,
) -> ApiResult<()> {
    let edit = input.to_edit()?;
    let seen = seen_modified.map(seen_at).transpose()?;
    edit_entry(core, vault, entry, seen, &edit).map(|_| ())
}

/// Adds tags to an Entry, as the `+` beside its tags does, keeping the version it replaces in the
/// Entry's history. `text` is split on commas and semicolons as KeePass does; tags the Entry has
/// already change nothing.
#[tauri::command]
#[specta::specta]
pub async fn add_tags(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    text: String,
) -> ApiResult<()> {
    add_tags_to(&core, vault, &entry, &text)
}

fn add_tags_to(core: &Arc<AppCore>, vault: VaultId, entry: &EntryId, text: &str) -> ApiResult<()> {
    let edit = EntryEdit {
        tags: Some(TagsEdit::Add(text.to_owned())),
        ..EntryEdit::default()
    };
    edit_entry(core, vault, entry, None, &edit).map(|_| ())
}

/// Moves an Entry into another Group of its Vault.
#[tauri::command]
#[specta::specta]
pub async fn move_entry(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    to: GroupId,
) -> ApiResult<()> {
    let (vault, entry, to) = core.read(|workspace| {
        Ok::<_, ApiError>((
            vault.resolve_open(workspace)?,
            entry.resolve()?,
            to.resolve()?,
        ))
    })?;
    core.edit(|workspace| workspace.move_entry(vault, entry, to))
}

/// Moves an Entry to the Recycle Bin, or deletes it for good when the Vault has the Recycle Bin
/// disabled or the Entry is in it already. Ask first when `deletesForGood` says so.
#[tauri::command]
#[specta::specta]
pub async fn delete_entry(core: Core<'_>, vault: VaultId, entry: EntryId) -> ApiResult<()> {
    let (vault, entry) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, entry.resolve()?)))?;
    core.edit(|workspace| workspace.delete_entry(vault, entry))
}

/// Forgets the Access decision remembered on the Entry for `host`, or for every host when none
/// is given.
#[tauri::command]
#[specta::specta]
pub async fn forget_access(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    host: Option<String>,
) -> ApiResult<()> {
    let (vault, entry) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, entry.resolve()?)))?;
    core.edit(|workspace| workspace.forget_access(vault, entry, host.as_deref()))
}

/// Opens the Entry's URL in the user's browser. Only web addresses open: an address without a
/// scheme, such as `github.com`, opens as `https://`, and any other scheme (`ssh:`, `file:`…) is
/// refused, so a Vault file can't make keefast run something.
#[tauri::command]
#[specta::specta]
pub async fn open_entry_url(
    app: AppHandle,
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
) -> ApiResult<()> {
    let url = core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        web_address(found.url()).ok_or_else(|| {
            ApiError::invalid("keefast opens only web addresses, starting with http or https.")
        })
    })?;
    app.opener().open_url(url, None::<&str>).map_err(|error| {
        ApiError::new(ErrorKind::Unavailable, "The browser could not be opened.")
            .with_detail(error.to_string())
    })
}

/// The web address an Entry's URL is for, if it is one: an `http` or `https` URL as it is, and a
/// bare host (perhaps with a port and path) over `https`.
fn web_address(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
        return (web && !rest.is_empty()).then(|| url.to_owned());
    }
    // `mailto:x`, `javascript:x`, `C:\x` have a scheme (or drive) without slashes; only a port
    // may follow a bare host's colon.
    let port_or_nothing = url
        .split_once(':')
        .is_none_or(|(_, rest)| rest.starts_with(|c: char| c.is_ascii_digit()));
    port_or_nothing.then(|| format!("https://{url}"))
}

/// Chooses the colour a tag is drawn in, in the Vault, kept in its custom data; `None` goes back
/// to the colour the UI picks for the tag's name.
#[tauri::command]
#[specta::specta]
pub async fn set_tag_color(
    core: Core<'_>,
    vault: VaultId,
    tag: String,
    color: Option<Color>,
) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.edit(|workspace| workspace.set_tag_color(vault, &tag, color.map(Into::into)))
}

#[cfg(test)]
mod tests {
    use keefast_core::CustomField;
    use secrecy::{ExposeSecret, SecretString};

    use keefast_plugins::PluginHost;

    use super::{
        CustomFieldInput, EntryInput, EntryQuery, OwnedFieldKind, SortOrder, add_tags_to,
        custom_field_value, custom_field_view, entry_detail, entry_list, facet_views,
        update_entry_with, web_address,
    };
    use crate::core::AppCore;
    use crate::dto::{EntryId, IconChoice, VaultId};

    /// A core with a copy of the KeePassXC fixture open, and a new Entry made in it from
    /// `fields`. The temporary directory must live as long as the Vault.
    fn with_entry(
        fields: keefast_core::EntryFields,
    ) -> (std::sync::Arc<AppCore>, VaultId, EntryId, tempfile::TempDir) {
        use keefast_workspace::{UnlockSecrets, VaultLocation};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.kdbx");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/keepassxc-kdbx4.kdbx"
            ),
            &path,
        )
        .unwrap();
        let (core, _) = crate::core::tests::core();
        let (vault, entry) = core.update(|workspace| {
            let vault = workspace
                .open_vault(
                    &VaultLocation::Local(path.clone()),
                    &UnlockSecrets::password("fixture-password"),
                )
                .unwrap();
            let root = workspace.vault(vault).unwrap().vault().root().id();
            let entry = workspace.create_entry(vault, root, &fields).unwrap();
            (vault, entry)
        });
        (core, vault.into(), entry.into(), dir)
    }

    #[test]
    fn the_detail_has_the_passwords_strength_and_not_the_password() {
        use super::super::generator::StrengthScoreChoice;

        let (core, vault, entry, _dir) = with_entry(keefast_core::EntryFields {
            title: "Forum".into(),
            password: SecretString::from("Password123!"),
            ..Default::default()
        });
        let detail = entry_detail(&core, vault, &entry).unwrap();
        let strength = detail.strength.unwrap();
        assert_eq!(strength.score, StrengthScoreChoice::Weak);
        assert!(strength.bits < 45);
        let json = serde_json::to_string(&detail).unwrap();
        assert!(!json.contains("Password123!"));

        let (core, vault, entry, _dir) = with_entry(keefast_core::EntryFields {
            title: "No password".into(),
            ..Default::default()
        });
        assert_eq!(entry_detail(&core, vault, &entry).unwrap().strength, None);
    }

    #[test]
    fn tags_added_from_the_detail_join_the_entrys_own() {
        let (core, vault, entry, _dir) = with_entry(keefast_core::EntryFields {
            title: "GitHub".into(),
            tags: vec!["dev".into()],
            ..Default::default()
        });
        add_tags_to(&core, vault, &entry, "work, dev; ops ").unwrap();
        let detail = entry_detail(&core, vault, &entry).unwrap();
        assert_eq!(detail.summary.tags, ["dev", "work", "ops"]);

        let gone = EntryId::from(keefast_core::EntryId::from_hex(&"0".repeat(32)).unwrap());
        let error = add_tags_to(&core, vault, &gone, "work").unwrap_err();
        assert_eq!(error.kind, crate::error::ErrorKind::NotFound);
    }

    #[test]
    fn fields_keeping_totp_settings_are_marked() {
        let field = |name: &str, protected| CustomField {
            name: name.to_owned(),
            value: SecretString::from("JBSWY3DPEHPK3PXP"),
            protected,
        };
        for name in [
            "otp",
            "TOTP Seed",
            "TOTP Settings",
            "TimeOtp-Secret-Base32",
            "TimeOtp-Period",
        ] {
            assert!(custom_field_view(&field(name, true)).totp_storage, "{name}");
        }
        for name in ["PAT (ci)", "OTP", "TimeOtp", "Recovery codes"] {
            assert!(
                !custom_field_view(&field(name, false)).totp_storage,
                "{name}"
            );
        }
    }

    /// The Entry list over a copy of the KeePassXC fixture with three Entries added: a Card with
    /// no username, a Card with one, and a Card the Plugin can't label (no number).
    #[test]
    fn the_list_shows_a_cards_label_when_there_is_no_username() {
        use keefast_core::{EntryFields, GroupId};
        use keefast_workspace::{UnlockSecrets, VaultLocation};

        use crate::core::tests::core;
        use crate::dto::Scope;

        let field = |name: &str, value: &str, protected| CustomField {
            name: name.to_owned(),
            value: SecretString::from(value),
            protected,
        };
        let card = |title: &str, username: &str, number: &str| EntryFields {
            title: title.into(),
            username: username.into(),
            custom_fields: vec![
                field("Card number", number, true),
                field("Expiry", "08/31", false),
                field("CVV", "737", true),
                field("PIN", "8271", true),
            ],
            ..EntryFields::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cards.kdbx");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/keepassxc-kdbx4.kdbx"
            ),
            &path,
        )
        .unwrap();
        let (core, _) = core();
        core.update(|workspace| {
            let vault = workspace
                .open_vault(
                    &VaultLocation::Local(path.clone()),
                    &UnlockSecrets::password("fixture-password"),
                )
                .unwrap();
            let root: GroupId = workspace.vault(vault).unwrap().vault().root().id();
            for fields in [
                card("Company Visa", "", "4111 1111 1111 1111"),
                card("Nubank Ultravioleta", "arthur", "5162 9200 1234 5676"),
                card("Blank card", "", ""),
            ] {
                workspace.create_entry(vault, root, &fields).unwrap();
            }
        });

        let list = entry_list(
            &core,
            &EntryQuery {
                scope: Scope::AllVaults,
                search: String::new(),
                sort: SortOrder::Title,
            },
        )
        .unwrap();
        let shown = |title: &str| {
            let entry = list.iter().find(|entry| entry.title == title).unwrap();
            (entry.username.as_str(), entry.label.as_deref())
        };
        assert_eq!(shown("Company Visa"), ("", Some("Visa •••• 1111")));
        // A username wins: the Plugin isn't even asked.
        assert_eq!(shown("Nubank Ultravioleta"), ("arthur", None));
        assert_eq!(shown("Blank card"), ("", None));
        // Entries without a Card have no label.
        assert!(
            list.iter()
                .filter(|entry| !["Company Visa"].contains(&entry.title.as_str()))
                .all(|entry| entry.label.is_none())
        );

        // No protected value reaches the list, whole or in part beyond the label's last four.
        let json = serde_json::to_string(&list).unwrap();
        for secret in ["4111 1111", "5162 9200", "\"737\"", "\"8271\""] {
            assert!(!json.contains(secret), "{secret} is in the list");
        }
    }

    #[test]
    fn a_card_comes_with_its_label_and_without_its_protected_values() {
        let field = |name: &str, value: &str, protected| CustomField {
            name: name.to_owned(),
            value: SecretString::from(value),
            protected,
        };
        let fields = [
            field("Note", "hi", false),
            field("Card number", "4111 1111 1111 1111", true),
            field("Cardholder", "ARTHUR REZENDE", false),
            field("Expiry", "08/31", false),
            field("CVV", "", true),
        ];
        let facets = facet_views(&PluginHost::bundled(), &fields);
        assert_eq!(facets.len(), 1);
        let card = &facets[0];
        assert_eq!(
            (card.plugin.as_str(), card.facet.as_str()),
            ("keefast.card", "card")
        );
        assert_eq!(card.label.as_deref(), Some("Visa •••• 1111"));
        assert!(card.can_copy);
        let commands: Vec<_> = card
            .commands
            .iter()
            .map(|c| (c.title.as_str(), c.field.as_str(), c.shortcut.as_deref()))
            .collect();
        assert_eq!(
            commands,
            [
                ("Copy card number", "number", Some("Ctrl Shift C")),
                ("Copy CVV", "cvv", Some("Ctrl Shift V")),
                ("Copy expiry", "expiry", Some("Ctrl Shift E")),
            ]
        );

        let [number, holder, expiry, cvv, pin] = &card.fields[..] else {
            panic!("the Card has five fields");
        };
        assert_eq!(number.index, Some(1));
        assert_eq!(number.value, None, "protected");
        assert_eq!(number.text.as_deref(), Some("Visa •••• 1111"));
        assert_eq!(holder.value.as_deref(), Some("ARTHUR REZENDE"));
        assert_eq!(expiry.kind, OwnedFieldKind::MonthYear);
        assert!(cvv.protected && cvv.is_empty && cvv.value.is_none());
        assert_eq!((pin.index, pin.protected, pin.is_empty), (None, true, true));
        // Nothing in the DTO is a protected value.
        let json = serde_json::to_string(&facets).unwrap();
        assert!(!json.contains("4111 1111 1111 1111"));
    }

    /// Entry editor A saves a Card the user didn't touch as kept: `value: None` with its index,
    /// so its protected values never cross to the UI, and come back as they were.
    #[test]
    fn card_fields_kept_by_index_come_back_as_they_were() {
        use keefast_core::{CompositeKey, Encryption, EntryFields, Timestamp, Vault};

        let field = |name: &str, value: &str, protected| CustomField {
            name: name.to_owned(),
            value: SecretString::from(value),
            protected,
        };
        let now = Timestamp::from_unix_seconds(1_800_000_000);
        let mut vault = Vault::create(
            "Test",
            &CompositeKey::password("x"),
            &Encryption::default(),
            now,
        )
        .unwrap();
        let fields = EntryFields {
            title: "Company Visa".into(),
            custom_fields: vec![
                field("Note", "hi", false),
                field("Card number", "4111 1111 1111 1111", true),
                field("Expiry", "08/31", false),
                field("CVV", "123", true),
            ],
            ..EntryFields::default()
        };
        let id = vault.create_entry(vault.root().id(), &fields, now).unwrap();
        let entry = vault.entry(id).unwrap();

        let kept = |name: &str, original, protected| CustomFieldInput {
            name: name.to_owned(),
            value: None,
            protected,
            original: Some(original),
        };
        let input = EntryInput {
            title: "Company Visa (work)".into(),
            username: String::new(),
            password: None,
            url: String::new(),
            notes: String::new(),
            custom_fields: vec![
                kept("Note", 0, false),
                kept("Card number", 1, true),
                CustomFieldInput {
                    name: "Expiry".into(),
                    value: Some("09/31".into()),
                    protected: false,
                    original: None,
                },
                kept("CVV", 3, true),
            ],
            tags: Vec::new(),
            color: None,
            icon: IconChoice::Standard { number: 0 },
            expires: None,
        };
        let edited = input.to_edit().unwrap().apply(Some(entry)).unwrap();
        let shown: Vec<_> = edited
            .custom_fields
            .iter()
            .map(|f| (f.name.as_str(), f.value.expose_secret(), f.protected))
            .collect();
        assert_eq!(
            shown,
            [
                ("Note", "hi", false),
                ("Card number", "4111 1111 1111 1111", true),
                ("Expiry", "09/31", false),
                ("CVV", "123", true),
            ]
        );
    }

    #[test]
    fn web_addresses_open_as_they_are() {
        assert_eq!(
            web_address(" https://github.com/login "),
            Some("https://github.com/login".to_owned())
        );
        assert_eq!(
            web_address("HTTP://192.168.0.1"),
            Some("HTTP://192.168.0.1".to_owned())
        );
    }

    #[test]
    fn a_bare_host_opens_over_https() {
        assert_eq!(
            web_address("github.com"),
            Some("https://github.com".to_owned())
        );
        assert_eq!(
            web_address("example.com:8080/admin"),
            Some("https://example.com:8080/admin".to_owned())
        );
    }

    #[test]
    fn other_schemes_and_anything_not_an_address_are_refused() {
        for url in [
            "",
            "ssh://bastion.example",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "mailto:me@example.com",
            "C:\\Windows",
            "https://",
            "two words",
        ] {
            assert_eq!(web_address(url), None, "{url}");
        }
    }

    /// The command palette's search over a copy of the KeePassXC fixture with a few Entries
    /// added, one of them deleted to the Recycle Bin.
    #[test]
    fn the_palette_matches_commands_and_entries_alike() {
        use keefast_core::{EntryFields, EntrySearch, GroupId};
        use keefast_workspace::{UnlockSecrets, VaultLocation};

        use crate::core::tests::core;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("palette.kdbx");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/keepassxc-kdbx4.kdbx"
            ),
            &path,
        )
        .unwrap();
        let entry = |title: &str, username: &str| EntryFields {
            title: title.into(),
            username: username.into(),
            ..EntryFields::default()
        };
        let (core, _) = core();
        core.update(|workspace| {
            let vault = workspace
                .open_vault(
                    &VaultLocation::Local(path.clone()),
                    &UnlockSecrets::password("fixture-password"),
                )
                .unwrap();
            let root: GroupId = workspace.vault(vault).unwrap().vault().root().id();
            for fields in [entry("Zcopy pass", ""), entry("Zebra", "zcopy-pass")] {
                workspace.create_entry(vault, root, &fields).unwrap();
            }
            let gone = workspace
                .create_entry(vault, root, &entry("Zcopy pass old", ""))
                .unwrap();
            workspace.delete_entry(vault, gone).unwrap();
        });
        let search = |query: &str, names: &[&str]| {
            let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
            super::palette_matches(&core, &EntrySearch::new(query), &names)
        };

        // A command's name and an Entry's title score alike; positions count UTF-16 units.
        let found = search(
            "zcopy pass",
            &["Zcopy password", "🔑 Zcopy password", "Copy"],
        );
        let names: Vec<_> = found
            .names
            .iter()
            .map(|name| {
                name.as_ref()
                    .map(|name| (name.score, name.positions.clone()))
            })
            .collect();
        let title = &found.entries[0];
        assert_eq!(title.entry.title, "Zcopy pass");
        assert_eq!(title.found.positions, [0, 1, 2, 3, 4, 6, 7, 8, 9]);
        assert_eq!(
            names,
            [
                Some((title.found.score, vec![0, 1, 2, 3, 4, 6, 7, 8, 9])),
                Some((title.found.score - 1, vec![3, 4, 5, 6, 7, 9, 10, 11, 12])),
                None,
            ]
        );
        // An Entry found by its username only has no title to highlight, and scores lower; one
        // in the Recycle Bin isn't listed.
        let titles: Vec<_> = found
            .entries
            .iter()
            .map(|e| e.entry.title.as_str())
            .collect();
        assert_eq!(titles, ["Zcopy pass", "Zebra"]);
        assert!(found.entries[1].found.positions.is_empty());
        assert!(found.entries[1].found.score < title.found.score);

        // The starts of words, and small typos, match as they do in the Entry list.
        let found = search("zcp", &["Zcopy password"]);
        assert_eq!(found.names[0].as_ref().unwrap().positions, [0, 1, 6]);
        assert_eq!(found.entries[0].entry.title, "Zcopy pass");
        assert_eq!(search("zcopi", &[]).entries[0].entry.title, "Zcopy pass");
    }

    /// A Vault opened from a copy of the KeePassXC fixture, with a Bank Entry holding two
    /// protected custom fields, saved; and another device that edits its Vault file.
    struct Elsewhere {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        workspace: keefast_workspace::Workspace,
        vault: crate::dto::VaultId,
        bank: crate::dto::EntryId,
    }

    impl Elsewhere {
        fn new() -> Self {
            use keefast_workspace::{LocalStorage, UnlockSecrets, VaultLocation, Workspace};

            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("personal.kdbx");
            std::fs::copy(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../fixtures/keepassxc-kdbx4.kdbx"
                ),
                &path,
            )
            .unwrap();
            let mut workspace = Workspace::new(std::sync::Arc::new(LocalStorage));
            let vault = workspace
                .open_vault(
                    &VaultLocation::Local(path.clone()),
                    &UnlockSecrets::password("fixture-password"),
                )
                .unwrap();
            let root = workspace.vault(vault).unwrap().vault().root().id();
            let fields = keefast_core::EntryFields {
                title: "Bank".into(),
                password: SecretString::from("made-here"),
                custom_fields: vec![field("PIN", "1111", true), field("CVV", "737", true)],
                ..keefast_core::EntryFields::default()
            };
            let bank = workspace.create_entry(vault, root, &fields).unwrap();
            workspace.save_vault(vault).unwrap();
            Self {
                _dir: dir,
                path,
                workspace,
                vault: vault.into(),
                bank: bank.into(),
            }
        }

        /// Another device edits the Bank Entry, later than anything here, and saves the Vault
        /// file; then the Vault is saved here, which Merges the change in.
        fn edit_elsewhere(&mut self, edit: impl FnOnce(&mut keefast_core::EntryFields)) {
            use keefast_core::{CompositeKey, Timestamp, Vault};

            let key = CompositeKey::password("fixture-password");
            let mut theirs = Vault::open(&std::fs::read(&self.path).unwrap(), &key).unwrap();
            let bank = self.bank.resolve().unwrap();
            let mut fields = keefast_core::EntryFields::from(theirs.entry(bank).unwrap());
            edit(&mut fields);
            let later = Timestamp::from_unix_seconds(self.workspace.now().unix_seconds() + 600);
            theirs.update_entry(bank, &fields, later).unwrap();
            std::fs::write(&self.path, theirs.save().unwrap()).unwrap();
            let id = self.vault.resolve_open(&self.workspace).unwrap();
            self.workspace.save_vault(id).unwrap();
        }

        fn bank(&self) -> &keefast_core::Entry {
            super::find(&self.workspace, self.vault, &self.bank)
                .unwrap()
                .1
        }
    }

    fn field(name: &str, value: &str, protected: bool) -> CustomField {
        CustomField {
            name: name.to_owned(),
            value: SecretString::from(value),
            protected,
        }
    }

    /// What the editor saves: the Bank Entry with its title changed, everything else kept.
    fn retitled(title: &str) -> EntryInput {
        let kept = |name: &str, original| CustomFieldInput {
            name: name.into(),
            value: None,
            protected: true,
            original: Some(original),
        };
        EntryInput {
            title: title.into(),
            username: String::new(),
            password: None,
            url: String::new(),
            notes: String::new(),
            custom_fields: vec![kept("PIN", 0), kept("CVV", 1)],
            tags: Vec::new(),
            color: None,
            icon: IconChoice::Standard { number: 0 },
            expires: None,
        }
    }

    #[test]
    fn the_editor_keeps_what_it_never_revealed_and_is_refused_over_a_newer_version() {
        let (core, vault, entry, _dir) = with_entry(keefast_core::EntryFields {
            title: "Bank".into(),
            password: SecretString::from("made-here"),
            custom_fields: vec![field("PIN", "1111", true), field("CVV", "737", true)],
            ..Default::default()
        });
        let modified = || entry_detail(&core, vault, &entry).unwrap().summary.modified;
        let seen = modified().unwrap();

        // `seenModified` is matched to the second: anything else is a version it never saw.
        for stale in [seen + 1.0, seen - 0.5] {
            let error = update_entry_with(&core, vault, &entry, &retitled("Late"), Some(stale))
                .unwrap_err();
            assert_eq!(error.kind, crate::error::ErrorKind::EntryChanged, "{stale}");
            assert!(
                error
                    .message
                    .starts_with("This Entry changed while you were editing it")
            );
        }

        update_entry_with(&core, vault, &entry, &retitled("Bank (edited)"), Some(seen)).unwrap();
        let reveal = |name: &str| {
            core.read(|workspace| custom_field_value(workspace, vault, &entry, name).unwrap())
        };
        assert_eq!(reveal("PIN"), "1111");
        assert_eq!(reveal("CVV"), "737");
        let detail = entry_detail(&core, vault, &entry).unwrap();
        assert_eq!(detail.summary.title, "Bank (edited)");
        assert!(
            detail.summary.has_password,
            "the unrevealed password is kept"
        );

        let error = update_entry_with(
            &core,
            vault,
            &entry,
            &EntryInput {
                expires: Some("31/01/2027".into()),
                ..retitled("Bank (edited)")
            },
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind, crate::error::ErrorKind::Invalid);
        assert_eq!(
            error.message,
            "Type the expiry date as YYYY-MM-DD, such as 2027-01-31."
        );
    }

    #[test]
    fn an_attachment_is_removed_by_its_index_and_one_past_the_end_is_not_found() {
        use super::super::attachments::remove_attachment_from;
        use keefast_core::Attachment;

        let (core, vault, entry, _dir) = with_entry(keefast_core::EntryFields {
            title: "Bank".into(),
            attachments: vec![
                Attachment::new("codes.txt", b"1234".to_vec()),
                Attachment::new("key.pem", b"-----".to_vec()),
            ],
            ..Default::default()
        });

        remove_attachment_from(&core, vault, &entry, 0).unwrap();
        let detail = entry_detail(&core, vault, &entry).unwrap();
        let names: Vec<_> = detail.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["key.pem"]);

        let error = remove_attachment_from(&core, vault, &entry, 1).unwrap_err();
        assert_eq!(error.kind, crate::error::ErrorKind::NotFound);
    }

    #[test]
    fn a_custom_field_is_revealed_by_name_after_a_merge_adds_and_reorders_fields() {
        let mut h = Elsewhere::new();
        let reveal = |h: &Elsewhere, name: &str| {
            custom_field_value(&h.workspace, h.vault, &h.bank, name).unwrap()
        };
        assert_eq!(reveal(&h, "CVV"), "737");

        h.edit_elsewhere(|fields| {
            fields.custom_fields = vec![
                field("Recovery code", "r-e-c-o-v-e-r", true),
                field("CVV", "737", true),
                field("PIN", "1111", true),
            ];
        });

        assert_eq!(h.bank().custom_fields()[1].name, "CVV", "the fields moved");
        assert_eq!(reveal(&h, "CVV"), "737");
        assert_eq!(reveal(&h, "PIN"), "1111");
        assert_eq!(
            custom_field_value(&h.workspace, h.vault, &h.bank, "Gone")
                .unwrap_err()
                .kind,
            crate::error::ErrorKind::NotFound
        );
    }
}
