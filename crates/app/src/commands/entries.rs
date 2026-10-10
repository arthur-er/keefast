//! The Entry list and the Entries themselves: reading, creating, editing, moving, deleting.
//!
//! Passwords and protected custom fields never leave in an Entry's DTO: the UI asks for one with
//! `reveal_password` or `reveal_custom_field` when the user shows it, and edits send only what
//! the user changed, so an untouched secret never crosses the boundary.

use std::collections::HashMap;

use keefast_core::{
    self as core, AccessDecision, AttachmentPreview, CustomField, Entry, EntryFields, EntrySearch,
    EntrySort, Group, Site, Timestamp, Totp, parse_tags,
};
use keefast_workspace::{self as ws, Workspace};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::Core;
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
    pub attachments: Vec<AttachmentView>,
    pub totp: Option<TotpView>,
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
    /// The Entry's fields after this edit, `current` being the Entry as it is, if it exists.
    fn to_fields(&self, current: Option<&Entry>) -> ApiResult<EntryFields> {
        let base = current.map(EntryFields::from).unwrap_or_default();
        let password = match &self.password {
            Some(password) => SecretString::from(password.as_str()),
            None => base.password.clone(),
        };
        let custom_fields = self
            .custom_fields
            .iter()
            .map(|field| {
                let value = match (&field.value, field.original) {
                    (Some(value), _) => SecretString::from(value.as_str()),
                    (None, Some(original)) => base
                        .custom_fields
                        .get(original as usize)
                        .map(|kept| kept.value.clone())
                        .ok_or_else(ApiError::not_found)?,
                    (None, None) => SecretString::from(String::new()),
                };
                Ok(CustomField {
                    name: field.name.trim().to_owned(),
                    value,
                    protected: field.protected,
                })
            })
            .collect::<ApiResult<Vec<_>>>()?;
        let expires = match &self.expires {
            None => None,
            Some(date) => Some(
                Timestamp::expiry_on(date, current.and_then(Entry::expires)).ok_or_else(|| {
                    ApiError::invalid("Type the expiry date as YYYY-MM-DD, such as 2027-01-31.")
                })?,
            ),
        };
        Ok(EntryFields {
            title: self.title.clone(),
            username: self.username.clone(),
            password,
            url: self.url.clone(),
            notes: self.notes.clone(),
            custom_fields,
            tags: self.tags.iter().flat_map(|tag| parse_tags(tag)).collect(),
            color: self.color.map(Into::into),
            icon: self.icon.to_core()?,
            expires,
            attachments: base.attachments,
        })
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
        .map(|field| CustomFieldView {
            name: field.name.clone(),
            protected: field.protected,
            value: (!field.protected).then(|| field.value.expose_secret().to_owned()),
            is_empty: field.value.expose_secret().is_empty(),
        })
        .collect()
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

/// The Entry list.
#[tauri::command]
#[specta::specta]
pub async fn list_entries(core: Core<'_>, query: EntryQuery) -> ApiResult<Vec<EntrySummary>> {
    core.read(|workspace| {
        let scope = query.scope.to_workspace(workspace)?;
        let search = EntrySearch::new(&query.search);
        let listed = workspace.find_entries(scope, &search, query.sort.into());
        let mut places: HashMap<ws::VaultId, Places> = HashMap::new();
        Ok(listed
            .iter()
            .filter_map(|listed| {
                let open = workspace.vault(listed.vault)?;
                let places = places
                    .entry(listed.vault)
                    .or_insert_with(|| Places::of(open.vault()));
                Some(summary(workspace, open, places, listed.entry))
            })
            .collect())
    })
}

/// An Entry, without its password and protected custom fields.
#[tauri::command]
#[specta::specta]
pub async fn get_entry(core: Core<'_>, vault: VaultId, entry: EntryId) -> ApiResult<EntryDetail> {
    core.read(|workspace| {
        let (open, found) = find(workspace, vault, &entry)?;
        let places = Places::of(open.vault());
        Ok(EntryDetail {
            summary: summary(workspace, open, &places, found),
            raw_title: found.title().to_owned(),
            notes: found.notes().to_owned(),
            additional_urls: found.additional_urls().to_vec(),
            custom_fields: custom_field_views(found),
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

/// A custom field's value, protected or not, for the user to see.
#[tauri::command]
#[specta::specta]
pub async fn reveal_custom_field(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    index: u32,
) -> ApiResult<String> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        found
            .custom_fields()
            .get(index as usize)
            .map(|field| field.value.expose_secret().to_owned())
            .ok_or_else(ApiError::not_found)
    })
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
    let (vault, group, fields) = core.read(|workspace| {
        Ok::<_, ApiError>((
            vault.resolve_open(workspace)?,
            group.resolve()?,
            input.to_fields(None)?,
        ))
    })?;
    core.edit(|workspace| workspace.create_entry(vault, group, &fields))
        .map(Into::into)
}

/// Sets an Entry's fields, keeping the version it replaces in the Entry's history. Fields left
/// as they were record no history.
#[tauri::command]
#[specta::specta]
pub async fn update_entry(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    input: EntryInput,
) -> ApiResult<()> {
    let (vault, entry, fields) = core.read(|workspace| {
        let (open, found) = find(workspace, vault, &entry)?;
        Ok::<_, ApiError>((open.id(), found.id(), input.to_fields(Some(found))?))
    })?;
    core.edit(|workspace| workspace.update_entry(vault, entry, &fields))
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

#[cfg(test)]
mod tests {
    use super::web_address;

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
}
