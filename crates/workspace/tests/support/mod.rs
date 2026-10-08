//! The faked environment Workspace tests run in.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keefast_workspace::{
    AppConfig, Clock, ConfigError, ConfigStore, StorageError, StorageProvider, VaultLocation,
};

pub const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../../fixtures/keepassxc-kdbx4.kdbx");
pub const PASSWORD: &str = "fixture-password";

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
