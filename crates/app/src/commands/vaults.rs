//! Opening, creating, locking and saving Vaults, and the recent Vault files.

use std::path::{Path, PathBuf};

use keefast_core::{Argon2, Cipher, Encryption, Kdf};
use keefast_workspace::{
    EntryScope, NewVault, OpenVaultError, UnlockSecrets, UnsavedLock, VaultLocation, View,
    Workspace, write_new_key_file,
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use super::{Core, blocking, home, pick_file, pick_new_file};
use crate::background;
use crate::core::Failures;
use crate::dto::{Location, VaultId, absolute};
use crate::error::{ApiError, ApiResult, ErrorKind};

/// The cipher a Vault file is encrypted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CipherChoice {
    Aes256,
    ChaCha20,
    Twofish,
}

/// How the master key is turned into the key that encrypts the Vault file. Argon2's memory is
/// in KiB, as KDBX keeps it; AES-KDF's rounds are counted up to 4294967295.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum KdfChoice {
    Argon2id {
        memory_kib: u32,
        iterations: u32,
        parallelism: u32,
    },
    Argon2d {
        memory_kib: u32,
        iterations: u32,
        parallelism: u32,
    },
    AesKdf {
        rounds: u32,
    },
}

/// The cipher and key derivation a Vault file is encrypted with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionSettings {
    pub cipher: CipherChoice,
    pub kdf: KdfChoice,
}

impl From<Encryption> for EncryptionSettings {
    fn from(encryption: Encryption) -> Self {
        let argon2 = |argon2: Argon2| {
            (
                u32::try_from(argon2.memory / 1024).unwrap_or(u32::MAX),
                u32::try_from(argon2.iterations).unwrap_or(u32::MAX),
                argon2.parallelism,
            )
        };
        Self {
            cipher: match encryption.cipher {
                Cipher::Aes256 => CipherChoice::Aes256,
                Cipher::ChaCha20 => CipherChoice::ChaCha20,
                Cipher::Twofish => CipherChoice::Twofish,
            },
            kdf: match encryption.kdf {
                Kdf::Argon2id(settings) => {
                    let (memory_kib, iterations, parallelism) = argon2(settings);
                    KdfChoice::Argon2id {
                        memory_kib,
                        iterations,
                        parallelism,
                    }
                }
                Kdf::Argon2d(settings) => {
                    let (memory_kib, iterations, parallelism) = argon2(settings);
                    KdfChoice::Argon2d {
                        memory_kib,
                        iterations,
                        parallelism,
                    }
                }
                Kdf::AesKdf { rounds } => KdfChoice::AesKdf {
                    rounds: u32::try_from(rounds).unwrap_or(u32::MAX),
                },
            },
        }
    }
}

impl EncryptionSettings {
    /// The encryption these settings describe, checked as key derivation would.
    pub fn to_core(self) -> ApiResult<Encryption> {
        let argon2 = |memory_kib: u32, iterations: u32, parallelism: u32| Argon2 {
            memory: u64::from(memory_kib) * 1024,
            iterations: u64::from(iterations),
            parallelism,
        };
        let kdf = match self.kdf {
            KdfChoice::Argon2id {
                memory_kib,
                iterations,
                parallelism,
            } => Kdf::Argon2id(argon2(memory_kib, iterations, parallelism)),
            KdfChoice::Argon2d {
                memory_kib,
                iterations,
                parallelism,
            } => Kdf::Argon2d(argon2(memory_kib, iterations, parallelism)),
            KdfChoice::AesKdf { rounds } => Kdf::AesKdf {
                rounds: u64::from(rounds),
            },
        };
        let encryption = Encryption {
            cipher: match self.cipher {
                CipherChoice::Aes256 => Cipher::Aes256,
                CipherChoice::ChaCha20 => Cipher::ChaCha20,
                CipherChoice::Twofish => Cipher::Twofish,
            },
            kdf,
        };
        encryption.check()?;
        Ok(encryption)
    }
}

/// A master key: a master password, a Key file, or both. The password is wrapped in a
/// `SecretString` as soon as it arrives.
#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyInput {
    pub password: String,
    pub key_file: Option<String>,
}

impl KeyInput {
    pub fn into_secrets(self) -> UnlockSecrets {
        let KeyInput { password, key_file } = self;
        UnlockSecrets::new(
            SecretString::from(password),
            key_file
                .filter(|path| !path.trim().is_empty())
                .map(|path| absolute(&path)),
        )
    }
}

