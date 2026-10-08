//! Several open Vaults, the Entry list across them, and the recent Vault files the app config
//! remembers between runs.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use keefast_workspace::{
    AppConfig, ConfigError, ConfigStore, Credentials, EntryScope, FileConfigStore, OpenVaultError,
    VaultId, VaultLocation, Workspace,
};
use support::{
    FakeConfigStore, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault, work_vault,
};

fn storage() -> Arc<FakeStorage> {
    Arc::new(
        FakeStorage::default()
            .with_file(&personal_vault(), KEEPASSXC_KDBX4)
            .with_file(&work_vault(), KEEPASSXC_KDBX4),
    )
}

fn key() -> Credentials {
    Credentials::password(PASSWORD)
}

fn workspace_with(config: Arc<dyn ConfigStore>) -> Workspace {
    Workspace::new(storage()).with_config(config)
}

fn open(workspace: &mut Workspace, location: &VaultLocation) -> VaultId {
    workspace.open_vault(location, &key()).expect("Vault opens")
}

fn vault_file(name: &str) -> VaultLocation {
    VaultLocation::Local(PathBuf::from(format!("/vaults/{name}.kdbx")))
}

#[test]
fn the_entry_list_shows_every_open_vault_or_just_one() {
    let mut workspace = Workspace::new(storage());
    let personal = open(&mut workspace, &personal_vault());
    let work = open(&mut workspace, &work_vault());

    let all = workspace.visible_entries(EntryScope::AllVaults);
    let listed: Vec<(VaultId, &str)> = all.iter().map(|e| (e.vault, e.entry.title())).collect();
    assert_eq!(
        listed,
        [
            (personal, "Email"),
            (work, "Email"),
            (personal, "GitHub"),
            (work, "GitHub"),
            (personal, "Prod DB"),
            (work, "Prod DB"),
            (personal, "Root Entry"),
            (work, "Root Entry"),
        ]
    );

    let only_work = workspace.visible_entries(EntryScope::Vault(work));
    assert_eq!(only_work.len(), 4);
    assert!(only_work.iter().all(|listed| listed.vault == work));
}

#[test]
fn the_entry_list_can_be_narrowed_to_a_group_of_one_vault() {
    let mut workspace = Workspace::new(storage());
    open(&mut workspace, &personal_vault());
    let work = open(&mut workspace, &work_vault());
    let group = workspace.vault(work).unwrap().vault().root().groups()[0].id();

    let entries = workspace.visible_entries(EntryScope::Group(work, group));

    let titles: Vec<&str> = entries.iter().map(|listed| listed.entry.title()).collect();
    assert_eq!(titles, ["GitHub", "Prod DB"]);
    assert!(entries.iter().all(|listed| listed.vault == work));
}

#[test]
fn opening_the_same_vault_file_twice_is_refused() {
    let mut workspace = Workspace::new(storage());
    let personal = open(&mut workspace, &personal_vault());

    let again = workspace.open_vault(&personal_vault(), &key());

    assert!(matches!(again, Err(OpenVaultError::AlreadyOpen(id)) if id == personal));
    assert_eq!(workspace.vaults().count(), 1);
    assert_eq!(workspace.open_vault_at(&personal_vault()), Some(personal));
}

#[test]
fn opened_vault_files_are_remembered_most_recent_first() {
    let config = Arc::new(FakeConfigStore::default());
    let mut workspace = workspace_with(config.clone());

    open(&mut workspace, &personal_vault());
    open(&mut workspace, &work_vault());

    assert_eq!(workspace.recent_vaults(), [work_vault(), personal_vault()]);
    assert_eq!(
        config.saved().unwrap().recent_vaults,
        [work_vault(), personal_vault()]
    );
}

#[test]
fn reopening_a_recent_vault_file_moves_it_to_the_front_without_duplicating_it() {
    let config = Arc::new(FakeConfigStore::holding(AppConfig {
        recent_vaults: vec![work_vault(), personal_vault()],
        ..AppConfig::default()
    }));
    let mut workspace = workspace_with(config.clone());

    open(&mut workspace, &personal_vault());

    assert_eq!(workspace.recent_vaults(), [personal_vault(), work_vault()]);
}

