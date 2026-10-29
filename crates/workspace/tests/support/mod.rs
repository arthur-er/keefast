//! The faked environment Workspace tests run in.

#![allow(dead_code)]

pub mod extension;
pub mod fake_dropbox;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keefast_workspace::{
    AppConfig, Browser, BrowserHosts, CachedFile, Clipboard, Clock, ConfigError, ConfigStore,
    Revision, SecretStore, SecretStoreError, StorageError, StorageProvider, StoredFile, VaultCache,
    VaultLocation, Watch,
};
use secrecy::{ExposeSecret, SecretString};

pub const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../../fixtures/keepassxc-kdbx4.kdbx");
pub const PASSWORD: &str = "fixture-password";
/// KeePassXC's browser settings on Entries and Groups; see scripts/make-browser-fixture.sh.
pub const BROWSER_KDBX: &[u8] = include_bytes!("../../../../fixtures/keepassxc-browser.kdbx");
/// TOTP settings in every client's field format; see scripts/make-totp-fixture.sh.
pub const TOTP_KDBX: &[u8] = include_bytes!("../../../../fixtures/totp.kdbx");

type OnChange = Arc<dyn Fn() + Send + Sync>;

/// An in-memory Storage provider holding Vault files by location, each write a new numbered
/// revision, as cloud storage keeps them.
#[derive(Default)]
pub struct FakeStorage {
    files: Mutex<HashMap<VaultLocation, (Vec<u8>, u64)>>,
    next_revision: Mutex<u64>,
    failing_writes: Mutex<bool>,
    writes: Mutex<usize>,
    watchers: Arc<Mutex<Vec<(VaultLocation, OnChange)>>>,
    /// Changes made elsewhere, landing just before the next write each.
    races: Mutex<Vec<(VaultLocation, Vec<u8>)>>,
    offline: Mutex<bool>,
    reads: Mutex<usize>,
    conflicts: Mutex<usize>,
}

impl FakeStorage {
    pub fn with_file(self, location: &VaultLocation, bytes: &[u8]) -> Self {
        self.put(location, bytes);
        self
    }

    pub fn file(&self, location: &VaultLocation) -> Option<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(location)
            .map(|(bytes, _)| bytes.clone())
    }

    fn put(&self, location: &VaultLocation, bytes: &[u8]) -> Revision {
        let mut next = self.next_revision.lock().unwrap();
        *next += 1;
        self.files
            .lock()
            .unwrap()
            .insert(location.clone(), (bytes.to_vec(), *next));
        Revision::new(next.to_string())
    }

    /// Replaces the Vault file as another device would, telling those watching it.
    pub fn change_elsewhere(&self, location: &VaultLocation, bytes: &[u8]) {
        self.put(location, bytes);
        let watchers: Vec<OnChange> = self
            .watchers
            .lock()
            .unwrap()
            .iter()
            .filter(|(watched, _)| watched == location)
            .map(|(_, on_change)| Arc::clone(on_change))
            .collect();
        for on_change in watchers {
            on_change();
        }
    }

    /// Another device deletes the Vault file.
    pub fn delete_elsewhere(&self, location: &VaultLocation) {
        self.files.lock().unwrap().remove(location);
    }

    /// Has another device replace the Vault file just before keefast's next write to it.
    pub fn change_before_next_write(&self, location: &VaultLocation, bytes: &[u8]) {
        self.races
            .lock()
            .unwrap()
            .push((location.clone(), bytes.to_vec()));
    }

    pub fn watching(&self, location: &VaultLocation) -> usize {
        self.watchers
            .lock()
            .unwrap()
            .iter()
            .filter(|(watched, _)| watched == location)
            .count()
    }

    /// Makes every write fail, like a full disk, until turned off again.
    pub fn fail_writes(&self, fail: bool) {
        *self.failing_writes.lock().unwrap() = fail;
    }

    pub fn writes(&self) -> usize {
        *self.writes.lock().unwrap()
    }

    /// Makes the provider unreachable, as without a network, until turned off again.
    pub fn go_offline(&self, offline: bool) {
        *self.offline.lock().unwrap() = offline;
    }

    /// How many times a Vault file's bytes were read.
    pub fn reads(&self) -> usize {
        *self.reads.lock().unwrap()
    }

    /// How many writes were turned down because the Vault file had changed.
    pub fn conflicts(&self) -> usize {
        *self.conflicts.lock().unwrap()
    }

    /// Lands the change made elsewhere waiting for the next write to `location`, if any.
    fn race(&self, location: &VaultLocation) {
        let race = {
            let mut races = self.races.lock().unwrap();
            let index = races.iter().position(|(raced, _)| raced == location);
            index.map(|index| races.remove(index))
        };
        if let Some((_, theirs)) = race {
            self.change_elsewhere(location, &theirs);
        }
    }

    fn check_online(&self) -> Result<(), StorageError> {
        match *self.offline.lock().unwrap() {
            true => Err(StorageError::Offline("no network".into())),
            false => Ok(()),
        }
    }
}

