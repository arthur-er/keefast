//! Turning browser integration on and off: the native host's registration with the browsers,
//! and the setting remembered in the app config.

mod support;

use std::fs;
use std::sync::Arc;

use keefast_workspace::{AppConfig, Browser, ConfigStore, Workspace};
use support::{FakeBrowserHosts, FakeConfigStore, FakeStorage};

fn workspace(config: Arc<FakeConfigStore>, hosts: Arc<FakeBrowserHosts>) -> Workspace {
    Workspace::new(Arc::new(FakeStorage::default()))
        .with_config(config)
        .with_browser_hosts(hosts)
}

#[test]
fn browser_integration_is_off_until_turned_on() {
    let hosts = Arc::new(FakeBrowserHosts::default());
    let workspace = workspace(Arc::default(), hosts.clone());

    assert!(!workspace.browser_integration());
    assert_eq!(hosts.installs(), 0);
    assert!(workspace.browser_hosts_status().is_none());
}

#[test]
fn turning_it_on_registers_the_host_and_is_remembered() {
    let config = Arc::new(FakeConfigStore::default());
    let hosts = Arc::new(FakeBrowserHosts::default());
    let mut workspace = workspace(config.clone(), hosts.clone());

    workspace.set_browser_integration(true);

    assert!(workspace.browser_integration());
    assert_eq!(hosts.installs(), 1);
    let browsers = workspace.browser_hosts_status().unwrap().as_ref().unwrap();
    assert_eq!(browsers, &[Browser::Chromium, Browser::Brave]);
    assert!(config.saved().unwrap().browser_integration);
}

#[test]
fn turning_it_off_unregisters_the_host() {
    let config = Arc::new(FakeConfigStore::default());
    let hosts = Arc::new(FakeBrowserHosts::default());
    let mut workspace = workspace(config.clone(), hosts.clone());
    workspace.set_browser_integration(true);

    workspace.set_browser_integration(false);

    assert!(!workspace.browser_integration());
    assert_eq!(hosts.uninstalls(), 1);
    assert!(!config.saved().unwrap().browser_integration);
}

#[test]
fn when_on_the_host_is_registered_again_at_startup() {
    let config = Arc::new(FakeConfigStore::holding(AppConfig {
        browser_integration: true,
        ..AppConfig::default()
    }));
    let hosts = Arc::new(FakeBrowserHosts::default());

    let workspace = workspace(config, hosts.clone());

    assert!(workspace.browser_integration());
    assert_eq!(hosts.installs(), 1);
}

#[test]
fn a_failed_registration_is_reported() {
    let hosts = Arc::new(FakeBrowserHosts::default());
    hosts.fail(true);
    let mut workspace = workspace(Arc::default(), hosts);

    workspace.set_browser_integration(true);

    assert!(workspace.browser_hosts_status().unwrap().is_err());
    assert!(workspace.browser_integration());
}

#[test]
fn the_setting_survives_in_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = keefast_workspace::FileConfigStore::new(dir.path().join("config.toml"));
    store
        .save(&AppConfig {
            browser_integration: true,
            ..AppConfig::default()
        })
        .unwrap();

    assert!(store.load().unwrap().browser_integration);
    let text = fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("browser_integration = true"));
}

#[cfg(not(windows))]
mod linux {
    use std::path::Path;

    use keefast_workspace::{ALLOWED_ORIGINS, BrowserHosts, HOST_NAME, NativeMessagingHosts};
    use serde_json::Value;

    use super::*;

    const HOST: &str = "/opt/keefast/bin/keefast-native-host";

    fn manifest_at(config: &Path, browser_dir: &str) -> std::path::PathBuf {
        config
            .join(browser_dir)
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json"))
    }

    #[test]
    fn manifests_are_written_for_the_browsers_that_are_installed() {
        let config = tempfile::tempdir().unwrap();
        fs::create_dir_all(config.path().join("chromium")).unwrap();
        fs::create_dir_all(config.path().join("BraveSoftware/Brave-Browser")).unwrap();
        let hosts = NativeMessagingHosts::new(HOST.into(), config.path().into());

        let installed = hosts.install().unwrap();

        assert_eq!(installed, [Browser::Chromium, Browser::Brave]);
        let text = fs::read_to_string(manifest_at(config.path(), "chromium")).unwrap();
        let manifest: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(manifest["name"], "org.keepassxc.keepassxc_browser");
        assert_eq!(manifest["path"], HOST);
        assert_eq!(manifest["type"], "stdio");
        assert_eq!(
            manifest["allowed_origins"],
            serde_json::json!(ALLOWED_ORIGINS)
        );
        assert!(manifest_at(config.path(), "BraveSoftware/Brave-Browser").exists());
        assert!(!config.path().join("google-chrome").exists());
    }

    #[test]
    fn every_supported_browser_has_its_own_directory() {
        let config = tempfile::tempdir().unwrap();
        for browser in Browser::ALL {
            fs::create_dir_all(config.path().join(browser.linux_config_dir())).unwrap();
        }
        let hosts = NativeMessagingHosts::new(HOST.into(), config.path().into());

        assert_eq!(hosts.install().unwrap(), Browser::ALL);
        for dir in [
            "google-chrome",
            "chromium",
            "BraveSoftware/Brave-Browser",
            "microsoft-edge",
            "vivaldi",
        ] {
            assert!(manifest_at(config.path(), dir).exists(), "{dir}");
        }
    }

    #[test]
    fn uninstalling_removes_keefasts_manifests_only() {
        let config = tempfile::tempdir().unwrap();
        fs::create_dir_all(config.path().join("chromium")).unwrap();
        fs::create_dir_all(config.path().join("vivaldi")).unwrap();
        // keefast was installed somewhere else before.
        let old = NativeMessagingHosts::new(
            "/nix/store/old/bin/keefast-native-host".into(),
            config.path().into(),
        );
        old.install().unwrap();
        // Then KeePassXC registered its own host for Vivaldi.
        let keepassxc = manifest_at(config.path(), "vivaldi");
        fs::write(
            &keepassxc,
            r#"{"name": "org.keepassxc.keepassxc_browser", "path": "/usr/bin/keepassxc-proxy"}"#,
        )
        .unwrap();
        let hosts = NativeMessagingHosts::new(HOST.into(), config.path().into());

        hosts.uninstall().unwrap();

        assert!(!manifest_at(config.path(), "chromium").exists());
        assert!(keepassxc.exists());
    }
}
