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

    /// An audit log beside the Vault file, as a Headless host keeps in its state directory.
    pub fn audit_log(&self) -> Arc<keefast_agent::AuditLog> {
        Arc::new(keefast_agent::AuditLog::new(
            self.path.with_file_name("audit.log"),
        ))
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

/// A KeePassXC Vault whose Master key is [`PASSWORD`] and [`XML_V2_KEY_FILE`].
pub const KEY_FILE_KDBX: &[u8] =
    include_bytes!("../../../../fixtures/key-files/keepassxc-xml-v2.kdbx");
pub const XML_V2_KEY_FILE: &[u8] = include_bytes!("../../../../fixtures/key-files/xml-v2.keyx");

/// An OS secret store in memory.
#[derive(Default)]
pub struct FakeSecretStore(Mutex<std::collections::HashMap<String, String>>);

impl FakeSecretStore {
    pub fn holding(name: &str, secret: &str) -> Arc<Self> {
        let store = Self::default();
        store
            .0
            .lock()
            .unwrap()
            .insert(name.to_owned(), secret.to_owned());
        Arc::new(store)
    }

    pub fn secret(&self, name: &str) -> Option<String> {
        self.0.lock().unwrap().get(name).cloned()
    }
}

impl keefast_workspace::SecretStore for FakeSecretStore {
    fn get(
        &self,
        name: &str,
    ) -> Result<Option<secrecy::SecretString>, keefast_workspace::SecretStoreError> {
        Ok(self.secret(name).map(secrecy::SecretString::from))
    }

    fn set(
        &self,
        name: &str,
        secret: &secrecy::SecretString,
    ) -> Result<(), keefast_workspace::SecretStoreError> {
        use secrecy::ExposeSecret;
        let secret = secret.expose_secret().to_owned();
        self.0.lock().unwrap().insert(name.to_owned(), secret);
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), keefast_workspace::SecretStoreError> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

/// An audit sink that keeps its records in memory, for the test to read.
#[derive(Default)]
pub struct MemoryAudit(Mutex<Vec<keefast_agent::AuditRecord>>);

impl MemoryAudit {
    pub fn records(&self) -> Vec<keefast_agent::AuditRecord> {
        self.0.lock().unwrap().clone()
    }
}

impl keefast_agent::AuditSink for MemoryAudit {
    fn record(&self, record: keefast_agent::AuditRecord) {
        self.0.lock().unwrap().push(record);
    }
}

/// A Workspace behind a mutex, with the Vault a session serves from it: what the Headless host
/// gives an [`keefast_agent::AgentSession`], so that it can let go of the Workspace while an
/// Approver is asked.
pub struct SharedVault {
    pub workspace: Mutex<Workspace>,
    pub vault: VaultId,
}

impl SharedVault {
    pub fn new(workspace: Workspace, vault: VaultId) -> Arc<Self> {
        Arc::new(Self {
            workspace: Mutex::new(workspace),
            vault,
        })
    }

    pub fn workspace(&self) -> std::sync::MutexGuard<'_, Workspace> {
        self.workspace.lock().unwrap()
    }
}

impl keefast_agent::VaultAccess for SharedVault {
    fn read<T>(&self, f: impl FnOnce(&Workspace, VaultId) -> T) -> Option<T> {
        let workspace = self.workspace();
        workspace.readable_vault(self.vault)?;
        Some(f(&workspace, self.vault))
    }

    fn edit<T>(&self, f: impl FnOnce(&mut Workspace, VaultId) -> T) -> Option<T> {
        let mut workspace = self.workspace();
        workspace.readable_vault(self.vault)?;
        Some(f(&mut workspace, self.vault))
    }
}

/// An Approver that answers from a script, in order, and keeps the prompts it was shown. With
/// nothing left to answer, it is as if no Approver were online. `during` runs while it is
/// asked, as a person might lock the Vault meanwhile.
#[derive(Default)]
pub struct FakeApprover {
    answers: Mutex<std::collections::VecDeque<keefast_agent::Approval>>,
    prompts: Mutex<Vec<keefast_agent::AgentPrompt>>,
    during: Mutex<Option<Box<dyn FnMut() + Send>>>,
}

impl FakeApprover {
    pub fn answering(answers: impl IntoIterator<Item = keefast_agent::Approval>) -> Arc<Self> {
        let approver = Self::default();
        approver.answers.lock().unwrap().extend(answers);
        Arc::new(approver)
    }

    pub fn answer(&self, answer: keefast_agent::Approval) {
        self.answers.lock().unwrap().push_back(answer);
    }

    pub fn while_asked(&self, during: impl FnMut() + Send + 'static) {
        *self.during.lock().unwrap() = Some(Box::new(during));
    }

    pub fn prompts(&self) -> Vec<keefast_agent::AgentPrompt> {
        self.prompts.lock().unwrap().clone()
    }
}

impl keefast_agent::Approver for FakeApprover {
    fn ask(&self, prompt: &keefast_agent::AgentPrompt) -> keefast_agent::Approval {
        self.prompts.lock().unwrap().push(prompt.clone());
        if let Some(during) = self.during.lock().unwrap().as_mut() {
            during();
        }
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(keefast_agent::Approval::NoApprover)
    }
}