impl StorageProvider for FakeStorage {
    fn read(&self, location: &VaultLocation) -> Result<StoredFile, StorageError> {
        self.check_online()?;
        *self.reads.lock().unwrap() += 1;
        let files = self.files.lock().unwrap();
        let (bytes, revision) = files
            .get(location)
            .ok_or_else(|| StorageError::NotFound(location.clone()))?;
        Ok(StoredFile {
            bytes: bytes.clone(),
            revision: Revision::new(revision.to_string()),
        })
    }

    fn write(
        &self,
        location: &VaultLocation,
        bytes: &[u8],
        expected: Option<&Revision>,
    ) -> Result<Revision, StorageError> {
        self.check_online()?;
        if *self.failing_writes.lock().unwrap() {
            return Err(StorageError::Io(io::Error::other("disk full")));
        }
        self.race(location);
        let current = self
            .files
            .lock()
            .unwrap()
            .get(location)
            .map(|(_, revision)| Revision::new(revision.to_string()));
        if let (Some(expected), Some(current)) = (expected, current)
            && *expected != current
        {
            *self.conflicts.lock().unwrap() += 1;
            return Err(StorageError::Conflict);
        }
        *self.writes.lock().unwrap() += 1;
        Ok(self.put(location, bytes))
    }

    fn create(&self, location: &VaultLocation, bytes: &[u8]) -> Result<Revision, StorageError> {
        self.check_online()?;
        if *self.failing_writes.lock().unwrap() {
            return Err(StorageError::Io(io::Error::other("disk full")));
        }
        self.race(location);
        if self.files.lock().unwrap().contains_key(location) {
            return Err(StorageError::Exists(location.clone()));
        }
        *self.writes.lock().unwrap() += 1;
        Ok(self.put(location, bytes))
    }

    fn revision(&self, location: &VaultLocation) -> Result<Revision, StorageError> {
        self.check_readable()?;
        let files = self.files.lock().unwrap();
        let (_, revision) = files
            .get(location)
            .ok_or_else(|| StorageError::NotFound(location.clone()))?;
        Ok(Revision::new(revision.to_string()))
    }

    fn watch(&self, location: &VaultLocation, on_change: OnChange) -> Option<Watch> {
        let id = Arc::as_ptr(&on_change) as *const () as usize;
        self.watchers
            .lock()
            .unwrap()
            .push((location.clone(), on_change));
        Some(Watch::new(Unwatch {
            watchers: Arc::clone(&self.watchers),
            id,
        }))
    }
}

/// Stops a [`FakeStorage`] watch when dropped.
struct Unwatch {
    watchers: Arc<Mutex<Vec<(VaultLocation, OnChange)>>>,
    id: usize,
}

impl Drop for Unwatch {
    fn drop(&mut self) {
        self.watchers
            .lock()
            .unwrap()
            .retain(|(_, on_change)| Arc::as_ptr(on_change) as *const () as usize != self.id);
    }
}

/// A real Storage provider, but a file appears, as another device or app would put it there,
/// right after a read first finds none: between a check that no file is there and the write
/// that follows it.
pub struct AppearsAfterCheck {
    inner: Arc<dyn StorageProvider>,
    appear: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl AppearsAfterCheck {
    pub fn new(inner: Arc<dyn StorageProvider>, appear: impl FnOnce() + Send + 'static) -> Self {
        Self {
            inner,
            appear: Mutex::new(Some(Box::new(appear))),
        }
    }
}

impl StorageProvider for AppearsAfterCheck {
    fn read(&self, location: &VaultLocation) -> Result<StoredFile, StorageError> {
        let result = self.inner.read(location);
        if matches!(result, Err(StorageError::NotFound(_)))
            && let Some(appear) = self.appear.lock().unwrap().take()
        {
            appear();
        }
        result
    }

    fn canonical(&self, location: &VaultLocation) -> VaultLocation {
        self.inner.canonical(location)
    }

    fn write(
        &self,
        location: &VaultLocation,
        bytes: &[u8],
        expected: Option<&Revision>,
    ) -> Result<Revision, StorageError> {
        self.inner.write(location, bytes, expected)
    }

    fn create(&self, location: &VaultLocation, bytes: &[u8]) -> Result<Revision, StorageError> {
        self.inner.create(location, bytes)
    }
}

/// An in-memory cache of Vault files, as the encrypted local cache keeps them on disk.
#[derive(Default)]
pub struct FakeVaultCache {
    files: Mutex<HashMap<VaultLocation, CachedFile>>,
    failing: Mutex<bool>,
}

impl FakeVaultCache {
    pub fn cached(&self, location: &VaultLocation) -> Option<CachedFile> {
        self.files.lock().unwrap().get(location).cloned()
    }

    /// Makes every store fail, like a full disk, until turned off again.
    pub fn fail(&self, fail: bool) {
        *self.failing.lock().unwrap() = fail;
    }
}

impl VaultCache for FakeVaultCache {
    fn load(&self, location: &VaultLocation) -> Option<CachedFile> {
        self.cached(location)
    }

