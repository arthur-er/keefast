//! The workspace snapshot the UI draws its shell from, and where the user is.

use keefast_core::{DEFAULT_BROWSER_GROUP, Group};
use keefast_workspace::{
    self as ws, LoginTarget as WsLoginTarget, SaveStatus, SyncStatus, Workspace,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Core;
use crate::core::AppCore;
use crate::dto::{
    Color, EntryId, GroupId, IconView, Location, LocationView, Scope, VaultId, View,
    group_icon_view, title_of,
};
use crate::error::ApiResult;

/// Everything the shell shows, in one answer: the Vaults, where the user is, the save status,
/// and what waits for the user's answer.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    /// The revision of the last `WorkspaceChanged`, as of this snapshot.
    pub revision: u32,
    /// Unlocked and locked Vaults, in the order they were opened.
    pub vaults: Vec<VaultSummary>,
    pub view: View,
    pub save_status: SaveState,
    /// The locked Vault to offer for unlocking first: the one the user was looking at, or else
    /// the first locked.
    pub vault_to_unlock: Option<VaultId>,
    /// Recent Vault files that aren't open, most recent first: those offered for reopening.
    pub recent_vaults: Vec<LocationView>,
    /// Every tag of the Entries outside the Recycle Bin in the open Vaults.
    pub tags: Vec<String>,
    /// A Vault waiting to lock until the user says what to do with its unsaved changes.
    pub pending_lock: Option<PendingLock>,
    pub pending_pairing: Option<PendingPairing>,
    pub pending_access: Option<PendingAccess>,
    pub pending_login: Option<PendingLogin>,
    /// Seconds until what keefast copied is cleared from the clipboard, if it will be.
    pub clipboard_clears_in: Option<f64>,
    /// Whether this build can reach Dropbox (it has an app key).
    pub dropbox_available: bool,
    /// Why the app config couldn't be read or saved, if it couldn't: recent Vault files and
    /// settings may then be forgotten.
    pub config_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SaveState {
    Saving,
    Failed,
    Unsaved,
    Saved,
}

impl From<SaveStatus> for SaveState {
    fn from(status: SaveStatus) -> Self {
        match status {
            SaveStatus::Saving => Self::Saving,
            SaveStatus::Failed => Self::Failed,
            SaveStatus::Unsaved => Self::Unsaved,
            SaveStatus::Saved => Self::Saved,
        }
    }
}

/// Where a Dropbox Vault stands with its Vault file in Dropbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SyncState {
    Synced,
    Syncing,
    Offline,
    Error,
}