#[test]
fn a_vault_file_that_fails_to_open_is_not_remembered() {
    let config = Arc::new(FakeConfigStore::default());
    let mut workspace = workspace_with(config.clone());

    let wrong = workspace.open_vault(&personal_vault(), &Credentials::password("wrong"));

    assert!(wrong.is_err());
    assert!(workspace.recent_vaults().is_empty());
}

#[test]
fn only_the_most_recent_vault_files_are_kept() {
    let older: Vec<VaultLocation> = (0..20).map(|n| vault_file(&format!("old-{n}"))).collect();
    let config = Arc::new(FakeConfigStore::holding(AppConfig {
        recent_vaults: older.clone(),
        ..AppConfig::default()
    }));
    let mut workspace = workspace_with(config);

    open(&mut workspace, &personal_vault());

    let recent = workspace.recent_vaults();
    assert_eq!(recent.len(), Workspace::MAX_RECENT_VAULTS);
    assert_eq!(recent[0], personal_vault());
    assert_eq!(recent[1..], older[..Workspace::MAX_RECENT_VAULTS - 1]);
}

#[test]
fn a_recent_vault_file_can_be_forgotten() {
    let config = Arc::new(FakeConfigStore::holding(AppConfig {
        recent_vaults: vec![work_vault(), personal_vault()],
        ..AppConfig::default()
    }));
    let mut workspace = workspace_with(config.clone());

    workspace.forget_recent(&work_vault());

    assert_eq!(workspace.recent_vaults(), [personal_vault()]);
    assert_eq!(config.saved().unwrap().recent_vaults, [personal_vault()]);
}

#[test]
fn the_autosave_setting_is_remembered() {
    let config = Arc::new(FakeConfigStore::default());
    let mut workspace = workspace_with(config.clone());

    workspace.set_autosave(true);

    assert!(workspace_with(config).autosave());
}

#[test]
fn a_config_that_cannot_be_saved_does_not_stop_a_vault_from_opening() {
    let config = Arc::new(FakeConfigStore::default());
    let mut workspace = workspace_with(config.clone());
    config.fail(true);

    let id = workspace.open_vault(&personal_vault(), &key());

    assert!(id.is_ok());
    assert!(matches!(workspace.config_error(), Some(ConfigError::Io(_))));
    assert_eq!(workspace.recent_vaults(), [personal_vault()]);

    config.fail(false);
    open(&mut workspace, &work_vault());
    assert!(workspace.config_error().is_none());
}

#[test]
fn a_config_that_cannot_be_loaded_starts_empty_and_is_not_overwritten() {
    let config = Arc::new(FakeConfigStore::holding(AppConfig {
        recent_vaults: vec![work_vault()],
        autosave: true,
        browser_integration: true,
        ..AppConfig::default()
    }));
    config.fail(true);
    let mut workspace = workspace_with(config.clone());
    config.fail(false);

    open(&mut workspace, &personal_vault());

    assert!(workspace.config_error().is_some());
    assert!(!workspace.autosave());
    assert_eq!(config.saved().unwrap().recent_vaults, [work_vault()]);
}

#[test]
fn recent_vault_files_survive_a_restart_through_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keefast").join("config.toml");
    let store = || Arc::new(FileConfigStore::new(path.clone()));

    let mut first_run = workspace_with(store());
    open(&mut first_run, &personal_vault());
    open(&mut first_run, &work_vault());
    first_run.set_autosave(true);
    drop(first_run);

    let second_run = workspace_with(store());
    assert!(second_run.config_error().is_none());
    assert_eq!(second_run.recent_vaults(), [work_vault(), personal_vault()]);
    assert!(second_run.autosave());
}

