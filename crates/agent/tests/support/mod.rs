//! What the `keefast-agent` tests share: a Vault file on disk copied from a fixture, a clock the
//! test moves, and Workspaces that open the Vault file with it.
//!
//! The Vault file is a real file in a temporary directory, read through the usual local Storage
//! provider, as `keefast-agent serve` reads it.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use keefast_workspace::{Clock, LocalStorage, UnlockSecrets, VaultId, VaultLocation, Workspace};
use tempfile::TempDir;

/// The fixtures' Master key.
pub const PASSWORD: &str = "fixture-password";
/// A KeePassXC KDBX 4 Vault from the fixture corpus.
pub const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../../fixtures/keepassxc-kdbx4.kdbx");

/// A clock that only moves when the test moves it.
pub struct FakeClock(Mutex<SystemTime>);

impl FakeClock {
    pub fn at_unix_seconds(seconds: u64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(
            SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
        )))
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

/// A Vault file in a directory of its own, removed when dropped.
pub struct VaultFile {
    _dir: TempDir,
    pub path: PathBuf,
}

impl VaultFile {
    pub fn from_fixture(bytes: &[u8]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.kdbx");
        std::fs::write(&path, bytes).unwrap();
        Self { _dir: dir, path }
    }

    pub fn location(&self) -> VaultLocation {
        VaultLocation::Local(self.path.clone())
    }

    /// A Workspace with this Vault file open and unlocked, its clock `clock`.
    pub fn open(&self, clock: &Arc<FakeClock>) -> (Workspace, VaultId) {
        let clock: Arc<dyn Clock> = clock.clone();
        let mut workspace = Workspace::with_clock(Arc::new(LocalStorage), clock);
        let vault = workspace
            .open_vault(&self.location(), &UnlockSecrets::password(PASSWORD))
            .unwrap();
        (workspace, vault)
    }
}
