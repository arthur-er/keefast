//! The shapes that cross the IPC boundary, as the web UI sees them (camelCase, typed for it by
//! specta), and their conversions to and from the Workspace's types.
//!
//! Nothing secret is in a DTO unless a `reveal_*` command asks for it: passwords and protected
//! custom fields stay in the Workspace, and copying happens on this side (ADR 0008).

use std::path::PathBuf;

use keefast_core::{self as core, Entry, EntryColor, EntryIcon, Timestamp};
use keefast_workspace::{self as ws, VaultLocation, Workspace};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{ApiError, ApiResult};

/// A Vault, unlocked or locked, by the number it keeps while the app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct VaultId(pub u32);

/// An Entry, by its UUID in hex. Unique within its Vault.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct EntryId(pub String);

/// A Group, by its UUID in hex. Unique within its Vault.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupId(pub String);

/// A custom icon of a Vault, by its UUID in hex.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct IconId(pub String);

impl From<ws::VaultId> for VaultId {
    fn from(id: ws::VaultId) -> Self {
        Self(u32::try_from(id.as_u64()).unwrap_or(u32::MAX))
    }
}

impl From<core::EntryId> for EntryId {
    fn from(id: core::EntryId) -> Self {
        Self(id.to_hex())
    }
}

impl From<core::GroupId> for GroupId {
    fn from(id: core::GroupId) -> Self {
        Self(id.to_hex())
    }
}

impl From<core::CustomIconId> for IconId {
    fn from(id: core::CustomIconId) -> Self {
        Self(id.to_hex())
    }
}

impl VaultId {
    /// The open Vault, unlocked or locked, this names.
    pub fn resolve(self, workspace: &Workspace) -> ApiResult<ws::VaultId> {
        let wanted = u64::from(self.0);
        workspace
            .vaults()
            .map(|open| open.id())
            .chain(workspace.locked_vaults().map(|locked| locked.id()))
            .find(|id| id.as_u64() == wanted)
            .ok_or_else(ApiError::vault_not_found)
    }

    /// The unlocked Vault this names.
    pub fn resolve_open(self, workspace: &Workspace) -> ApiResult<ws::VaultId> {
        let wanted = u64::from(self.0);
        workspace
            .vaults()
            .map(|open| open.id())
            .find(|id| id.as_u64() == wanted)
            .ok_or_else(ApiError::vault_not_found)
    }
}

impl EntryId {
    pub fn resolve(&self) -> ApiResult<core::EntryId> {
        core::EntryId::from_hex(&self.0).ok_or_else(ApiError::not_found)
    }
}

impl GroupId {
    pub fn resolve(&self) -> ApiResult<core::GroupId> {
        core::GroupId::from_hex(&self.0).ok_or_else(ApiError::not_found)
    }
}

impl IconId {
    pub fn resolve(&self) -> ApiResult<core::CustomIconId> {
        core::CustomIconId::from_hex(&self.0).ok_or_else(ApiError::not_found)
    }
}

/// An Entry of a Vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryKey {
    pub vault: VaultId,
    pub entry: EntryId,
}

/// Which Storage provider holds a Vault file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Local,
    Dropbox,
}

/// Where a Vault file is: a local path, or a path in the connected Dropbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub provider: Provider,
    pub path: String,
}

/// A Vault file's location with the names to show for it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LocationView {
    pub provider: Provider,
    pub path: String,
    /// The Vault file's name without extension, as the Vault is named.
    pub name: String,
    /// Where it is, in full, saying which Storage provider holds it when it isn't local.
    pub description: String,
}

impl From<&VaultLocation> for Location {
    fn from(location: &VaultLocation) -> Self {
        match location {
            VaultLocation::Local(path) => Self {
                provider: Provider::Local,
                path: path.to_string_lossy().into_owned(),
            },
            VaultLocation::Dropbox(path) => Self {
                provider: Provider::Dropbox,
                path: path.clone(),
            },
        }
    }
}

impl From<&VaultLocation> for LocationView {
    fn from(location: &VaultLocation) -> Self {
        let Location { provider, path } = location.into();
        let description = match location {
            VaultLocation::Local(_) => location.description(),
            VaultLocation::Dropbox(path) => format!("Dropbox: {path}"),
        };
        Self {
            provider,
            path,
            name: location.display_name(),
            description,
        }
    }
}

impl Location {
    pub fn to_workspace(&self) -> VaultLocation {
        match self.provider {
            Provider::Local => VaultLocation::Local(absolute(&self.path)),
            Provider::Dropbox => VaultLocation::Dropbox(self.path.clone()),
        }
    }
}

/// A typed path made absolute, with `~` for the home directory, as the GPUI app read paths.
pub fn absolute(text: &str) -> PathBuf {
    let text = text.trim();
    let path = match (text.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(text),
    };
    std::path::absolute(&path).unwrap_or(path)
}

/// The colours an Entry can be marked with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Color {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Violet,
}

impl From<EntryColor> for Color {
    fn from(color: EntryColor) -> Self {
        match color {
            EntryColor::Red => Self::Red,
            EntryColor::Orange => Self::Orange,
            EntryColor::Yellow => Self::Yellow,
            EntryColor::Green => Self::Green,
            EntryColor::Blue => Self::Blue,
            EntryColor::Violet => Self::Violet,
        }
    }
}

impl From<Color> for EntryColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Red => Self::Red,
            Color::Orange => Self::Orange,
            Color::Yellow => Self::Yellow,
            Color::Green => Self::Green,
            Color::Blue => Self::Blue,
            Color::Violet => Self::Violet,
        }
    }
}

