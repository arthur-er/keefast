//! The faked environment Workspace tests run in.

#![allow(dead_code)]

pub mod extension;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keefast_workspace::{
    AppConfig, Browser, BrowserHosts, Clipboard, Clock, ConfigError, ConfigStore, StorageError,
    StorageProvider, VaultLocation,
};

pub const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../../fixtures/keepassxc-kdbx4.kdbx");
pub const PASSWORD: &str = "fixture-password";
/// KeePassXC's browser settings on Entries and Groups; see scripts/make-browser-fixture.sh.
pub const BROWSER_KDBX: &[u8] = include_bytes!("../../../../fixtures/keepassxc-browser.kdbx");
/// TOTP settings in every client's field format; see scripts/make-totp-fixture.sh.
pub const TOTP_KDBX: &[u8] = include_bytes!("../../../../fixtures/totp.kdbx");

/// An in-memory Storage provider holding Vault files by location.
#[derive(Default)]
pub struct FakeStorage {
    files: Mutex<HashMap<VaultLocation, Vec<u8>>>,
    failing_writes: Mutex<bool>,
    writes: Mutex<usize>,
}

impl FakeStorage {
    pub fn with_file(self, location: &VaultLocation, bytes: &[u8]) -> Self {
        self.files
            .lock()
            .unwrap()
            .insert(location.clone(), bytes.to_vec());
        self
    }

    pub fn file(&self, location: &VaultLocation) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(location).cloned()
    }

    /// Makes every write fail, like a full disk, until turned off again.
    pub fn fail_writes(&self, fail: bool) {
        *self.failing_writes.lock().unwrap() = fail;
    }

    pub fn writes(&self) -> usize {
        *self.writes.lock().unwrap()
    }
}

impl StorageProvider for FakeStorage {
    fn read(&self, location: &VaultLocation) -> Result<Vec<u8>, StorageError> {
        self.file(location)
            .ok_or_else(|| StorageError::NotFound(location.clone()))
    }

    fn write(&self, location: &VaultLocation, bytes: &[u8]) -> Result<(), StorageError> {
        if *self.failing_writes.lock().unwrap() {
            return Err(StorageError::Io(io::Error::other("disk full")));
        }
        *self.writes.lock().unwrap() += 1;
        self.files
            .lock()
            .unwrap()
            .insert(location.clone(), bytes.to_vec());
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
