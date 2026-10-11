//! The Plugins installed on this machine, and what the user agreed to for each.
//!
//! Installed Plugins live in keefast's local data directory (`~/.local/share/keefast/plugins` on
//! Linux, `%LOCALAPPDATA%\keefast\plugins` on Windows): each as the package file it was
//! installed from, `<id>.kfplugin`, and beside them `plugins.json`, which keeps for each whether
//! it is enabled, the Plugin capabilities and Owned fields the user agreed to, and the publisher
//! key first seen for its id (trust on first use). Every load reads each package again and checks
//! its signature, its key and its capabilities against what was agreed.
//!
//! The Plugins bundled with keefast are listed too, as keefast's. They can be disabled, not
//! removed. Packages signed by keefast's release key are keefast's too, and are the only ones
//! that may have a `keefast.` id (`bundled.rs`).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bundled::{
    KEEFAST_PREFIX, bundled_by_release_key, bundled_key, bundled_plugins, release_keys,
};
use crate::manifest::{Capability, Manifest};
use crate::package::{MAX_PACKAGE, Package, PackageError, PublisherKey};
use crate::{PluginError, PluginHost};

const STATE_FILE: &str = "plugins.json";
const PACKAGE_EXTENSION: &str = "kfplugin";

/// Who made a Plugin, as far as keefast can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publisher {
    /// keefast itself: signed by one of keefast's release keys, or bundled with this build and
    /// signed by the key it was built with.
    Keefast {
        /// The signing key's fingerprint.
        fingerprint: String,
        /// Signed by keefast's release key. Only a Plugin bundled with a development or source
        /// build isn't: it is signed by a key that build made for itself.
        release: bool,
    },
    /// Someone keefast can't vouch for: the name their manifest gives, and their key, trusted
    /// since the Plugin was first installed.
    Unverified { name: String, fingerprint: String },
}

/// A Plugin installed, or compiled in.
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub id: String,
    pub name: String,
    /// `None` when its package can't be read any more.
    pub version: Option<String>,
    pub publisher: Publisher,
    /// Compiled into keefast, so it can't be removed.
    pub bundled: bool,
    pub enabled: bool,
    /// Whether it runs now: enabled, and it loaded.
    pub running: bool,
    /// The Plugin capabilities the user agreed to.
    pub capabilities: Vec<Capability>,
    pub owned_fields: Vec<String>,
    pub icon: Option<Vec<u8>>,
    /// Why it doesn't run though enabled: its package failed its checks, or its component didn't
    /// load.
    pub problem: Option<String>,
}

/// What installing a package would do, for the user to agree to before anything is installed.
#[derive(Debug, Clone)]
pub struct Offer {
    /// Names exactly this package: [`PluginStore::install`] installs it only if it is still
    /// the package the user agreed to.
    pub digest: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: Publisher,
    /// Whether keefast sees the package's key for the first time, and will trust it from now
    /// on for this Plugin's updates.
    pub new_key: bool,
    /// The version installed now, if this is an update.
    pub installed_version: Option<String>,
    /// Every Plugin capability it asks for.
    pub capabilities: Vec<Capability>,
    /// The custom fields it would own.
    pub owned_fields: Vec<String>,
    /// What it asks for that the user hasn't agreed to yet: everything, for a new Plugin.
    pub new_capabilities: Vec<Capability>,
    pub new_owned_fields: Vec<String>,
    pub icon: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Package(#[from] PackageError),
    #[error("{0}")]
    Plugin(#[from] PluginError),
    #[error(
        "\"{name}\" is signed by another key than the installed one, so it may not come from the \
         same publisher. If you trust it, remove the installed \"{name}\" first."
    )]
    KeyChanged { name: String },
    #[error("Only keefast's own Plugins can be named {}…", KEEFAST_PREFIX)]
    Reserved,
    #[error("{0} comes with keefast, and is updated with keefast rather than from a file.")]
    BundledId(String),
    #[error(
        "\"{name}\" would keep its data in the field \"{field}\", which the installed Plugin \
         \"{other}\" owns. Two Plugins can't own the same field."
    )]
    Clash {
        name: String,
        field: String,
        other: String,
    },
    #[error(
        "The package changed since keefast showed what it asks for. Choose it again to see what \
         it asks for now."
    )]
    Changed,
    #[error("That Plugin is no longer installed.")]
    NotInstalled,
    #[error("{0} comes with keefast and can't be removed. Disable it instead.")]
    Bundled(String),
    #[error("Plugins can't be installed: keefast has no data directory on this machine.")]
    NoStore,
    #[error("The installed Plugins could not be read or saved: {0}")]
    Io(String),
}

