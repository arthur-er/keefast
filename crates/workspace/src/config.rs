//! The app config: machine-local preferences, kept as TOML in the OS config directory. It never
//! holds secrets (ADR 0005).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic_file;
use crate::storage::VaultLocation;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Vault files opened lately, most recent first.
    pub recent_vaults: Vec<VaultLocation>,
    pub autosave: bool,
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
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ConfigFile {
    autosave: bool,
    recent_vaults: Vec<RecentVault>,
}

/// A recent Vault file, tagged with its Storage provider:
/// `{ provider = "local", path = "/home/me/personal.kdbx" }`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
enum RecentVault {
    Local { path: String },
}

impl From<ConfigFile> for AppConfig {
    fn from(file: ConfigFile) -> Self {
        Self {
            recent_vaults: file
                .recent_vaults
                .into_iter()
                .map(|recent| match recent {
                    RecentVault::Local { path } => VaultLocation::Local(path.into()),
                })
                .collect(),
            autosave: file.autosave,
        }
    }
}

impl From<&AppConfig> for ConfigFile {
    fn from(config: &AppConfig) -> Self {
        Self {
            autosave: config.autosave,
            // TOML only holds Unicode, so a path that isn't valid Unicode is not remembered.
            recent_vaults: config
                .recent_vaults
                .iter()
                .filter_map(|location| match location {
                    VaultLocation::Local(path) => Some(RecentVault::Local {
                        path: path.to_str()?.to_owned(),
                    }),
                })
                .collect(),
        }
    }
}
