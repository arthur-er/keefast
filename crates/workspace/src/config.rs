//! The app config: machine-local preferences, kept as TOML in the OS config directory. It never
//! holds secrets (ADR 0005).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use keefast_core::{CharacterRecipe, PassphraseRecipe};

use crate::atomic_file;
use crate::generator::{GeneratorMode, GeneratorSettings};
use crate::storage::VaultLocation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    /// Vault files opened lately, most recent first.
    pub recent_vaults: Vec<VaultLocation>,
    pub autosave: bool,
    /// Whether browser extensions may reach keefast through its native-messaging host.
    pub browser_integration: bool,
    pub generator: GeneratorSettings,
    /// The Vault file the user last saved a captured login to, offered first next time.
    pub last_login_vault: Option<VaultLocation>,
    /// How long copied usernames and passwords stay on the clipboard; `None` keeps them.
    pub clipboard_clear_after: Option<Duration>,
    /// How long without the user's input before every Vault locks; `None` never locks them.
    pub idle_lock_after: Option<Duration>,
    /// Whether every Vault locks when the OS screen locks or the machine suspends.
    pub lock_on_os_lock: bool,
    /// The Dropbox app keefast connects as, overriding the one it was built with. Not a secret:
    /// keefast connects with PKCE, which needs no client secret.
    pub dropbox_app_key: Option<String>,
    pub theme: ThemeSetting,
    /// Whether the favicons of Entry URLs' sites are downloaded. Off unless the user turns it
    /// on, as keefast makes no network requests they haven't agreed to.
    pub download_favicons: bool,
}

/// Which colours the app is drawn in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThemeSetting {
    /// Follow the OS light/dark setting.
    #[default]
    System,
    Light,
    Dark,
}

impl AppConfig {
    pub const DEFAULT_CLIPBOARD_CLEAR_AFTER: Duration = Duration::from_secs(30);
    pub const DEFAULT_IDLE_LOCK_AFTER: Duration = Duration::from_secs(15 * 60);
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recent_vaults: Vec::new(),
            autosave: false,
            browser_integration: false,
            generator: GeneratorSettings::default(),
            last_login_vault: None,
            clipboard_clear_after: Some(Self::DEFAULT_CLIPBOARD_CLEAR_AFTER),
            idle_lock_after: Some(Self::DEFAULT_IDLE_LOCK_AFTER),
            lock_on_os_lock: true,
            dropbox_app_key: None,
            theme: ThemeSetting::System,
            download_favicons: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not access the app config: {0}")]
    Io(#[from] io::Error),
    #[error("the app config is damaged: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("could not write the app config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Loads and saves the app config. Injected into the Workspace.
pub trait ConfigStore: Send + Sync {
    /// The saved config, or the defaults when none has been saved yet.
    fn load(&self) -> Result<AppConfig, ConfigError>;

    fn save(&self, config: &AppConfig) -> Result<(), ConfigError>;
}

/// The app config as a TOML file.
pub struct FileConfigStore {
    path: PathBuf,
}

impl FileConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `keefast/config.toml` in the OS config directory (`~/.config` on Linux, `%APPDATA%` on
    /// Windows), if the OS has one.
    pub fn in_os_config_dir() -> Option<Self> {
        let dir = dirs::config_dir()?;
        Some(Self::new(dir.join("keefast").join("config.toml")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ConfigStore for FileConfigStore {
    fn load(&self) -> Result<AppConfig, ConfigError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(AppConfig::default());
            }
            Err(error) => return Err(error.into()),
        };
        Ok(toml::from_str::<ConfigFile>(&text)?.into())
    }

    fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        let text = toml::to_string(&ConfigFile::from(config))?;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        atomic_file::replace(&self.path, text.as_bytes())?;
        Ok(())
    }
}

/// The config file's layout, kept apart from [`AppConfig`] so the file format stays stable.
/// Recent Vault files are read one by one, so one this version doesn't understand (a Storage
/// provider from a newer version, say) is skipped rather than failing the whole file. Generator
/// settings this version doesn't understand (a new mode, say) don't fail the file either: the
/// generator goes back to its defaults, and a theme it doesn't know follows the OS.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct ConfigFile {
    autosave: bool,
    browser_integration: bool,
    /// 0 keeps copied secrets on the clipboard.
    clipboard_clear_seconds: u64,
    /// 0 never locks the Vaults for inactivity.
    idle_lock_seconds: u64,
    lock_on_os_lock: bool,
    /// `system`, `light` or `dark`.
    theme: String,
    download_favicons: bool,
    recent_vaults: Vec<toml::Value>,
    generator: Option<toml::Value>,
    last_login_vault: Option<toml::Value>,
    dropbox_app_key: Option<String>,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self::from(&AppConfig::default())
    }
}

/// A Vault file's location, tagged with its Storage provider:
/// `{ provider = "local", path = "/home/me/personal.kdbx" }` or
/// `{ provider = "dropbox", path = "/Vaults/Personal.kdbx" }`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
enum StoredLocation {
    Local { path: String },
    Dropbox { path: String },
}

