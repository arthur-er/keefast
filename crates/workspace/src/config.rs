//! The app config: machine-local preferences, kept as TOML in the OS config directory. It never
//! holds secrets (ADR 0005).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use keefast_core::{CharacterRecipe, PassphraseRecipe};

use crate::atomic_file;
use crate::generator::{GeneratorMode, GeneratorSettings};
use crate::storage::VaultLocation;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Vault files opened lately, most recent first.
    pub recent_vaults: Vec<VaultLocation>,
    pub autosave: bool,
    /// Whether browser extensions may reach keefast through its native-messaging host.
    pub browser_integration: bool,
    pub generator: GeneratorSettings,
    /// The Vault file the user last saved a captured login to, offered first next time.
    pub last_login_vault: Option<VaultLocation>,
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
/// generator goes back to its defaults.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ConfigFile {
    autosave: bool,
    browser_integration: bool,
    recent_vaults: Vec<toml::Value>,
    generator: Option<toml::Value>,
    last_login_vault: Option<toml::Value>,
}

/// A Vault file's location, tagged with its Storage provider:
/// `{ provider = "local", path = "/home/me/personal.kdbx" }`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
enum StoredLocation {
    Local { path: String },
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
        }
    }
}

impl From<&AppConfig> for ConfigFile {
    fn from(config: &AppConfig) -> Self {
        Self {
            autosave: config.autosave,
            browser_integration: config.browser_integration,
            recent_vaults: config
                .recent_vaults
                .iter()
                .filter_map(write_location)
                .collect(),
            generator: toml::Value::try_from(GeneratorFile::from(&config.generator)).ok(),
            last_login_vault: config.last_login_vault.as_ref().and_then(write_location),
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

fn read_location(value: toml::Value) -> Option<VaultLocation> {
    match value.try_into().ok()? {
        StoredLocation::Local { path } => Some(VaultLocation::Local(path.into())),
    }
}

/// TOML only holds Unicode, so a path that isn't valid Unicode is not remembered.
fn write_location(location: &VaultLocation) -> Option<toml::Value> {
    let recent = match location {
        VaultLocation::Local(path) => StoredLocation::Local {
            path: path.to_str()?.to_owned(),
        },
    };
    toml::Value::try_from(recent).ok()
}
