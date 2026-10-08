//! TOTP through the Workspace: setting it up on an Entry, saving it, and reading the current
//! code and countdown at the Workspace's clock.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{CompositeKey, EntryId, Totp, TotpAlgorithm, TotpError, Vault};
use keefast_workspace::{EditVaultError, VaultId, Workspace};
use support::{FakeClock, FakeStorage, PASSWORD, TOTP_KDBX, personal_vault};

/// RFC 6238's SHA-1 test secret, Base32-encoded.
const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
/// 1111111109 is one of RFC 6238's test times, 29 seconds into its 30 second step.
const RFC_TIME: u64 = 1_111_111_109;

struct Harness {
    storage: Arc<FakeStorage>,
    clock: Arc<FakeClock>,
    workspace: Workspace,
    vault: VaultId,
}

fn harness() -> Harness {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), TOTP_KDBX));
    let clock = Arc::new(FakeClock::at_unix_seconds(RFC_TIME));
    let mut workspace = Workspace::with_clock(storage.clone(), clock.clone());
    let vault = workspace
        .open_vault(&personal_vault(), &CompositeKey::password(PASSWORD))
        .expect("fixture opens");
    Harness {
        storage,
        clock,
        workspace,
        vault,
    }
}

impl Harness {
    fn entry(&self, title: &str) -> EntryId {
        self.workspace
            .vault(self.vault)
            .unwrap()
            .vault()
            .root()
            .all_entries()
            .find(|entry| entry.title() == title)
            .unwrap_or_else(|| panic!("no Entry {title}"))
            .id()
    }

    fn code(&self, title: &str) -> Option<Result<(String, u32), TotpError>> {
        let code = self.workspace.totp_code(self.vault, self.entry(title))?;
        Some(code.map(|code| (code.code().to_owned(), code.remaining())))
    }
}

fn sha1(digits: u32) -> Totp {
    Totp::new(SECRET, TotpAlgorithm::Sha1, digits, 30).unwrap()
}

#[test]
fn shows_the_current_code_and_how_long_it_lasts() {
    let h = harness();
    assert_eq!(h.code("OTP URI"), Some(Ok(("081804".into(), 1))));
    h.clock.advance(Duration::from_secs(1));
    assert_eq!(h.code("OTP URI"), Some(Ok(("050471".into(), 30))));
    h.clock.advance(Duration::from_secs(12));
    assert_eq!(h.code("OTP URI"), Some(Ok(("050471".into(), 18))));
}

#[test]
fn entries_without_a_usable_totp_say_so() {
    let h = harness();
    assert_eq!(h.code("No TOTP"), None);
    assert_eq!(h.code("Broken"), Some(Err(TotpError::InvalidSecret)));
    let empty = Workspace::new(h.storage.clone());
    assert!(empty.totp_code(h.vault, h.entry("OTP URI")).is_none());
}

#[test]
fn setting_up_totp_is_an_edit_that_saves() {
    let mut h = harness();
    let id = h.entry("No TOTP");
    h.workspace.set_totp(h.vault, id, Some(&sha1(8))).unwrap();
    assert_eq!(h.code("No TOTP"), Some(Ok(("07081804".into(), 1))));
    assert!(h.workspace.vault(h.vault).unwrap().has_unsaved_changes());

    h.workspace.save_vault(h.vault).unwrap();
    let saved = Vault::open(
        &h.storage.file(&personal_vault()).unwrap(),
        &CompositeKey::password(PASSWORD),
    )
    .unwrap();
    let entry = saved.entry(id).unwrap();
    assert_eq!(entry.totp(), Some(Ok(&sha1(8))));
    assert_eq!(
        entry.last_modified().map(|time| time.unix_seconds()),
        Some(RFC_TIME as i64)
    );
}

#[test]
fn totp_can_be_removed() {
    let mut h = harness();
    let id = h.entry("Legacy");
    h.workspace.set_totp(h.vault, id, None).unwrap();
    assert_eq!(h.code("Legacy"), None);
}

#[test]
fn setting_totp_needs_an_open_vault() {
    let h = harness();
    let mut empty = Workspace::new(h.storage.clone());
    assert_eq!(
        empty.set_totp(h.vault, h.entry("No TOTP"), Some(&sha1(6))),
        Err(EditVaultError::VaultNotOpen)
    );
}