/// What a new Vault is made with.
#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewVaultInput {
    /// Where the new Vault file goes; `~` is the home directory.
    pub path: String,
    /// The name of the Vault's top Group; the Vault file's name when empty.
    pub name: String,
    pub key: KeyInput,
    pub encryption: EncryptionSettings,
}

/// The user's answer when a Vault to lock has unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LockAnswer {
    /// Save the changes, then lock.
    Save,
    /// Lock without saving: the changes are lost.
    Discard,
    /// Don't lock the Vault (only when the user asked for the lock).
    Cancel,
}

/// The encryption keefast gives new Vaults: AES-256 with Argon2id at its default cost.
#[tauri::command]
#[specta::specta]
pub async fn default_encryption() -> ApiResult<EncryptionSettings> {
    Ok(Encryption::default().into())
}

/// Asks the user for a Vault file to open.
#[tauri::command]
#[specta::specta]
pub async fn choose_vault_file(app: AppHandle) -> ApiResult<Option<String>> {
    let path = pick_file(&app, "Open Vault file").await?;
    Ok(path.map(|path| path.to_string_lossy().into_owned()))
}

/// Asks the user for a Key file.
#[tauri::command]
#[specta::specta]
pub async fn choose_key_file(app: AppHandle) -> ApiResult<Option<String>> {
    let path = pick_file(&app, "Choose Key file").await?;
    Ok(path.map(|path| path.to_string_lossy().into_owned()))
}

/// Asks the user where to save a new Vault file, starting by `near` (a path typed so far), else
/// in the home directory.
#[tauri::command]
#[specta::specta]
pub async fn choose_new_vault_file(
    app: AppHandle,
    near: Option<String>,
) -> ApiResult<Option<String>> {
    let directory = directory_of(near.as_deref());
    let path = pick_new_file(&app, "New Vault file", directory, "Vault.kdbx").await?;
    Ok(path.map(|path| path.to_string_lossy().into_owned()))
}

/// Asks the user where to save a new Key file, starting by `near` (a Vault file's path), writes
/// a new random key there in KeePass's XML v2.0 format, and returns where. An existing file is
/// never replaced.
#[tauri::command]
#[specta::specta]
pub async fn create_key_file(app: AppHandle, near: Option<String>) -> ApiResult<Option<String>> {
    let directory = directory_of(near.as_deref());
    let Some(path) = pick_new_file(&app, "New Key file", directory, "Vault.keyx").await? else {
        return Ok(None);
    };
    let written = path.clone();
    blocking(move || write_new_key_file(&written))
        .await?
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => ApiError::new(
                ErrorKind::Conflict,
                "A file is already there. keefast never replaces a file with a new Key file.",
            ),
            _ => ApiError::new(ErrorKind::Io, "The Key file could not be written there.")
                .with_detail(error.to_string()),
        })?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Where file dialogs start: by `near`, else in the home directory.
fn directory_of(near: Option<&str>) -> Option<PathBuf> {
    near.filter(|near| !near.trim().is_empty())
        .map(absolute)
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(home)
}

/// Opens a Vault file, or unlocks its Vault if it is locked, deriving the key off the main
/// thread. A Vault file whose Vault is open already is just shown. `None` when the OS locked or
/// suspended while the key was derived: the Vault stays as it was, for the user to unlock
/// again.
#[tauri::command]
#[specta::specta]
pub async fn unlock_vault(
    core: Core<'_>,
    location: Location,
    key: KeyInput,
) -> ApiResult<Option<VaultId>> {
    let secrets = key.into_secrets();
    let location = location.to_workspace();
    let pending = core.read(|workspace| workspace.begin_open(location, secrets));
    let pending = match pending {
        Ok(pending) => pending,
        Err(OpenVaultError::AlreadyOpen(vault)) => {
            core.update(|workspace| workspace.show(View::from(EntryScope::Vault(vault))));
            return Ok(Some(vault.into()));
        }
        Err(error) => return Err(error.into()),
    };
    let unlocked = blocking(move || pending.unlock()).await??;
    Ok(core
        .update(|workspace| workspace.add_unlocked(unlocked))
        .map(Into::into))
}