/// What the Entry list shows: every open Vault, one Vault, one Group, or the Entries of every
/// open Vault with a tag or colour (outside the Recycle Bin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Scope {
    AllVaults,
    Vault { vault: VaultId },
    Group { vault: VaultId, group: GroupId },
    Tag { tag: String },
    Color { color: Color },
}

impl Scope {
    pub fn from_workspace(scope: &ws::EntryScope) -> Self {
        match scope {
            ws::EntryScope::AllVaults => Self::AllVaults,
            ws::EntryScope::Vault(vault) => Self::Vault {
                vault: (*vault).into(),
            },
            ws::EntryScope::Group(vault, group) => Self::Group {
                vault: (*vault).into(),
                group: (*group).into(),
            },
            ws::EntryScope::Tag(tag) => Self::Tag { tag: tag.clone() },
            ws::EntryScope::Color(color) => Self::Color {
                color: (*color).into(),
            },
        }
    }

    pub fn to_workspace(&self, workspace: &Workspace) -> ApiResult<ws::EntryScope> {
        Ok(match self {
            Self::AllVaults => ws::EntryScope::AllVaults,
            Self::Vault { vault } => ws::EntryScope::Vault(vault.resolve(workspace)?),
            Self::Group { vault, group } => {
                ws::EntryScope::Group(vault.resolve(workspace)?, group.resolve()?)
            }
            Self::Tag { tag } => ws::EntryScope::Tag(tag.clone()),
            Self::Color { color } => ws::EntryScope::Color((*color).into()),
        })
    }
}

/// Where the user is: what the Entry list shows and the Entry selected, kept by the Workspace so
/// unlocking a Vault returns there. Browsers pair with the Vault shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub scope: Scope,
    pub entry: Option<EntryKey>,
}

impl View {
    pub fn from_workspace(view: &ws::View) -> Self {
        Self {
            scope: Scope::from_workspace(&view.scope),
            entry: view.entry.map(|(vault, entry)| EntryKey {
                vault: vault.into(),
                entry: entry.into(),
            }),
        }
    }

    pub fn to_workspace(&self, workspace: &Workspace) -> ApiResult<ws::View> {
        let entry = match &self.entry {
            Some(key) => Some((key.vault.resolve(workspace)?, key.entry.resolve()?)),
            None => None,
        };
        Ok(ws::View {
            scope: self.scope.to_workspace(workspace)?,
            entry,
        })
    }
}

/// An Entry's icon: one of KeePass's standard icons, by number, or a custom icon of the Vault,
/// with the URL to draw it from.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum IconView {
    Standard { number: u32 },
    Custom { id: IconId, url: String },
}

/// An icon to give an Entry.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum IconChoice {
    Standard { number: u32 },
    Custom { id: IconId },
}

impl IconChoice {
    pub fn to_core(&self) -> ApiResult<EntryIcon> {
        Ok(match self {
            Self::Standard { number } => EntryIcon::Standard(*number),
            Self::Custom { id } => EntryIcon::Custom(id.resolve()?),
        })
    }
}

/// The standard icon of a key, KeePass's first, which a custom icon the Vault hasn't got shows as.
const KEY_ICON: u32 = 0;

/// The standard icon of a folder, which a Group's custom icon the Vault hasn't got shows as.
const FOLDER_ICON: u32 = 48;

pub fn icon_view(vault: ws::VaultId, icon: EntryIcon, in_vault: &core::Vault) -> IconView {
    icon_view_or(vault, icon, in_vault, KEY_ICON)
}

/// A Group's icon, as [`icon_view`] gives an Entry's.
pub fn group_icon_view(vault: ws::VaultId, icon: EntryIcon, in_vault: &core::Vault) -> IconView {
    icon_view_or(vault, icon, in_vault, FOLDER_ICON)
}

/// The icon, served through `kficon:` when custom, or `missing` when the Vault hasn't got it.
fn icon_view_or(
    vault: ws::VaultId,
    icon: EntryIcon,
    in_vault: &core::Vault,
    missing: u32,
) -> IconView {
    match icon {
        EntryIcon::Standard(number) => IconView::Standard {
            number: number.min(core::STANDARD_ICON_COUNT - 1),
        },
        EntryIcon::Custom(id) if in_vault.custom_icon(id).is_some() => IconView::Custom {
            id: id.into(),
            url: crate::icon_protocol::custom_icon_url(vault, id),
        },
        EntryIcon::Custom(_) => IconView::Standard { number: missing },
    }
}

/// Seconds since 1970-01-01 UTC, as JavaScript's `Date` takes them (times 1000).
pub fn seconds(time: Timestamp) -> f64 {
    time.unix_seconds() as f64
}

/// What an Entry is called: its title, or "(untitled)".
pub fn title_of(entry: &Entry) -> String {
    match entry.title().trim() {
        "" => "(untitled)".to_owned(),
        title => title.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_path_is_made_absolute_with_the_home_directory_for_a_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(absolute("~/Vaults/a.kdbx"), home.join("Vaults/a.kdbx"));
        assert!(absolute("relative.kdbx").is_absolute());
        #[cfg(unix)]
        assert_eq!(absolute(" /tmp/x.kdbx "), PathBuf::from("/tmp/x.kdbx"));
    }

    #[test]
    fn a_dropbox_location_says_so() {
        let view = LocationView::from(&VaultLocation::Dropbox("/Apps/Work.kdbx".into()));
        assert_eq!(view.name, "Work");
        assert_eq!(view.description, "Dropbox: /Apps/Work.kdbx");
        assert_eq!(view.provider, Provider::Dropbox);
    }
}