impl From<SyncStatus> for SyncState {
    fn from(status: SyncStatus) -> Self {
        match status {
            SyncStatus::Synced => Self::Synced,
            SyncStatus::Syncing => Self::Syncing,
            SyncStatus::Offline => Self::Offline,
            SyncStatus::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum VaultState {
    Unlocked,
    /// Waiting to lock until its unsaved changes are saved or discarded: nothing in it shows.
    Locking,
    Locked,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummary {
    pub id: VaultId,
    pub name: String,
    pub location: LocationView,
    pub state: VaultState,
    pub unsaved_changes: bool,
    pub saving: bool,
    pub save_failed: bool,
    /// `None` for a local Vault file.
    pub sync_status: Option<SyncState>,
    /// Entries in the Vault, outside the Recycle Bin too; 0 unless unlocked.
    pub entry_count: u32,
    pub root_group: Option<GroupId>,
    pub recycle_bin: Option<GroupId>,
    pub browser_group: Option<GroupId>,
    /// How many Entries each of its Smart lists has; 0 unless unlocked.
    pub smart_lists: SmartListCounts,
    /// The colours chosen for its tags, by tag; empty unless unlocked. A tag without one is drawn
    /// in a colour picked from its name.
    pub tag_colors: Vec<TagColor>,
}

/// The colour chosen for a tag in a Vault.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TagColor {
    pub tag: String,
    pub color: Color,
}

/// How many Entries a Vault's Smart lists have, as the sidebar shows them.
#[derive(Debug, Clone, Copy, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SmartListCounts {
    pub with_totp: u32,
    pub recently_modified: u32,
    pub expired: u32,
}

impl SmartListCounts {
    fn of(workspace: &Workspace, vault: ws::VaultId) -> Self {
        let count = |list| workspace.smart_list(vault, list).len() as u32;
        Self {
            with_totp: count(ws::SmartList::WithTotp),
            recently_modified: count(ws::SmartList::RecentlyModified),
            expired: count(ws::SmartList::Expired),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingLock {
    pub vault: VaultId,
    pub vault_name: String,
    /// Whether the user may call the lock off: not when idle time, the OS locking or a suspend
    /// asked for it, since whoever is at the keyboard may not be the user.
    pub can_cancel: bool,
}

/// A pending browser request, by number. Answer the one in the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RequestId(pub u32);

impl RequestId {
    fn of(id: u64) -> Self {
        Self(u32::try_from(id).unwrap_or(u32::MAX))
    }

    pub fn matches(self, id: u64) -> bool {
        u64::from(self.0) == id
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairing {
    pub id: RequestId,
    pub vault: VaultId,
    pub vault_name: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingAccess {
    pub id: RequestId,
    /// The host of the site asking.
    pub site: String,
    pub entries: Vec<AskedEntry>,
}

/// An Entry a browser asks for or could update.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AskedEntry {
    pub vault: VaultId,
    pub entry: EntryId,
    pub title: String,
    pub username: String,
    pub vault_name: String,
}

/// Where a captured login goes: a new Entry in a Group (or in the Vault's Browser group, made if
/// it has none, when `group` is `None`), or an existing Entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LoginTarget {
    NewEntry {
        vault: VaultId,
        group: Option<GroupId>,
    },
    Update {
        vault: VaultId,
        entry: EntryId,
    },
}

impl LoginTarget {
    fn from_workspace(target: WsLoginTarget) -> Self {
        match target {
            WsLoginTarget::NewEntry { vault, group } => Self::NewEntry {
                vault: vault.into(),
                group: group.map(Into::into),
            },
            WsLoginTarget::Update { vault, entry } => Self::Update {
                vault: vault.into(),
                entry: entry.into(),
            },
        }
    }

    pub fn to_workspace(&self, workspace: &Workspace) -> ApiResult<WsLoginTarget> {
        Ok(match self {
            Self::NewEntry { vault, group } => WsLoginTarget::NewEntry {
                vault: vault.resolve_open(workspace)?,
                group: group.as_ref().map(GroupId::resolve).transpose()?,
            },
            Self::Update { vault, entry } => WsLoginTarget::Update {
                vault: vault.resolve_open(workspace)?,
                entry: entry.resolve()?,
            },
        })
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingLogin {
    pub id: RequestId,
    /// The host of the site the login was submitted on.
    pub site: String,
    pub username: String,
    /// The places it can go, as a list: headings (without a target), the Entries it could
    /// update, then every Group of each Vault it may go to.
    pub options: Vec<LoginOption>,
    /// Where to offer to save it.
    pub suggested: LoginTarget,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LoginOption {
    /// `None` for a heading.
    pub target: Option<LoginTarget>,
    pub label: String,
    /// Shown muted after the label: a Vault's name, or that it is the Browser group.
    pub detail: Option<String>,
    /// A Group's icon; `None` for headings, Entries, Vaults and a Group still to be made.
    pub icon: Option<IconView>,
    pub depth: u32,
}

/// What the command line asked to open first.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchInfo {
    pub vault_file: Option<Location>,
    pub key_file: Option<String>,
}

/// A Group of a Vault, as where new Entries and Groups go.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupKey {
    pub vault: VaultId,
    pub group: GroupId,
}

pub fn snapshot(workspace: &Workspace, revision: u32) -> WorkspaceSnapshot {
    let mut vaults: Vec<VaultSummary> = workspace
        .vaults()
        .map(|open| {
            let locking = open.is_locking();
            let vault = open.vault();
            VaultSummary {
                id: open.id().into(),
                name: open.display_name(),
                location: open.location().into(),
                state: match locking {
                    true => VaultState::Locking,
                    false => VaultState::Unlocked,
                },
                unsaved_changes: open.has_unsaved_changes(),
                saving: open.is_saving(),
                save_failed: open.save_failed(),
                sync_status: open.sync_status().map(Into::into),
                entry_count: match locking {
                    true => 0,
                    false => vault.root().all_entries().count() as u32,
                },
                root_group: (!locking).then(|| vault.root().id().into()),
                recycle_bin: vault.recycle_bin().filter(|_| !locking).map(Into::into),
                browser_group: vault.browser_group().filter(|_| !locking).map(Into::into),
                smart_lists: SmartListCounts::of(workspace, open.id()),
                tag_colors: match locking {
                    true => Vec::new(),
                    false => vault
                        .tag_colors()
                        .into_iter()
                        .map(|(tag, color)| TagColor {
                            tag,
                            color: color.into(),
                        })
                        .collect(),
                },
            }
        })
        .chain(workspace.locked_vaults().map(|locked| VaultSummary {
            id: locked.id().into(),
            name: locked.display_name(),
            location: locked.location().into(),
            state: VaultState::Locked,
            unsaved_changes: false,
            saving: false,
            save_failed: false,
            sync_status: None,
            entry_count: 0,
            root_group: None,
            recycle_bin: None,
            browser_group: None,
            smart_lists: SmartListCounts::default(),
            tag_colors: Vec::new(),
        }))
        .collect();
    vaults.sort_by_key(|vault| vault.id.0);
    WorkspaceSnapshot {
        revision,
        vaults,
        view: View::from_workspace(&workspace.view()),
        save_status: workspace.save_status().into(),
        vault_to_unlock: workspace.vault_to_unlock().map(Into::into),
        recent_vaults: workspace.closed_recent_vaults().map(Into::into).collect(),
        tags: workspace.tags().into_iter().map(str::to_owned).collect(),
        pending_lock: workspace.pending_lock().map(|vault| PendingLock {
            vault: vault.into(),
            vault_name: crate::core::vault_name(workspace, vault),
            can_cancel: workspace.can_cancel_lock(vault),
        }),
        pending_pairing: workspace.pending_pairing().map(|pending| PendingPairing {
            id: RequestId::of(pending.id.as_u64()),
            vault: pending.vault.into(),
            vault_name: crate::core::vault_name(workspace, pending.vault),
        }),
        pending_access: workspace.pending_access().map(|pending| PendingAccess {
            id: RequestId::of(pending.id.as_u64()),
            site: pending.site,
            entries: asked_entries(workspace, &pending.entries),
        }),
        pending_login: workspace.pending_login().map(|pending| PendingLogin {
            id: RequestId::of(pending.id.as_u64()),
            options: login_options(workspace, &pending),
            suggested: LoginTarget::from_workspace(pending.suggested),
            site: pending.site,
            username: pending.username,
        }),
        clipboard_clears_in: workspace
            .clipboard_clears_in()
            .map(|wait| wait.as_secs_f64()),
        dropbox_available: workspace.dropbox().is_some(),
        config_error: workspace.config_error().map(ToString::to_string),
    }
}

fn asked_entries(
    workspace: &Workspace,
    entries: &[(ws::VaultId, keefast_core::EntryId)],
) -> Vec<AskedEntry> {
    entries
        .iter()
        .filter_map(|&(vault, entry)| {
            let open = workspace.vault(vault)?;
            let found = open.vault().entry(entry)?;
            Some(AskedEntry {
                vault: vault.into(),
                entry: entry.into(),
                title: title_of(found),
                username: found.username().to_owned(),
                vault_name: open.display_name(),
            })
        })
        .collect()
}

/// The places a captured login can go: the Entries it could update, then every Group of each
/// Vault it may go to, with the Browser group to be made when a Vault has none yet.
fn login_options(workspace: &Workspace, pending: &ws::PendingLogin) -> Vec<LoginOption> {
    let heading = |label: &str| LoginOption {
        target: None,
        label: label.to_owned(),
        detail: None,
        icon: None,
        depth: 0,
    };
    let mut options = Vec::new();
    if !pending.entries.is_empty() {
        options.push(heading("Update an existing Entry"));
    }
    for &(vault, entry) in &pending.entries {
        let Some(open) = workspace.vault(vault) else {
            continue;
        };
        let Some(found) = open.vault().entry(entry) else {
            continue;
        };
        options.push(LoginOption {
            target: Some(LoginTarget::Update {
                vault: vault.into(),
                entry: entry.into(),
            }),
            label: format!("{} · {}", title_of(found), found.username()),
            detail: Some(open.display_name()),
            icon: None,
            depth: 1,
        });
    }
    options.push(heading("Save as a new Entry in"));
    for &vault in &pending.vaults {
        let Some(open) = workspace.vault(vault) else {
            continue;
        };
        let browser_group = open.vault().browser_group();
        let new_entry = |group: Option<keefast_core::GroupId>| {
            Some(LoginTarget::NewEntry {
                vault: vault.into(),
                group: group.map(Into::into),
            })
        };
        let root = open.vault().root();
        options.push(LoginOption {
            target: new_entry(Some(root.id())),
            label: open.display_name(),
            detail: None,
            icon: None,
            depth: 1,
        });
        if browser_group.is_none() {
            options.push(LoginOption {
                target: new_entry(None),
                label: format!("{DEFAULT_BROWSER_GROUP} (new Group)"),
                detail: Some("Browser group".to_owned()),
                icon: None,
                depth: 2,
            });
        }
        let mut groups = Vec::new();
        flatten_groups(root.groups(), 2, &mut groups);
        for (group, depth) in groups {
            if open.vault().is_group_in_recycle_bin(group.id()) != Ok(false) {
                continue;
            }
            options.push(LoginOption {
                target: new_entry(Some(group.id())),
                label: group.name().to_owned(),
                detail: (browser_group == Some(group.id())).then(|| "Browser group".to_owned()),
                icon: Some(group_icon_view(vault, group.icon(), open.vault())),
                depth,
            });
        }
    }
    options
}

/// Every Group in `groups` and below, depth first, with its depth.
pub fn flatten_groups<'a>(groups: &'a [Group], depth: u32, out: &mut Vec<(&'a Group, u32)>) {
    for group in groups {
        out.push((group, depth));
        flatten_groups(group.groups(), depth + 1, out);
    }
}

/// Everything the shell shows. Refetch it on `WorkspaceChanged`.
#[tauri::command]
#[specta::specta]
pub async fn get_workspace(core: Core<'_>) -> ApiResult<WorkspaceSnapshot> {
    // The revision is read with the Workspace held, so it is never newer than the snapshot.
    Ok(core.read(|workspace| snapshot(workspace, core.revision())))
}

/// What the command line asked to open first: a Vault file, and its Key file.
#[tauri::command]
#[specta::specta]
pub async fn get_launch(core: Core<'_>) -> ApiResult<LaunchInfo> {
    let launch = core.launch();
    Ok(LaunchInfo {
        vault_file: launch
            .vault_file
            .clone()
            .map(|path| (&ws::VaultLocation::Local(path)).into()),
        key_file: launch
            .key_file
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
    })
}

/// Tells the Workspace where the user is, to return them there when a Vault they were in is
/// unlocked again. Browsers pair with the Vault shown.
///
/// Answers with a revision that snapshots show the view from: a snapshot with an older revision
/// may have been read before the view changed, so the UI keeps showing the view it asked for
/// until one this new arrives.
#[tauri::command]
#[specta::specta]
pub async fn set_view(core: Core<'_>, view: View) -> ApiResult<u32> {
    show_view(&core, view)
}

fn show_view(core: &AppCore, view: View) -> ApiResult<u32> {
    core.update(|workspace| -> ApiResult<()> {
        let view = view.to_workspace(workspace)?;
        workspace.show(view);
        Ok(())
    })?;
    // The change's revision, or a later one: either way the snapshot has the view in it.
    Ok(core.revision())
}

/// Tells the Workspace the user gave keefast input, which postpones the idle lock. The UI calls
/// it on keys, clicks and scrolling, at most every few seconds.
#[tauri::command]
#[specta::specta]
pub async fn note_activity(core: Core<'_>) -> ApiResult<()> {
    // Activity changes nothing the UI shows, so the UI isn't told.
    core.update_quietly(Workspace::note_activity);
    Ok(())
}

/// Where new Entries and Groups go for what the Entry list shows: the shown Group, or the top
/// Group of the shown Vault. With all Vaults (or a tag or colour) shown there is none, unless
/// only one Vault is open.
#[tauri::command]
#[specta::specta]
pub async fn group_for_new_items(core: Core<'_>, scope: Scope) -> ApiResult<Option<GroupKey>> {
    core.read(|workspace| {
        let scope = scope.to_workspace(workspace)?;
        Ok(workspace
            .group_for_new_items(scope)
            .map(|(vault, group)| GroupKey {
                vault: vault.into(),
                group: group.into(),
            }))
    })
}

#[cfg(test)]
mod tests {
    use super::{show_view, snapshot};
    use crate::core::tests::core;
    use crate::dto::{Color, Scope, View};

    #[test]
    fn snapshots_show_the_view_from_the_revision_set_view_answers() {
        let (core, _) = core();
        core.update(|workspace| workspace.note_activity());
        let view = View {
            scope: Scope::Color { color: Color::Red },
            entry: None,
        };

        let revision = show_view(&core, view.clone()).unwrap();

        assert_eq!(revision, 2);
        let shown = core.read(|workspace| snapshot(workspace, core.revision()));
        assert!(shown.revision >= revision);
        assert_eq!(shown.view, view);
    }
}