    fn store(&self, location: &VaultLocation, file: &CachedFile) -> io::Result<()> {
        if *self.failing.lock().unwrap() {
            return Err(io::Error::other("disk full"));
        }
        self.files
            .lock()
            .unwrap()
            .insert(location.clone(), file.clone());
        Ok(())
    }
}

/// A clock that stands still until told to move.
pub struct FakeClock(Mutex<SystemTime>);

impl FakeClock {
    pub fn at_unix_seconds(seconds: u64) -> Self {
        Self(Mutex::new(UNIX_EPOCH + Duration::from_secs(seconds)))
    }

    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

/// A clipboard that holds text in memory, like the system clipboard would.
#[derive(Default)]
pub struct FakeClipboard(Option<String>);

impl FakeClipboard {
    pub fn text(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Something else puts `text` on the clipboard, as when the user copies elsewhere.
    pub fn copy_elsewhere(&mut self, text: &str) {
        self.0 = Some(text.to_owned());
    }
}

impl Clipboard for FakeClipboard {
    fn read_text(&self) -> Option<String> {
        self.0.clone()
    }

    fn write_text(&mut self, text: &str) {
        self.0 = Some(text.to_owned());
    }
}

pub fn personal_vault() -> VaultLocation {
    VaultLocation::Local(PathBuf::from("/vaults/personal.kdbx"))
}

pub fn work_vault() -> VaultLocation {
    VaultLocation::Local(PathBuf::from("/vaults/work.kdbx"))
}

/// An in-memory app config, as a config file would hold it between runs.
#[derive(Default)]
pub struct FakeConfigStore {
    config: Mutex<Option<AppConfig>>,
    failing: Mutex<bool>,
}

impl FakeConfigStore {
    pub fn holding(config: AppConfig) -> Self {
        Self {
            config: Mutex::new(Some(config)),
            failing: Mutex::new(false),
        }
    }

    pub fn saved(&self) -> Option<AppConfig> {
        self.config.lock().unwrap().clone()
    }

    /// Makes every load and save fail, like an unreadable config directory.
    pub fn fail(&self, fail: bool) {
        *self.failing.lock().unwrap() = fail;
    }

    fn check(&self) -> Result<(), ConfigError> {
        match *self.failing.lock().unwrap() {
            true => Err(ConfigError::Io(io::Error::other("permission denied"))),
            false => Ok(()),
        }
    }
}

impl ConfigStore for FakeConfigStore {
    fn load(&self) -> Result<AppConfig, ConfigError> {
        self.check()?;
        Ok(self.saved().unwrap_or_default())
    }

    fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        self.check()?;
        *self.config.lock().unwrap() = Some(config.clone());
        Ok(())
    }
}

/// Records native-host registrations instead of writing them.
#[derive(Default)]
pub struct FakeBrowserHosts {
    pub installs: Mutex<usize>,
    pub uninstalls: Mutex<usize>,
    failing: Mutex<bool>,
}

impl FakeBrowserHosts {
    pub fn installs(&self) -> usize {
        *self.installs.lock().unwrap()
    }

    pub fn uninstalls(&self) -> usize {
        *self.uninstalls.lock().unwrap()
    }

    pub fn fail(&self, fail: bool) {
        *self.failing.lock().unwrap() = fail;
    }
}

impl BrowserHosts for FakeBrowserHosts {
    fn install(&self) -> io::Result<Vec<Browser>> {
        if *self.failing.lock().unwrap() {
            return Err(io::Error::other("permission denied"));
        }
        *self.installs.lock().unwrap() += 1;
        Ok(vec![Browser::Chromium, Browser::Brave])
    }

    fn uninstall(&self) -> io::Result<()> {
        *self.uninstalls.lock().unwrap() += 1;
        Ok(())
    }
}

/// An in-memory secret store, standing in for the OS one.
#[derive(Default)]
pub struct FakeSecretStore {
    secrets: Mutex<HashMap<String, String>>,
    failing: Mutex<bool>,
}

impl FakeSecretStore {
    pub fn secret(&self, name: &str) -> Option<String> {
        self.secrets.lock().unwrap().get(name).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.lock().unwrap().is_empty()
    }

    /// Makes every call fail, like a locked or missing OS secret store.
    pub fn fail(&self, fail: bool) {
        *self.failing.lock().unwrap() = fail;
    }

    fn check(&self) -> Result<(), SecretStoreError> {
        match *self.failing.lock().unwrap() {
            true => Err(SecretStoreError::Unavailable("no Secret Service".into())),
            false => Ok(()),
        }
    }
}

impl SecretStore for FakeSecretStore {
    fn get(&self, name: &str) -> Result<Option<SecretString>, SecretStoreError> {
        self.check()?;
        Ok(self.secret(name).map(SecretString::from))
    }

    fn set(&self, name: &str, secret: &SecretString) -> Result<(), SecretStoreError> {
        self.check()?;
        self.secrets
            .lock()
            .unwrap()
            .insert(name.to_owned(), secret.expose_secret().to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.secrets.lock().unwrap().remove(name);
        Ok(())
    }
}