/// Writes a new Vault file, never over an existing file, deriving the key off the main thread,
/// then opens and shows its Vault.
#[tauri::command]
#[specta::specta]
pub async fn create_vault(core: Core<'_>, input: NewVaultInput) -> ApiResult<VaultId> {
    let NewVaultInput {
        path,
        name,
        key,
        encryption,
    } = input;
    if path.trim().is_empty() {
        return Err(ApiError::invalid("Choose where to save the Vault file."));
    }
    let new = NewVault {
        name,
        secrets: key.into_secrets(),
        encryption: encryption.to_core()?,
    };
    let location = VaultLocation::Local(absolute(&path));
    let pending = core.read(|workspace| workspace.begin_create(location, new))?;
    let created = blocking(move || pending.create()).await??;
    Ok(core
        .update(|workspace| workspace.add_unlocked(created))
        .expect("a Vault just created is always added")
        .into())
}

/// Forgets a recent Vault file.
#[tauri::command]
#[specta::specta]
pub async fn forget_recent(core: Core<'_>, location: Location) -> ApiResult<()> {
    let location = location.to_workspace();
    core.update(|workspace| workspace.forget_recent(&location));
    Ok(())
}

/// The global Lock: locks every Vault, and clears what keefast copied. Vaults with unsaved
/// changes are saved first with autosave on, and otherwise wait for the user's answer
/// (`pendingLock` in the snapshot).
#[tauri::command]
#[specta::specta]
pub async fn lock_all(core: Core<'_>) -> ApiResult<()> {
    core.lock_with(|workspace, clipboard| workspace.lock_all(clipboard));
    Ok(())
}

/// Locks one Vault, like `lock_all`.
#[tauri::command]
#[specta::specta]
pub async fn lock_vault(core: Core<'_>, vault: VaultId) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    core.lock_with(|workspace, clipboard| workspace.lock_vault(vault, clipboard));
    Ok(())
}

/// Answers whether to save a Vault's unsaved changes before it locks. If saving fails, the user
/// is asked again.
#[tauri::command]
#[specta::specta]
pub async fn answer_lock(core: Core<'_>, vault: VaultId, answer: LockAnswer) -> ApiResult<()> {
    let vault = core.read(|workspace| vault.resolve_open(workspace))?;
    let answer = match answer {
        LockAnswer::Save => UnsavedLock::Save,
        LockAnswer::Discard => UnsavedLock::Discard,
        LockAnswer::Cancel => UnsavedLock::Cancel,
    };
    let save = core.update(|workspace| workspace.answer_lock(vault, answer));
    core.run_saves(save.into_iter().collect(), Failures::Announce);
    Ok(())
}

/// Saves every Vault with unsaved changes or a failed last save.
#[tauri::command]
#[specta::specta]
pub async fn save_all(core: Core<'_>) -> ApiResult<()> {
    let saves = core.update(Workspace::begin_saves);
    core.run_saves(saves, Failures::Announce);
    Ok(())
}

/// Saves one Vault now, unless it is being saved already (it is saved again once that is done).
#[tauri::command]
#[specta::specta]
pub async fn save_vault(core: Core<'_>, vault: VaultId) -> ApiResult<()> {
    let save = core.update(|workspace| {
        let vault = vault.resolve_open(workspace)?;
        Ok::<_, ApiError>(workspace.begin_save(vault).ok())
    })?;
    core.run_saves(save.into_iter().collect(), Failures::Announce);
    Ok(())
}

/// Checks every Dropbox Vault for changes made elsewhere now, and uploads saves made offline.
#[tauri::command]
#[specta::specta]
pub async fn sync_now(core: Core<'_>) -> ApiResult<()> {
    background::sync_now(core.inner());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_settings_go_both_ways_and_are_checked() {
        let settings = EncryptionSettings::from(Encryption::default());
        assert_eq!(settings.to_core().unwrap(), Encryption::default());

        let no_iterations = EncryptionSettings {
            kdf: KdfChoice::Argon2d {
                memory_kib: 65536,
                iterations: 0,
                parallelism: 2,
            },
            ..settings
        };
        assert_eq!(
            no_iterations.to_core().unwrap_err().message,
            "Argon2 needs at least 1 iteration."
        );

        let no_rounds = EncryptionSettings {
            kdf: KdfChoice::AesKdf { rounds: 0 },
            ..settings
        };
        assert_eq!(no_rounds.to_core().unwrap_err().kind, ErrorKind::Invalid);
    }
}
