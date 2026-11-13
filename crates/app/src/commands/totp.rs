//! An Entry's TOTP: its code now, and setting it up.

use keefast_core::{Totp, TotpAlgorithm, TotpCode};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Core;
use super::entries::find;
use crate::dto::{EntryId, VaultId};
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TotpAlgorithmChoice {
    Sha1,
    Sha256,
    Sha512,
}

impl From<TotpAlgorithm> for TotpAlgorithmChoice {
    fn from(algorithm: TotpAlgorithm) -> Self {
        match algorithm {
            TotpAlgorithm::Sha1 => Self::Sha1,
            TotpAlgorithm::Sha256 => Self::Sha256,
            TotpAlgorithm::Sha512 => Self::Sha512,
        }
    }
}

impl From<TotpAlgorithmChoice> for TotpAlgorithm {
    fn from(algorithm: TotpAlgorithmChoice) -> Self {
        match algorithm {
            TotpAlgorithmChoice::Sha1 => Self::Sha1,
            TotpAlgorithmChoice::Sha256 => Self::Sha256,
            TotpAlgorithmChoice::Sha512 => Self::Sha512,
        }
    }
}

/// A TOTP code, with how long it stays valid and the code after it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TotpCodeView {
    pub code: String,
    /// The code that takes over when `remaining` runs out.
    pub next: String,
    /// Seconds until the next code, from 1 to the period.
    pub remaining: u32,
    pub period: u32,
}

impl From<TotpCode> for TotpCodeView {
    fn from(code: TotpCode) -> Self {
        Self {
            code: code.code().to_owned(),
            next: code.next().to_owned(),
            remaining: code.remaining(),
            period: code.period(),
        }
    }
}

/// TOTP settings as the setup form has them: an `otpauth://` URI, which brings its own settings,
/// or a Base32 secret with the settings given. Blank digits or period mean the usual 6 digits
/// and 30 seconds.
#[derive(Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TotpSetup {
    pub secret_or_uri: String,
    pub algorithm: TotpAlgorithmChoice,
    pub digits: String,
    pub period: String,
}

impl TotpSetup {
    fn to_core(&self) -> ApiResult<Totp> {
        Ok(Totp::from_setup(
            &self.secret_or_uri,
            self.algorithm.into(),
            &self.digits,
            &self.period,
        )?)
    }
}

/// The Entry's TOTP code now, or `None` when it has no TOTP or its settings can't be read (the
/// Entry's `totp` says why). Ask again when `remaining` runs out.
#[tauri::command]
#[specta::specta]
pub async fn get_totp_code(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
) -> ApiResult<Option<TotpCodeView>> {
    core.read(|workspace| {
        let (open, found) = find(workspace, vault, &entry)?;
        Ok(workspace
            .totp_code(open.id(), found.id())
            .and_then(Result::ok)
            .map(Into::into))
    })
}

/// The Entry's TOTP settings for the setup form, secret included; `None` without TOTP. Steam
/// TOTPs, which have no settings by hand, come as their URI.
#[tauri::command]
#[specta::specta]
pub async fn reveal_totp_setup(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
) -> ApiResult<Option<TotpSetup>> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        let Some(Ok(totp)) = found.totp() else {
            return Ok(None);
        };
        let secret = match totp.is_steam() {
            true => totp.to_uri(found.title(), found.username()),
            false => totp.secret(),
        };
        Ok(Some(TotpSetup {
            secret_or_uri: secret.expose_secret().to_owned(),
            algorithm: totp.algorithm().into(),
            digits: totp.digits().to_string(),
            period: totp.period().to_string(),
        }))
    })
}

/// The code TOTP settings make now, to check them before saving; an error says what is wrong.
#[tauri::command]
#[specta::specta]
pub async fn preview_totp(core: Core<'_>, setup: TotpSetup) -> ApiResult<TotpCodeView> {
    let totp = setup.to_core()?;
    let now = core.read(|workspace| workspace.now());
    Ok(totp.code(now).into())
}

/// Sets up TOTP on an Entry, keeping the version it replaces in the Entry's history.
#[tauri::command]
#[specta::specta]
pub async fn set_totp(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    setup: TotpSetup,
) -> ApiResult<()> {
    let totp = setup.to_core()?;
    let (vault, entry) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, entry.resolve()?)))?;
    core.edit(|workspace| workspace.set_totp(vault, entry, Some(&totp)))
}

/// Removes an Entry's TOTP, keeping the version it replaces in the Entry's history.
#[tauri::command]
#[specta::specta]
pub async fn remove_totp(core: Core<'_>, vault: VaultId, entry: EntryId) -> ApiResult<()> {
    let (vault, entry) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, entry.resolve()?)))?;
    core.edit(|workspace| workspace.set_totp(vault, entry, None))
}