#[test]
fn the_config_file_holds_locations_but_never_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut workspace = workspace_with(Arc::new(FileConfigStore::new(path.clone())));

    open(&mut workspace, &personal_vault());

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("/vaults/personal.kdbx"), "{text}");
    assert!(!text.contains(PASSWORD), "{text}");
}

#[test]
fn a_missing_config_file_is_an_empty_config() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileConfigStore::new(dir.path().join("nothing-here.toml"));

    assert_eq!(store.load().unwrap(), AppConfig::default());
}

#[test]
fn a_damaged_config_file_is_reported_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "recent_vaults = [[[").unwrap();
    let mut workspace = workspace_with(Arc::new(FileConfigStore::new(path.clone())));

    open(&mut workspace, &personal_vault());

    assert!(matches!(
        workspace.config_error(),
        Some(ConfigError::Parse(_))
    ));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "recent_vaults = [[["
    );
}

#[test]
fn the_config_file_lives_in_the_os_config_directory() {
    let Some(store) = FileConfigStore::in_os_config_dir() else {
        return;
    };

    assert!(store.path().ends_with("keefast/config.toml"));
}

#[test]
fn recent_vault_files_already_open_are_not_offered_for_reopening() {
    let config = Arc::new(FakeConfigStore::holding(AppConfig {
        recent_vaults: vec![work_vault(), personal_vault()],
        ..AppConfig::default()
    }));
    let mut workspace = workspace_with(config);

    open(&mut workspace, &personal_vault());

    let offered: Vec<&VaultLocation> = workspace.closed_recent_vaults().collect();
    assert_eq!(offered, [&work_vault()]);
}

#[test]
fn new_entries_go_to_the_shown_vault_or_the_only_open_one() {
    let mut workspace = Workspace::new(storage());
    let personal = open(&mut workspace, &personal_vault());
    let root = |workspace: &Workspace, id| workspace.vault(id).unwrap().vault().root().id();
    let personal_root = root(&workspace, personal);

    assert_eq!(
        workspace.group_for_new_items(EntryScope::AllVaults),
        Some((personal, personal_root)),
        "with one Vault open, showing all Vaults still has a place for new Entries"
    );

    let work = open(&mut workspace, &work_vault());
    let work_group = workspace.vault(work).unwrap().vault().root().groups()[0].id();
    assert_eq!(workspace.group_for_new_items(EntryScope::AllVaults), None);
    assert_eq!(
        workspace.group_for_new_items(EntryScope::Vault(work)),
        Some((work, root(&workspace, work)))
    );
    assert_eq!(
        workspace.group_for_new_items(EntryScope::Group(work, work_group)),
        Some((work, work_group))
    );
}

#[cfg(unix)]
#[test]
fn a_vault_file_reached_through_another_path_is_the_same_vault_file() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("personal.kdbx");
    std::fs::write(&real, KEEPASSXC_KDBX4).unwrap();
    let link = dir.path().join("link.kdbx");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut workspace = Workspace::new(Arc::new(keefast_workspace::LocalStorage))
        .with_config(Arc::new(FakeConfigStore::default()));

    let first = open(&mut workspace, &VaultLocation::Local(real.clone()));
    let through_link = workspace.open_vault(&VaultLocation::Local(link), &key());
    let dotted = dir.path().join(".").join("personal.kdbx");
    let through_dots = workspace.open_vault(&VaultLocation::Local(dotted), &key());

    assert!(matches!(through_link, Err(OpenVaultError::AlreadyOpen(id)) if id == first));
    assert!(matches!(through_dots, Err(OpenVaultError::AlreadyOpen(id)) if id == first));
    assert_eq!(workspace.recent_vaults().len(), 1);
}

#[test]
fn a_recent_vault_file_this_version_does_not_understand_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
autosave = true

[[recent_vaults]]
provider = "dropbox"
id = "id:abc123"

[[recent_vaults]]
provider = "local"
path = "/vaults/personal.kdbx"
"#,
    )
    .unwrap();

    let config = FileConfigStore::new(path).load().unwrap();

    assert!(config.autosave);
    assert_eq!(config.recent_vaults, [personal_vault()]);
}
