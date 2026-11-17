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
    // KeeWeb's themes, from earlier versions, are read as the nearest of dark and light.
    for (theme, nearest) in [
        ("solarized-dark", ThemeSetting::Dark),
        ("dark-blue", ThemeSetting::Dark),
        ("dark-brown", ThemeSetting::Dark),
        ("terminal", ThemeSetting::Dark),
        ("dark-contrast", ThemeSetting::Dark),
        ("solarized-light", ThemeSetting::Light),
        ("light-blue", ThemeSetting::Light),
        ("light-brown", ThemeSetting::Light),
        ("terminal-light", ThemeSetting::Light),
        ("high-contrast", ThemeSetting::Light),
    ] {
        assert_eq!(
            load(&format!("theme = \"{theme}\"\n")).theme,
            nearest,
            "{theme}"
        );
    }
    // A theme from a newer version doesn't fail the file.
    let config = load("theme = \"neon\"\nautosave = true\n");
    assert_eq!(config.theme, ThemeSetting::System);
    assert!(config.autosave);

    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("config.toml"));
    let config = AppConfig {
        theme: ThemeSetting::Light,
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
fn closing_to_the_tray_is_off_until_turned_on() {
    assert!(!AppConfig::default().close_to_tray);
    assert!(!load("").close_to_tray);
    assert!(load("close_to_tray = true\n").close_to_tray);

    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    assert!(!workspace.close_to_tray());

    workspace.set_close_to_tray(true);

    assert!(workspace.close_to_tray());
    assert!(store.saved().unwrap().close_to_tray);

    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("config.toml"));
    let config = AppConfig {
        close_to_tray: true,
        ..AppConfig::default()
    };
    store.save(&config).unwrap();
    assert_eq!(store.load().unwrap(), config);
}

#[test]
fn checking_for_updates_waits_for_the_user_to_be_asked() {
    assert_eq!(AppConfig::default().check_for_updates, None);
    assert_eq!(load("").check_for_updates, None);
    assert_eq!(
        load("check_for_updates = false\n").check_for_updates,
        Some(false)
    );

    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    assert_eq!(workspace.check_for_updates(), None);

    workspace.set_check_for_updates(true);

    assert_eq!(workspace.check_for_updates(), Some(true));
    assert_eq!(store.saved().unwrap().check_for_updates, Some(true));

    // Unasked is left out of the file, and each answer is read back.
    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("config.toml"));
    for answer in [None, Some(true), Some(false)] {
        let config = AppConfig {
            check_for_updates: answer,
            ..AppConfig::default()
        };
        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap(), config);
    }
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

#[test]
fn shortcuts_the_user_changed_are_remembered_and_reset() {
    let store = Arc::new(FakeConfigStore::default());
    let (mut workspace, _) = workspace_with(store.clone());
    assert!(workspace.shortcuts().is_empty());

    workspace.set_shortcut("search", Some(vec!["Ctrl J".to_owned()]));
    assert_eq!(workspace.shortcuts()["search"], ["Ctrl J"]);
    assert_eq!(store.saved().unwrap().shortcuts["search"], ["Ctrl J"]);

    workspace.set_shortcut("search", None);
    assert!(workspace.shortcuts().is_empty());
    assert!(store.saved().unwrap().shortcuts.is_empty());
}

#[test]
fn shortcuts_are_kept_in_the_config_file() {
    assert!(load("").shortcuts.is_empty());
    let loaded = load("autosave = true\n\n[shortcuts]\nsearch = [\"Ctrl J\"]\n");
    assert_eq!(loaded.shortcuts["search"], ["Ctrl J"]);
    assert!(loaded.autosave);

    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("config.toml"));
    let mut config = AppConfig::default();
    config
        .shortcuts
        .insert("lockAll".to_owned(), vec!["Ctrl Shift L".to_owned()]);
    store.save(&config).unwrap();
    assert_eq!(store.load().unwrap().shortcuts, config.shortcuts);
}
