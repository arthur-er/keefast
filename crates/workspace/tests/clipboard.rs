//! Copying an Entry's username or password, and clearing it from the clipboard again, with
//! the clock and the clipboard faked.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{CompositeKey, EntryFields, EntryId, Totp, TotpAlgorithm};
use keefast_workspace::{
    AppConfig, ConfigStore, CopyError, CopyField, EntryScope, FileConfigStore, VaultId, Workspace,
};
use support::{
    FakeClipboard, FakeClock, FakeConfigStore, FakeStorage, KEEPASSXC_KDBX4, PASSWORD,
    personal_vault,
};

struct Harness {
    clock: Arc<FakeClock>,
    clipboard: FakeClipboard,
    workspace: Workspace,
    vault: VaultId,
    github: EntryId,
}

fn harness_with(config: AppConfig) -> Harness {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let clock = Arc::new(FakeClock::at_unix_seconds(1_800_000_000));
    let mut workspace = Workspace::with_clock(storage, clock.clone())
        .with_config(Arc::new(FakeConfigStore::holding(config)));
    let vault = workspace
        .open_vault(&personal_vault(), &CompositeKey::password(PASSWORD))
        .expect("fixture opens");
    let github = workspace
        .visible_entries(EntryScope::Vault(vault))
        .into_iter()
        .find(|listed| listed.entry.title() == "GitHub")
        .unwrap()
        .entry
        .id();
    Harness {
        clock,
        clipboard: FakeClipboard::default(),
        workspace,
        vault,
        github,
    }
}

fn harness() -> Harness {
    harness_with(AppConfig::default())
}

impl Harness {
    fn copy(&mut self, field: CopyField) {
        self.workspace
            .copy_field(&mut self.clipboard, self.vault, self.github, field)
            .expect("the Entry exists");
    }

    fn wait(&mut self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
        self.workspace.clear_clipboard_if_due(&mut self.clipboard);
    }

    fn github_password(&self) -> String {
        use secrecy::ExposeSecret;
        let entry = self.workspace.entry(self.vault, self.github).unwrap();
        entry.password().expose_secret().to_owned()
    }

    fn github_username(&self) -> String {
        let entry = self.workspace.entry(self.vault, self.github).unwrap();
        entry.username().to_owned()
    }
}

#[test]
fn copying_puts_the_username_or_password_on_the_clipboard() {
    let mut h = harness();

    h.copy(CopyField::Username);
    assert_eq!(h.clipboard.text(), Some(h.github_username().as_str()));

    h.copy(CopyField::Password);
    assert_eq!(h.clipboard.text(), Some(h.github_password().as_str()));
}

#[test]
fn a_copied_password_is_cleared_after_30_seconds() {
    let mut h = harness();
    h.copy(CopyField::Password);
    assert_eq!(
        h.workspace.clipboard_clears_in(),
        Some(Duration::from_secs(30))
    );

    h.wait(29);
    assert_eq!(h.clipboard.text(), Some(h.github_password().as_str()));
    assert_eq!(
        h.workspace.clipboard_clears_in(),
        Some(Duration::from_secs(1))
    );

    h.wait(1);
    assert_eq!(h.clipboard.text(), Some(""));
    assert_eq!(h.workspace.clipboard_clears_in(), None);
}

#[test]
fn a_copied_username_is_cleared_too() {
    let mut h = harness();
    h.copy(CopyField::Username);

    h.wait(30);

    assert_eq!(h.clipboard.text(), Some(""));
}

#[test]
fn what_the_user_copied_since_is_left_alone() {
    let mut h = harness();
    h.copy(CopyField::Password);
    h.clipboard.copy_elsewhere("a shopping list");

    h.wait(30);

    assert_eq!(h.clipboard.text(), Some("a shopping list"));
    assert_eq!(h.workspace.clipboard_clears_in(), None);
}

#[test]
fn copying_again_restarts_the_wait() {
    let mut h = harness();
    h.copy(CopyField::Username);
    h.wait(20);
    h.copy(CopyField::Password);

    h.wait(10);
    assert_eq!(h.clipboard.text(), Some(h.github_password().as_str()));

    h.wait(20);
    assert_eq!(h.clipboard.text(), Some(""));
}

#[test]
fn the_wait_is_configurable() {
    let mut h = harness_with(AppConfig {
        clipboard_clear_after: Some(Duration::from_secs(10)),
        ..AppConfig::default()
    });
    h.copy(CopyField::Password);

    h.wait(9);
    assert_eq!(h.clipboard.text(), Some(h.github_password().as_str()));

    h.wait(1);
    assert_eq!(h.clipboard.text(), Some(""));
}