impl From<ConfigFile> for AppConfig {
    fn from(file: ConfigFile) -> Self {
        Self {
            recent_vaults: file
                .recent_vaults
                .into_iter()
                .filter_map(read_location)
                .collect(),
            autosave: file.autosave,
            browser_integration: file.browser_integration,
            generator: file
                .generator
                .and_then(|generator| generator.try_into::<GeneratorFile>().ok())
                .map(GeneratorSettings::from)
                .unwrap_or_default(),
            last_login_vault: file.last_login_vault.and_then(read_location),
            clipboard_clear_after: wait_from_seconds(file.clipboard_clear_seconds),
            idle_lock_after: wait_from_seconds(file.idle_lock_seconds),
            lock_on_os_lock: file.lock_on_os_lock,
            dropbox_app_key: file.dropbox_app_key.filter(|key| !key.is_empty()),
            theme: match file.theme.as_str() {
                "light" => ThemeSetting::Light,
                "dark" => ThemeSetting::Dark,
                _ => ThemeSetting::System,
            },
            download_favicons: file.download_favicons,
        }
    }
}

impl From<&AppConfig> for ConfigFile {
    fn from(config: &AppConfig) -> Self {
        Self {
            autosave: config.autosave,
            browser_integration: config.browser_integration,
            clipboard_clear_seconds: wait_to_seconds(config.clipboard_clear_after),
            idle_lock_seconds: wait_to_seconds(config.idle_lock_after),
            lock_on_os_lock: config.lock_on_os_lock,
            theme: match config.theme {
                ThemeSetting::System => "system",
                ThemeSetting::Light => "light",
                ThemeSetting::Dark => "dark",
            }
            .to_owned(),
            download_favicons: config.download_favicons,
            recent_vaults: config
                .recent_vaults
                .iter()
                .filter_map(write_location)
                .collect(),
            generator: toml::Value::try_from(GeneratorFile::from(&config.generator)).ok(),
            last_login_vault: config.last_login_vault.as_ref().and_then(write_location),
            dropbox_app_key: config.dropbox_app_key.clone(),
        }
    }
}

/// The generator settings: `[generator]` with `mode = "characters"` or `"passphrase"` and the
/// settings of both modes. A setting left out is the default.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct GeneratorFile {
    mode: GeneratorModeFile,
    length: u32,
    lowercase: bool,
    uppercase: bool,
    digits: bool,
    symbols: bool,
    exclude_lookalikes: bool,
    words: u32,
    separator: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum GeneratorModeFile {
    Characters,
    Passphrase,
}

impl Default for GeneratorFile {
    fn default() -> Self {
        Self::from(&GeneratorSettings::default())
    }
}

impl From<GeneratorFile> for GeneratorSettings {
    fn from(file: GeneratorFile) -> Self {
        Self {
            mode: match file.mode {
                GeneratorModeFile::Characters => GeneratorMode::Characters,
                GeneratorModeFile::Passphrase => GeneratorMode::Passphrase,
            },
            characters: CharacterRecipe {
                length: file.length,
                lowercase: file.lowercase,
                uppercase: file.uppercase,
                digits: file.digits,
                symbols: file.symbols,
                exclude_lookalikes: file.exclude_lookalikes,
            },
            passphrase: PassphraseRecipe {
                words: file.words,
                separator: file.separator,
            },
        }
    }
}

impl From<&GeneratorSettings> for GeneratorFile {
    fn from(settings: &GeneratorSettings) -> Self {
        let CharacterRecipe {
            length,
            lowercase,
            uppercase,
            digits,
            symbols,
            exclude_lookalikes,
        } = settings.characters;
        Self {
            mode: match settings.mode {
                GeneratorMode::Characters => GeneratorModeFile::Characters,
                GeneratorMode::Passphrase => GeneratorModeFile::Passphrase,
            },
            length,
            lowercase,
            uppercase,
            digits,
            symbols,
            exclude_lookalikes,
            words: settings.passphrase.words,
            separator: settings.passphrase.separator.clone(),
        }
    }
}

/// A wait in whole seconds, where 0 is never.
fn wait_from_seconds(seconds: u64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

/// A wait as whole seconds, where 0 is never, so a wait under a second is kept as one.
fn wait_to_seconds(wait: Option<Duration>) -> u64 {
    wait.map_or(0, |wait| wait.as_secs().max(1))
}

fn read_location(value: toml::Value) -> Option<VaultLocation> {
    match value.try_into().ok()? {
        StoredLocation::Local { path } => Some(VaultLocation::Local(path.into())),
        StoredLocation::Dropbox { path } => Some(VaultLocation::Dropbox(path)),
    }
}

/// TOML only holds Unicode, so a path that isn't valid Unicode is not remembered.
fn write_location(location: &VaultLocation) -> Option<toml::Value> {
    let recent = match location {
        VaultLocation::Local(path) => StoredLocation::Local {
            path: path.to_str()?.to_owned(),
        },
        VaultLocation::Dropbox(path) => StoredLocation::Dropbox { path: path.clone() },
    };
    toml::Value::try_from(recent).ok()
}
