//! A Vault's own settings: its master key and encryption, and its browser-integration settings,
//! which live in the Vault file (ADR 0005).

use keefast_core::{DefaultAccessPolicy, KeyParts, Pairing};
use keefast_workspace::VaultLocation;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::Core;
use super::vaults::{EncryptionSettings, KeyInput};
use super::workspace::{SaveState, SyncState};
use crate::browser;
use crate::core::Failures;
use crate::dto::{GroupId, LocationView, VaultId, seconds};
use crate::error::{ApiError, ApiResult, ErrorKind};

/// What a Vault answers a site asking for an Entry that has no Access decision for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AccessPolicy {
    Ask,
    AlwaysAllow,
    AlwaysDeny,
}

impl From<DefaultAccessPolicy> for AccessPolicy {
    fn from(policy: DefaultAccessPolicy) -> Self {
        match policy {
            DefaultAccessPolicy::Ask => Self::Ask,
            DefaultAccessPolicy::AlwaysAllow => Self::AlwaysAllow,
            DefaultAccessPolicy::AlwaysDeny => Self::AlwaysDeny,
        }
    }
}

impl From<AccessPolicy> for DefaultAccessPolicy {
    fn from(policy: AccessPolicy) -> Self {
        match policy {
            AccessPolicy::Ask => Self::Ask,
            AccessPolicy::AlwaysAllow => Self::AlwaysAllow,
            AccessPolicy::AlwaysDeny => Self::AlwaysDeny,
        }
    }
}

/// What a Vault's master key is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MasterKeyParts {
    Password,
    KeyFile,
    PasswordAndKeyFile,
}

impl From<KeyParts> for MasterKeyParts {
    fn from(parts: KeyParts) -> Self {
        match parts {
            KeyParts::Password => Self::Password,
            KeyParts::KeyFile => Self::KeyFile,
            KeyParts::PasswordAndKeyFile => Self::PasswordAndKeyFile,
        }
    }
}

/// A browser paired with a Vault.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    pub name: String,
    /// When it was paired, in seconds since 1970, if the Vault knows.
    pub created: Option<f64>,
    /// When the browser last asked the Vault for anything, in seconds since 1970, if it has
    /// since keefast started recording it.
    pub last_used: Option<f64>,
}