#[test]
fn clearing_can_be_turned_off() {
    let mut h = harness_with(AppConfig {
        clipboard_clear_after: None,
        ..AppConfig::default()
    });
    h.copy(CopyField::Password);

    assert_eq!(h.workspace.clipboard_clears_in(), None);
    h.wait(3600);
    assert_eq!(h.clipboard.text(), Some(h.github_password().as_str()));
}

#[test]
fn copying_from_an_entry_that_is_gone_fails_and_leaves_the_clipboard_alone() {
    let mut h = harness();
    h.clipboard.copy_elsewhere("a shopping list");
    // Into the Recycle Bin, then out of it for good.
    h.workspace.delete_entry(h.vault, h.github).unwrap();
    h.workspace.delete_entry(h.vault, h.github).unwrap();

    let result = h
        .workspace
        .copy_field(&mut h.clipboard, h.vault, h.github, CopyField::Password);

    assert_eq!(result, Err(CopyError::NotFound));
    assert_eq!(h.clipboard.text(), Some("a shopping list"));
    assert_eq!(h.workspace.clipboard_clears_in(), None);
}

#[test]
fn an_empty_field_is_not_copied() {
    let mut h = harness();
    let mut fields = EntryFields::from(h.workspace.entry(h.vault, h.github).unwrap());
    fields.username = String::new();
    h.workspace
        .update_entry(h.vault, h.github, &fields)
        .unwrap();
    h.clipboard.copy_elsewhere("a shopping list");

    let result = h
        .workspace
        .copy_field(&mut h.clipboard, h.vault, h.github, CopyField::Username);

    assert_eq!(result, Err(CopyError::Empty));
    assert_eq!(h.clipboard.text(), Some("a shopping list"));
    assert_eq!(h.workspace.clipboard_clears_in(), None);
}

#[test]
fn quitting_clears_a_copied_password_at_once() {
    let mut h = harness();
    h.copy(CopyField::Password);

    h.workspace.clear_clipboard_now(&mut h.clipboard);

    assert_eq!(h.clipboard.text(), Some(""));
    assert_eq!(h.workspace.clipboard_clears_in(), None);
}

#[test]
fn quitting_leaves_what_the_user_copied_since_alone() {
    let mut h = harness();
    h.copy(CopyField::Password);
    h.clipboard.copy_elsewhere("a shopping list");

    h.workspace.clear_clipboard_now(&mut h.clipboard);

    assert_eq!(h.clipboard.text(), Some("a shopping list"));
}

#[test]
fn the_clipboard_wait_is_kept_in_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let wait_in_file = |text: &str| {
        std::fs::write(&path, text).unwrap();
        FileConfigStore::new(path.clone())
            .load()
            .unwrap()
            .clipboard_clear_after
    };

    assert_eq!(
        wait_in_file("clipboard_clear_seconds = 12\n"),
        Some(Duration::from_secs(12))
    );
    assert_eq!(wait_in_file("clipboard_clear_seconds = 0\n"), None);
    assert_eq!(wait_in_file(""), Some(Duration::from_secs(30)));
}

#[test]
fn a_copied_totp_code_is_cleared_like_a_password() {
    let mut h = harness();
    let totp = Totp::new("JBSWY3DPEHPK3PXP", TotpAlgorithm::Sha1, 6, 30).unwrap();
    h.workspace
        .set_totp(h.vault, h.github, Some(&totp))
        .unwrap();
    let code = h
        .workspace
        .totp_code(h.vault, h.github)
        .unwrap()
        .unwrap()
        .code()
        .to_owned();

    h.copy(CopyField::TotpCode);
    assert_eq!(h.clipboard.text(), Some(code.as_str()));
    assert_eq!(
        h.workspace.clipboard_clears_in(),
        Some(Duration::from_secs(30))
    );

    h.wait(30);

    assert_eq!(h.clipboard.text(), Some(""));
}

#[test]
fn an_entry_without_totp_has_no_code_to_copy() {
    let mut h = harness();

    let result = h
        .workspace
        .copy_field(&mut h.clipboard, h.vault, h.github, CopyField::TotpCode);

    assert_eq!(result, Err(CopyError::Empty));
    assert_eq!(h.clipboard.text(), None);
}