impl From<io::Error> for InstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// `plugins.json`: what the user agreed to for each installed Plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    plugins: Vec<Agreed>,
    /// The Plugins compiled into keefast that the user disabled.
    disabled_bundled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Agreed {
    id: String,
    enabled: bool,
    /// The key its first package was signed by, base64: every later one must be too.
    publisher_key: String,
    capabilities: Vec<Capability>,
    owned_fields: Vec<String>,
}

/// What the last load found of an installed Plugin.
#[derive(Debug, Clone, Default)]
struct Found {
    manifest: Option<Manifest>,
    icon: Option<Vec<u8>>,
    problem: Option<String>,
}

/// The installed Plugins, on disk, and what the user agreed to for them.
pub struct PluginStore {
    /// `None` when there is nowhere to keep Plugins: only the bundled ones run.
    dir: Option<PathBuf>,
    state: State,
    /// Why `plugins.json` couldn't be read; while it can't, nothing is changed on disk.
    state_error: Option<String>,
    bundled: Vec<Manifest>,
    found: BTreeMap<String, Found>,
    /// keefast's release keys: what they sign is keefast's.
    release_keys: Vec<PublisherKey>,
}

impl PluginStore {
    /// Plugins kept in `dir`, which needn't exist yet.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let (state, state_error) = match read_state(&dir.join(STATE_FILE)) {
            Ok(state) => (state, None),
            Err(error) => (State::default(), Some(error)),
        };
        Self {
            dir: Some(dir),
            state,
            state_error,
            bundled: Vec::new(),
            found: BTreeMap::new(),
            release_keys: release_keys(),
        }
    }

    /// `keefast/plugins` in the OS's local data directory, if it has one.
    pub fn in_os_data_dir() -> Option<Self> {
        Some(Self::open(
            dirs::data_local_dir()?.join("keefast").join("plugins"),
        ))
    }

    /// Nowhere to install Plugins: only the bundled ones run.
    pub fn bundled_only() -> Self {
        Self {
            dir: None,
            state: State::default(),
            state_error: None,
            bundled: Vec::new(),
            found: BTreeMap::new(),
            release_keys: release_keys(),
        }
    }

    /// The store trusting these keys as keefast's release keys rather than those keefast was
    /// built with, for tests: no test can sign with the real ones.
    #[doc(hidden)]
    pub fn with_release_keys(mut self, keys: Vec<PublisherKey>) -> Self {
        self.release_keys = keys;
        self
    }

    fn is_release_key(&self, key: &PublisherKey) -> bool {
        self.release_keys.contains(key)
    }

    /// Whether a package's key may update a Plugin first installed with the `pinned` one: it is
    /// the same key, or both are keefast's release keys, which keefast rotates from one to the
    /// next.
    fn same_publisher(&self, pinned: &str, key: &PublisherKey) -> bool {
        match PublisherKey::parse(pinned) {
            Ok(pinned) => {
                pinned == *key || (self.is_release_key(&pinned) && self.is_release_key(key))
            }
            Err(_) => false,
        }
    }

    /// Why the installed Plugins couldn't be read, if they couldn't.
    pub fn error(&self) -> Option<&str> {
        self.state_error.as_deref()
    }

    /// Loads the enabled Plugins into a host that has none, checking each package again.
    pub fn load(&mut self, host: &mut PluginHost) {
        self.bundled.clear();
        self.found.clear();
        for bundled in bundled_plugins() {
            let id = bundled.manifest.id.clone();
            let found = Found {
                problem: if self.state.disabled_bundled.contains(&id) {
                    None
                } else {
                    run_bundled(host, bundled.package)
                },
                ..Found::default()
            };
            self.found.insert(id, found);
            self.bundled.push(bundled.manifest);
        }
        for agreed in self.state.plugins.clone() {
            let found = self.load_installed(host, &agreed);
            self.found.insert(agreed.id, found);
        }
    }

    /// Checks an installed Plugin's package and, if it is enabled, runs it.
    fn load_installed(&self, host: &mut PluginHost, agreed: &Agreed) -> Found {
        let package = match self.check_installed(agreed) {
            Ok(package) => package,
            Err(problem) => {
                return Found {
                    problem: Some(problem),
                    ..Found::default()
                };
            }
        };
        let problem = if agreed.enabled {
            host.add(package.manifest.clone(), &package.component)
                .err()
                .map(|error| error.to_string())
        } else {
            None
        };
        Found {
            manifest: Some(package.manifest),
            icon: package.icon,
            problem,
        }
    }

    /// An installed Plugin's package, if it is still signed by the key first trusted for it and
    /// asks for nothing the user didn't agree to.
    fn check_installed(&self, agreed: &Agreed) -> Result<Package, String> {
        let dir = self.dir.as_deref().ok_or("keefast has no data directory")?;
        let bytes = read_package(&package_path(dir, &agreed.id))
            .map_err(|error| format!("its package can't be read: {error}"))?;
        let package = Package::read(&bytes).map_err(|error| error.to_string())?;
        if package.manifest.id != agreed.id {
            return Err(format!("its package is now {}", package.manifest.id));
        }
        if !self.same_publisher(&agreed.publisher_key, &package.key) {
            return Err("its package is signed by another key than the one first trusted".into());
        }
        if package.manifest.id.starts_with(KEEFAST_PREFIX) && !self.is_release_key(&package.key) {
            return Err("its package isn't signed by keefast's key".into());
        }
        let asks_more = package
            .manifest
            .capabilities
            .iter()
            .any(|capability| !agreed.capabilities.contains(capability))
            || package
                .manifest
                .owned_names()
                .any(|name| !agreed.owned_fields.iter().any(|owned| owned == name));
        if asks_more {
            return Err("its package asks for Plugin capabilities you didn't agree to".into());
        }
        Ok(package)
    }

    /// Every Plugin, keefast's first, then the installed ones by name.
    pub fn list(&self, host: &PluginHost) -> Vec<InstalledPlugin> {
        let found = |id: &str| self.found.get(id).cloned().unwrap_or_default();
        let mut list: Vec<_> = self
            .bundled
            .iter()
            .map(|manifest| {
                let found = found(&manifest.id);
                InstalledPlugin {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    version: Some(manifest.version.clone()),
                    publisher: Publisher::Keefast {
                        fingerprint: bundled_key().fingerprint(),
                        release: bundled_by_release_key(),
                    },
                    bundled: true,
                    enabled: !self.state.disabled_bundled.contains(&manifest.id),
                    running: host.manifest(&manifest.id).is_some(),
                    capabilities: manifest.capabilities.clone(),
                    owned_fields: manifest.owned_names().map(str::to_owned).collect(),
                    icon: None,
                    problem: found.problem,
                }
            })
            .collect();
        let mut installed: Vec<_> = self
            .state
            .plugins
            .iter()
            .map(|agreed| {
                let found = found(&agreed.id);
                let manifest = found.manifest.as_ref();
                let key = PublisherKey::parse(&agreed.publisher_key).ok();
                let fingerprint = key.map(|key| key.fingerprint()).unwrap_or_default();
                InstalledPlugin {
                    id: agreed.id.clone(),
                    name: manifest.map_or_else(|| agreed.id.clone(), |m| m.name.clone()),
                    version: manifest.map(|m| m.version.clone()),
                    publisher: if key.is_some_and(|key| self.is_release_key(&key)) {
                        Publisher::Keefast {
                            fingerprint,
                            release: true,
                        }
                    } else {
                        Publisher::Unverified {
                            name: manifest
                                .and_then(|m| m.publisher.clone())
                                .unwrap_or_default(),
                            fingerprint,
                        }
                    },
                    bundled: false,
                    enabled: agreed.enabled,
                    running: host.manifest(&agreed.id).is_some(),
                    capabilities: agreed.capabilities.clone(),
                    owned_fields: agreed.owned_fields.clone(),
                    icon: found.icon,
                    problem: found.problem,
                }
            })
            .collect();
        installed.sort_by_key(|plugin| plugin.name.to_lowercase());
        list.extend(installed);
        list
    }

    /// Reads a package and says what installing it would do, refusing one that can't be
    /// installed. Nothing is installed.
    pub fn inspect(&self, host: &PluginHost, bytes: &[u8]) -> Result<Offer, InstallError> {
        self.offer(host, bytes).map(|(offer, _)| offer)
    }

    fn offer(&self, host: &PluginHost, bytes: &[u8]) -> Result<(Offer, Package), InstallError> {
        let package = Package::read(bytes)?;
        let manifest = &package.manifest;
        let id = &manifest.id;
        if let Some(bundled) = self.bundled.iter().find(|b| &b.id == id) {
            return Err(InstallError::BundledId(bundled.name.clone()));
        }
        let keefasts = self.is_release_key(&package.key);
        if id.starts_with(KEEFAST_PREFIX) && !keefasts {
            return Err(InstallError::Reserved);
        }
        let agreed = self.agreed(id);
        if let Some(agreed) = agreed
            && !self.same_publisher(&agreed.publisher_key, &package.key)
        {
            return Err(InstallError::KeyChanged {
                name: manifest.name.clone(),
            });
        }
        for field in manifest.owned_names() {
            if let Some(other) = self.owner_of(field, id) {
                return Err(InstallError::Clash {
                    name: manifest.name.clone(),
                    field: field.to_owned(),
                    other,
                });
            }
        }
        host.check_component(&package.component)?;

        let capabilities = manifest.capabilities.clone();
        let owned_fields: Vec<String> = manifest.owned_names().map(str::to_owned).collect();
        let (new_capabilities, new_owned_fields) = match agreed {
            None => (capabilities.clone(), owned_fields.clone()),
            Some(agreed) => (
                capabilities
                    .iter()
                    .filter(|capability| !agreed.capabilities.contains(capability))
                    .copied()
                    .collect(),
                owned_fields
                    .iter()
                    .filter(|name| !agreed.owned_fields.contains(name))
                    .cloned()
                    .collect(),
            ),
        };
        let offer = Offer {
            digest: package.digest.clone(),
            id: id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            publisher: if keefasts {
                Publisher::Keefast {
                    fingerprint: package.key.fingerprint(),
                    release: true,
                }
            } else {
                Publisher::Unverified {
                    name: manifest.publisher.clone().unwrap_or_default(),
                    fingerprint: package.key.fingerprint(),
                }
            },
            new_key: agreed.is_none() && !keefasts,
            installed_version: agreed.map(|agreed| {
                self.found
                    .get(&agreed.id)
                    .and_then(|found| found.manifest.as_ref())
                    .map_or_else(|| "?".to_owned(), |m| m.version.clone())
            }),
            capabilities,
            owned_fields,
            new_capabilities,
            new_owned_fields,
            icon: package.icon.clone(),
        };
        Ok((offer, package))
    }

    /// Installs a package the user agreed to, or updates the installed Plugin to it, and runs
    /// it. `digest` is the [`Offer`]'s the user saw: a package changed since is refused.
    pub fn install(
        &mut self,
        host: &mut PluginHost,
        bytes: &[u8],
        digest: &str,
    ) -> Result<Offer, InstallError> {
        let dir = self.writable_dir()?;
        let (offer, package) = self.offer(host, bytes)?;
        if offer.digest != digest {
            return Err(InstallError::Changed);
        }
        fs::create_dir_all(&dir)?;
        write_atomically(&package_path(&dir, &offer.id), bytes)?;
        let agreed = Agreed {
            id: offer.id.clone(),
            enabled: true,
            publisher_key: package.key.to_base64(),
            capabilities: offer.capabilities.clone(),
            owned_fields: offer.owned_fields.clone(),
        };
        match self.state.plugins.iter_mut().find(|a| a.id == offer.id) {
            Some(existing) => *existing = agreed.clone(),
            None => self.state.plugins.push(agreed.clone()),
        }
        self.save_state(&dir)?;
        host.unload(&offer.id);
        let found = self.load_installed(host, &agreed);
        self.found.insert(offer.id.clone(), found);
        Ok(offer)
    }

    /// Enables or disables a Plugin. A disabled Plugin doesn't run, so its Owned fields show as
    /// ordinary custom fields; enabling it checks its package again.
    pub fn set_enabled(
        &mut self,
        host: &mut PluginHost,
        id: &str,
        enabled: bool,
    ) -> Result<(), InstallError> {
        let dir = self.writable_dir()?;
        if self.bundled.iter().any(|manifest| manifest.id == id) {
            self.state.disabled_bundled.retain(|other| other != id);
            if !enabled {
                self.state.disabled_bundled.push(id.to_owned());
            }
            self.save_state(&dir)?;
            host.unload(id);
            if enabled
                && let Some(bundled) = bundled_plugins()
                    .into_iter()
                    .find(|bundled| bundled.manifest.id == id)
            {
                self.found.entry(id.to_owned()).or_default().problem =
                    run_bundled(host, bundled.package);
            }
            return Ok(());
        }
        let agreed = self
            .state
            .plugins
            .iter_mut()
            .find(|agreed| agreed.id == id)
            .ok_or(InstallError::NotInstalled)?;
        agreed.enabled = enabled;
        let agreed = agreed.clone();
        self.save_state(&dir)?;
        host.unload(id);
        let found = self.load_installed(host, &agreed);
        self.found.insert(id.to_owned(), found);
        Ok(())
    }

    /// Removes an installed Plugin and its package, and forgets its key, so a package of the
    /// same id signed by another key can be installed afterwards. The data it kept stays in the
    /// Vaults as ordinary custom fields.
    pub fn remove(&mut self, host: &mut PluginHost, id: &str) -> Result<(), InstallError> {
        if let Some(manifest) = self.bundled.iter().find(|manifest| manifest.id == id) {
            return Err(InstallError::Bundled(manifest.name.clone()));
        }
        let dir = self.writable_dir()?;
        if self.agreed(id).is_none() {
            return Err(InstallError::NotInstalled);
        }
        match fs::remove_file(package_path(&dir, id)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        self.state.plugins.retain(|agreed| agreed.id != id);
        self.save_state(&dir)?;
        host.unload(id);
        self.found.remove(id);
        Ok(())
    }

    fn agreed(&self, id: &str) -> Option<&Agreed> {
        self.state.plugins.iter().find(|agreed| agreed.id == id)
    }

    /// The name of the Plugin other than `except` owning a field, enabled or not.
    fn owner_of(&self, field: &str, except: &str) -> Option<String> {
        let bundled = self
            .bundled
            .iter()
            .find(|manifest| manifest.owned_names().any(|name| name == field))
            .map(|manifest| manifest.name.clone());
        bundled.or_else(|| {
            self.state
                .plugins
                .iter()
                .filter(|agreed| agreed.id != except)
                .find(|agreed| agreed.owned_fields.iter().any(|name| name == field))
                .map(|agreed| {
                    self.found
                        .get(&agreed.id)
                        .and_then(|found| found.manifest.as_ref())
                        .map_or_else(|| agreed.id.clone(), |m| m.name.clone())
                })
        })
    }

    /// The directory to change Plugins in, if there is one and `plugins.json` was read.
    fn writable_dir(&self) -> Result<PathBuf, InstallError> {
        if let Some(error) = &self.state_error {
            return Err(InstallError::Io(error.clone()));
        }
        self.dir.clone().ok_or(InstallError::NoStore)
    }

    fn save_state(&self, dir: &Path) -> Result<(), InstallError> {
        let json = serde_json::to_vec_pretty(&self.state).expect("the state serializes");
        fs::create_dir_all(dir)?;
        write_atomically(&dir.join(STATE_FILE), &json)?;
        Ok(())
    }
}

/// Runs a bundled Plugin whose package passed its checks; why it doesn't run otherwise.
fn run_bundled(host: &mut PluginHost, package: Result<Package, String>) -> Option<String> {
    let package = match package {
        Ok(package) => package,
        Err(problem) => return Some(problem),
    };
    host.add(package.manifest, &package.component)
        .err()
        .map(|error| error.to_string())
}

fn package_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{PACKAGE_EXTENSION}"))
}

/// A package file, refused unread if it is larger than a package may be.
pub(crate) fn read_package(path: &Path) -> io::Result<Vec<u8>> {
    if fs::metadata(path)?.len() > MAX_PACKAGE {
        return Err(io::Error::other(PackageError::TooBig.to_string()));
    }
    fs::read(path)
}

fn read_state(path: &Path) -> Result<State, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("{} is damaged: {error}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(State::default()),
        Err(error) => Err(format!("{} can't be read: {error}", path.display())),
    }
}

/// Writes a temporary file beside `path`, flushes it, and renames it over `path`, so a crash
/// never leaves half a file.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("partial");
    let result = (|| {
        fs::write(&temp, bytes)?;
        fs::File::open(&temp)?.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