impl From<Pairing> for PairingView {
    fn from(pairing: Pairing) -> Self {
        Self {
            name: pairing.name,
            created: pairing.created.map(seconds),
            last_used: pairing.last_used.map(seconds),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettings {
    pub vault: VaultId,
    /// The Vault's name, or its Vault file's when it has none.
    pub name: String,
    pub location: LocationView,
    pub save_status: SaveState,
    pub sync_status: Option<SyncState>,
    pub master_key: MasterKeyParts,
    pub encryption: EncryptionSettings,
    /// Whether deleting moves Entries and Groups to the Recycle Bin.
    pub recycle_bin_enabled: bool,
    pub default_access_policy: AccessPolicy,
    /// The browsers paired with the Vault, in Vault file order.
    pub pairings: Vec<PairingView>,
    pub browser_group: Option<GroupId>,
    pub browser_group_name: Option<String>,
}

/// A new master key (or `None` to keep the Vault's) and the encryption to save the Vault with.
#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChangeKeyInput {
    pub new_key: Option<KeyInput>,
    pub encryption: EncryptionSettings,
}

#[tauri::command]
#[specta::specta]
pub async fn get_vault_settings(core: Core<'_>, vault: VaultId) -> ApiResult<VaultSettings> {
    core.read(|workspace| {
        let id = vault.resolve_open(workspace)?;
        let open = workspace
            .readable_vault(id)
            .ok_or_else(ApiError::vault_not_found)?;
        let save_status = if open.is_saving() {
            SaveState::Saving
        } else if open.save_failed() {
            SaveState::Failed
        } else if open.has_unsaved_changes() {
            SaveState::Unsaved
        } else {
            SaveState::Saved
        };
        let browser_group = open.vault().browser_group();
        Ok(VaultSettings {
            vault,
            name: open.display_name(),
            location: open.location().into(),
            save_status,
            sync_status: open.sync_status().map(Into::into),
            master_key: open.vault().key_parts().into(),
            encryption: open.vault().encryption().into(),
            recycle_bin_enabled: open.vault().recycle_bin_enabled(),
            default_access_policy: open.vault().default_access_policy().into(),
            pairings: workspace
                .pairings(id)
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            browser_group: browser_group.map(Into::into),
            browser_group_name: browser_group
                .and_then(|group| open.vault().group(group))
                .map(|group| group.name().to_owned()),
        })
    })
}

/// Gives the Vault a new master key, if one is given, and the encryption, and saves it now. The
/// Vault keeps its old key until the save has written the new one (ADR 0006). Check that the
/// user typed the password the same twice before calling.
#[tauri::command]
#[specta::specta]
pub async fn change_vault_key(
    core: Core<'_>,
    vault: VaultId,
    input: ChangeKeyInput,
) -> ApiResult<()> {
    let encryption = input.encryption.to_core()?;
    let secrets = input.new_key.map(KeyInput::into_secrets);
    let vault = core.update(|workspace| {
        let vault = vault.resolve_open(workspace)?;
        if let Some(secrets) = &secrets {
            workspace.change_master_key(vault, secrets)?;
        }
        workspace.set_encryption(vault, &encryption)?;
        ApiResult::Ok(vault)
    })?;
    let save = core.update(|workspace| workspace.begin_save(vault).ok());
    core.run_saves(save.into_iter().collect(), Failures::Announce);
    Ok(())
}

/// Renames the Vault. An empty name names it after its Vault file again.
#[tauri::command]
#[specta::specta]
pub async fn rename_vault(core: Core<'_>, vault: VaultId, name: String) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.edit(|workspace| workspace.rename_vault(vault, &name))
}

/// Turns the Vault's Recycle Bin on or off: off, deleting deletes for good.
#[tauri::command]
#[specta::specta]
pub async fn set_recycle_bin_enabled(
    core: Core<'_>,
    vault: VaultId,
    enabled: bool,
) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.edit(|workspace| workspace.set_recycle_bin_enabled(vault, enabled))
}

/// Shows the Vault file in the system's file manager. Only a Vault file on this machine can be.
#[tauri::command]
#[specta::specta]
pub async fn show_vault_file_in_folder(
    app: AppHandle,
    core: Core<'_>,
    vault: VaultId,
) -> ApiResult<()> {
    let location = core.read(|workspace| {
        let id = vault.resolve_open(workspace)?;
        let open = workspace.vault(id).ok_or_else(ApiError::vault_not_found)?;
        ApiResult::Ok(open.location().clone())
    })?;
    let VaultLocation::Local(path) = location else {
        return Err(ApiError::invalid(
            "Only a Vault file on this computer can be shown in a folder.",
        ));
    };
    app.opener().reveal_item_in_dir(&path).map_err(|error| {
        ApiError::new(ErrorKind::Unavailable, "The folder could not be shown.")
            .with_detail(error.to_string())
    })
}

/// Sets the Vault's Default access policy, answering the browser requests it settles.
#[tauri::command]
#[specta::specta]
pub async fn set_default_access_policy(
    core: Core<'_>,
    vault: VaultId,
    policy: AccessPolicy,
) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let replies =
        core.edit(|workspace| workspace.set_default_access_policy(vault, policy.into()))?;
    browser::send(&core, replies);
    Ok(())
}

/// Revokes the Vault's Pairing named `name`: the browser holding its key gets nothing from the
/// Vault any more. Ask the user first.
#[tauri::command]
#[specta::specta]
pub async fn revoke_pairing(core: Core<'_>, vault: VaultId, name: String) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let replies = core.edit(|workspace| workspace.revoke_pairing(vault, &name))?;
    browser::send(&core, replies);
    Ok(())
}
