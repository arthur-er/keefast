//! Registering keefast's native-messaging host with Chromium-family browsers, so the browser
//! extension can start it.
//!
//! keefast registers under the KeePassXC-Browser extension's host name, so the stock
//! KeePassXC-Browser extension talks to keefast (ADR 0003). That takes the place of a
//! KeePassXC host registered for the same browsers.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::atomic_file;

/// The native-messaging host name the KeePassXC-Browser extension connects to.
pub const HOST_NAME: &str = "org.keepassxc.keepassxc_browser";

/// The KeePassXC-Browser extension, from the Chrome Web Store and from Microsoft Edge Add-ons.
pub const ALLOWED_ORIGINS: [&str; 2] = [
    "chrome-extension://oboonakemofpalcgghocfoadofidjkkk/",
    "chrome-extension://pdffhmdngciaglkoonimfcmckehcpafo/",
];

/// The native host's executable name, without the platform's extension.
pub const HOST_BINARY: &str = "keefast-native-host";

/// Registers and unregisters keefast's native host. Injected into the Workspace.
pub trait BrowserHosts: Send + Sync {
    /// Registers the host with every supported browser that is installed, and says which.
    fn install(&self) -> io::Result<Vec<Browser>>;

    /// Removes keefast's registrations, leaving any other program's alone.
    fn uninstall(&self) -> io::Result<()>;
}

/// The Chromium-family browsers keefast integrates with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Chromium,
    Brave,
    Edge,
    Vivaldi,
}

impl Browser {
    pub const ALL: [Browser; 5] = [
        Browser::Chrome,
        Browser::Chromium,
        Browser::Brave,
        Browser::Edge,
        Browser::Vivaldi,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "Chrome",
            Browser::Chromium => "Chromium",
            Browser::Brave => "Brave",
            Browser::Edge => "Edge",
            Browser::Vivaldi => "Vivaldi",
        }
    }

    /// The browser's directory under the user's config directory on Linux.
    pub fn linux_config_dir(self) -> &'static str {
        match self {
            Browser::Chrome => "google-chrome",
            Browser::Chromium => "chromium",
            Browser::Brave => "BraveSoftware/Brave-Browser",
            Browser::Edge => "microsoft-edge",
            Browser::Vivaldi => "vivaldi",
        }
    }

    /// The browser's own registry key under `HKEY_CURRENT_USER` on Windows, which exists once
    /// it has run.
    pub fn windows_registry_key(self) -> &'static str {
        match self {
            Browser::Chrome => r"Software\Google\Chrome",
            Browser::Chromium => r"Software\Chromium",
            Browser::Brave => r"Software\BraveSoftware\Brave-Browser",
            Browser::Edge => r"Software\Microsoft\Edge",
            Browser::Vivaldi => r"Software\Vivaldi",
        }
    }

    /// The registry keys, under `HKEY_CURRENT_USER`, the browser may look up native hosts in on
    /// Windows. Brave and Vivaldi have read Chrome's, so keefast registers in both, as KeePassXC
    /// registers them in Chrome's.
    pub fn windows_host_keys(self) -> Vec<String> {
        let own = format!(r"{}\NativeMessagingHosts", self.windows_registry_key());
        match self {
            Browser::Brave | Browser::Vivaldi => vec![
                own,
                format!(
                    r"{}\NativeMessagingHosts",
                    Browser::Chrome.windows_registry_key()
                ),
            ],
            _ => vec![own],
        }
    }
}

/// Native-messaging host manifests as the browsers look for them: in each browser's
/// `NativeMessagingHosts` directory on Linux; in keefast's own directory, pointed to from each
/// browser's registry key, on Windows.
pub struct NativeMessagingHosts {
    host_binary: PathBuf,
    /// The user's config directory on Linux; keefast's data directory on Windows.
    dir: PathBuf,
}

impl NativeMessagingHosts {
    pub fn new(host_binary: PathBuf, dir: PathBuf) -> Self {
        Self { host_binary, dir }
    }

    /// For the current user, with the native host installed next to the running app.
    pub fn for_current_user() -> Option<Self> {
        let app = std::env::current_exe().ok()?;
        let host = app
            .with_file_name(HOST_BINARY)
            .with_extension(std::env::consts::EXE_EXTENSION);
        #[cfg(windows)]
        let dir = dirs::data_dir()?.join("keefast");
        #[cfg(not(windows))]
        let dir = dirs::config_dir()?;
        Some(Self::new(host, dir))
    }

    /// The manifest that tells a browser how to start keefast's native host.
    pub fn manifest(&self) -> String {
        let manifest = json!({
            "name": HOST_NAME,
            "description": "keefast browser integration",
            "path": self.host_binary,
            "type": "stdio",
            "allowed_origins": ALLOWED_ORIGINS,
        });
        serde_json::to_string_pretty(&manifest).expect("JSON values serialise")
    }

    #[cfg(not(windows))]
    fn manifest_path(&self, browser: Browser) -> PathBuf {
        self.dir
            .join(browser.linux_config_dir())
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json"))
    }
}

/// Whether the manifest at `path` starts keefast's native host, wherever it is installed.
fn is_keefast_manifest(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    manifest["path"]
        .as_str()
        .and_then(|host| Path::new(host).file_stem())
        .is_some_and(|stem| stem == HOST_BINARY)
}

#[cfg(not(windows))]
impl BrowserHosts for NativeMessagingHosts {
    /// Writes the manifest for every browser whose config directory exists.
    fn install(&self) -> io::Result<Vec<Browser>> {
        let mut installed = Vec::new();
        for browser in Browser::ALL {
            if !self.dir.join(browser.linux_config_dir()).is_dir() {
                continue;
            }
            let path = self.manifest_path(browser);
            fs::create_dir_all(path.parent().expect("manifests are in a directory"))?;
            atomic_file::replace(&path, self.manifest().as_bytes())?;
            installed.push(browser);
        }
        Ok(installed)
    }

    fn uninstall(&self) -> io::Result<()> {
        for browser in Browser::ALL {
            let path = self.manifest_path(browser);
            if is_keefast_manifest(&path) {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
impl BrowserHosts for NativeMessagingHosts {
    /// Writes the manifest once and points the registry keys of every browser that has run
    /// to it.
    fn install(&self) -> io::Result<Vec<Browser>> {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        let path = self.dir.join(format!("{HOST_NAME}.json"));
        fs::create_dir_all(&self.dir)?;
        atomic_file::replace(&path, self.manifest().as_bytes())?;
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let installed: Vec<Browser> = Browser::ALL
            .into_iter()
            .filter(|browser| user.open_subkey(browser.windows_registry_key()).is_ok())
            .collect();
        for browser in &installed {
            for key in browser.windows_host_keys() {
                let (key, _) = user.create_subkey(format!(r"{key}\{HOST_NAME}"))?;
                key.set_value("", &path.as_os_str())?;
            }
        }
        Ok(installed)
    }

    fn uninstall(&self) -> io::Result<()> {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        let user = RegKey::predef(HKEY_CURRENT_USER);
        for key in Browser::ALL
            .iter()
            .flat_map(|browser| browser.windows_host_keys())
        {
            let key = format!(r"{key}\{HOST_NAME}");
            let Ok(registered) = user.open_subkey(&key) else {
                continue;
            };
            let manifest: String = registered.get_value("").unwrap_or_default();
            if is_keefast_manifest(Path::new(&manifest)) {
                user.delete_subkey_all(&key)?;
            }
        }
        let path = self.dir.join(format!("{HOST_NAME}.json"));
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}
