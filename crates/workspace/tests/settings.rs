//! The settings screen's settings: the theme, lock and clipboard timings and favicon download,
//! changed through the Workspace and kept in the app config, and the Vault browser's pane widths.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_workspace::{
    AppConfig, ConfigStore, CopyField, EntryScope, FileConfigStore, PaneWidths, ThemeSetting,
    UnlockSecrets, Workspace,
};
use support::{
    FakeClipboard, FakeClock, FakeConfigStore, FakeStorage, KEEPASSXC_KDBX4, PASSWORD,
    personal_vault,
};

fn workspace_with(store: Arc<FakeConfigStore>) -> (Workspace, Arc<FakeClock>) {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let clock = Arc::new(FakeClock::at_unix_seconds(1_800_000_000));
    let workspace = Workspace::with_clock(storage, clock.clone()).with_config(store);
    (workspace, clock)
}

fn load(text: &str) -> AppConfig {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    FileConfigStore::new(path).load().unwrap()
}

#[test]
fn the_theme_follows_the_os_until_the_user_picks_one() {
    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    assert_eq!(workspace.theme(), ThemeSetting::System);

    workspace.set_theme(ThemeSetting::Dark);

    assert_eq!(workspace.theme(), ThemeSetting::Dark);
    assert_eq!(store.saved().unwrap().theme, ThemeSetting::Dark);
}

#[test]
fn the_theme_is_kept_in_the_config_file() {
    assert_eq!(load("").theme, ThemeSetting::System);
    assert_eq!(load("theme = \"light\"\n").theme, ThemeSetting::Light);
    assert_eq!(load("theme = \"dark\"\n").theme, ThemeSetting::Dark);
    assert_eq!(
        load("theme = \"solarized-dark\"\n").theme,
        ThemeSetting::SolarizedDark
    );
    // A theme from a newer version doesn't fail the file.
    let config = load("theme = \"neon\"\nautosave = true\n");
    assert_eq!(config.theme, ThemeSetting::System);
    assert!(config.autosave);

    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("config.toml"));
    let config = AppConfig {
        theme: ThemeSetting::HighContrast,
        download_favicons: true,
        ..AppConfig::default()
    };
    store.save(&config).unwrap();
    assert_eq!(store.load().unwrap(), config);
}

#[test]
fn changing_the_idle_lock_takes_effect_now_and_is_remembered() {
    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, clock) = workspace_with(store.clone());
    workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    clock.advance(Duration::from_secs(60));

    workspace.set_idle_lock_after(Some(Duration::from_secs(5 * 60)));

    assert_eq!(
        workspace.idle_lock_after(),
        Some(Duration::from_secs(5 * 60))
    );
    assert_eq!(workspace.idle_lock_in(), Some(Duration::from_secs(4 * 60)));
    assert_eq!(
        store.saved().unwrap().idle_lock_after,
        Some(Duration::from_secs(5 * 60))
    );

    workspace.set_idle_lock_after(None);
    assert_eq!(workspace.idle_lock_in(), None);
    assert_eq!(store.saved().unwrap().idle_lock_after, None);
}

#[test]
fn locking_on_os_lock_can_be_changed_and_is_remembered() {
    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    assert!(workspace.lock_on_os_lock());

    workspace.set_lock_on_os_lock(false);

    assert!(!workspace.lock_on_os_lock());
    assert!(!store.saved().unwrap().lock_on_os_lock);
}

#[test]
fn changing_the_clipboard_timeout_applies_to_the_next_copy_and_is_remembered() {
    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    let vault = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    let entry = workspace.visible_entries(EntryScope::Vault(vault))[0]
        .entry
        .id();
    let mut clipboard = FakeClipboard::default();

    workspace.set_clipboard_clear_after(Some(Duration::from_secs(10)));
    workspace
        .copy_field(&mut clipboard, vault, entry, CopyField::Username)
        .unwrap();
    assert_eq!(
        workspace.clipboard_clear_after(),
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        workspace.clipboard_clears_in(),
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        store.saved().unwrap().clipboard_clear_after,
        Some(Duration::from_secs(10))
    );

    workspace.set_clipboard_clear_after(None);
    workspace
        .copy_field(&mut clipboard, vault, entry, CopyField::Username)
        .unwrap();
    assert_eq!(workspace.clipboard_clears_in(), None);
}

#[test]
fn favicon_download_is_off_until_turned_on() {
    assert!(!AppConfig::default().download_favicons);
    assert!(!load("").download_favicons);
    assert!(load("download_favicons = true\n").download_favicons);

    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    assert!(!workspace.downloads_favicons());

    workspace.set_download_favicons(true);

    assert!(workspace.downloads_favicons());
    assert!(store.saved().unwrap().download_favicons);
}

#[test]
fn pane_widths_are_remembered_as_dragged() {
    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    assert_eq!(workspace.pane_widths(), PaneWidths::default());

    let widths = PaneWidths {
        sidebar: 260,
        entry_list: 400,
    };
    workspace.set_pane_widths(widths);

    assert_eq!(workspace.pane_widths(), widths);
    assert_eq!(store.saved().unwrap().pane_widths, widths);
}

#[test]
fn pane_widths_are_kept_in_the_config_file() {
    assert_eq!(load("").pane_widths, PaneWidths::default());
    assert_eq!(
        load("sidebar_width = 260\nentry_list_width = 400\n").pane_widths,
        PaneWidths {
            sidebar: 260,
            entry_list: 400,
        }
    );
    // A width of 0 is the default.
    assert_eq!(
        load("sidebar_width = 0\n").pane_widths,
        PaneWidths::default()
    );

    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("config.toml"));
    let config = AppConfig {
        pane_widths: PaneWidths {
            sidebar: 180,
            entry_list: 500,
        },
        ..AppConfig::default()
    };
    store.save(&config).unwrap();
    assert_eq!(store.load().unwrap(), config);
}
