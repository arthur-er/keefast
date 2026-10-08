//! Favicon download: off by default, as keefast makes no network requests the user hasn't
//! agreed to. Turned on, each site of an unlocked Vault's Entries has its favicon downloaded
//! once and kept until no unlocked Vault has an Entry for it.

mod support;

use std::sync::{Arc, Mutex};

use keefast_core::{EntryFields, Site};
use keefast_workspace::{
    AppConfig, EntryScope, Favicon, FaviconSource, PendingFavicon, UnlockSecrets, VaultId,
    Workspace,
};
use support::{
    FakeClipboard, FakeConfigStore, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault,
    work_vault,
};

/// Hands out a made-up favicon for every site but those it is told have none, and records
/// which sites it was asked for.
#[derive(Default)]
struct FakeFavicons {
    asked: Mutex<Vec<String>>,
    without: Vec<&'static str>,
}

impl FakeFavicons {
    fn asked(&self) -> Vec<String> {
        let mut asked = self.asked.lock().unwrap().clone();
        asked.sort();
        asked
    }

    fn icon_for(origin: &str) -> Vec<u8> {
        format!("png of {origin}").into_bytes()
    }
}

impl FaviconSource for FakeFavicons {
    fn download(&self, site: &Site) -> Option<Favicon> {
        let origin = site.origin();
        self.asked.lock().unwrap().push(origin.clone());
        if self.without.contains(&origin.as_str()) {
            return None;
        }
        Some(Favicon::from_png(Self::icon_for(&origin)))
    }
}

struct Harness {
    source: Arc<FakeFavicons>,
    workspace: Workspace,
    personal: VaultId,
}

/// The fixture's Entries are for these sites, in both Vaults.
const SITES: [&str; 4] = [
    "https://db.example.com",
    "https://example.com",
    "https://github.com",
    "https://mail.example.org",
];

fn harness_with(source: FakeFavicons, download: bool) -> Harness {
    let storage = Arc::new(
        FakeStorage::default()
            .with_file(&personal_vault(), KEEPASSXC_KDBX4)
            .with_file(&work_vault(), KEEPASSXC_KDBX4),
    );
    let source = Arc::new(source);
    let config = AppConfig {
        download_favicons: download,
        ..AppConfig::default()
    };
    let mut workspace = Workspace::new(storage)
        .with_config(Arc::new(FakeConfigStore::holding(config)))
        .with_favicon_source(source.clone());
    let personal = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    Harness {
        source,
        workspace,
        personal,
    }
}

fn harness() -> Harness {
    harness_with(FakeFavicons::default(), true)
}

/// Runs downloads to the end, as the app does off the UI thread.
fn run(workspace: &mut Workspace, downloads: Vec<PendingFavicon>) {
    for download in downloads {
        workspace.finish_favicon_download(download.run());
    }
}

impl Harness {
    fn download(&mut self) {
        let downloads = self.workspace.begin_favicon_downloads();
        run(&mut self.workspace, downloads);
    }

    fn favicon_of(&self, vault: VaultId, title: &str) -> Option<Vec<u8>> {
        let entry = self
            .workspace
            .visible_entries(EntryScope::Vault(vault))
            .into_iter()
            .find(|listed| listed.entry.title() == title)
            .unwrap_or_else(|| panic!("no Entry {title}"))
            .entry
            .id();
        self.workspace
            .favicon(vault, entry)
            .map(|favicon| favicon.png().to_vec())
    }
}

#[test]
fn nothing_is_downloaded_while_favicon_download_is_off() {
    let mut h = harness_with(FakeFavicons::default(), false);

    assert!(h.workspace.begin_favicon_downloads().is_empty());
    assert!(h.source.asked().is_empty());
    assert_eq!(h.favicon_of(h.personal, "GitHub"), None);
}

#[test]
fn each_site_has_its_favicon_downloaded_once() {
    let mut h = harness();
    let work = h
        .workspace
        .open_vault(&work_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    h.download();
    h.download();

    assert_eq!(h.source.asked(), SITES);
    assert_eq!(
        h.favicon_of(h.personal, "GitHub"),
        Some(FakeFavicons::icon_for("https://github.com"))
    );
    assert_eq!(
        h.favicon_of(work, "GitHub"),
        Some(FakeFavicons::icon_for("https://github.com"))
    );
}

#[test]
fn a_site_being_downloaded_is_not_asked_for_again() {
    let mut h = harness();

    let first = h.workspace.begin_favicon_downloads();
    let again = h.workspace.begin_favicon_downloads();

    assert_eq!(first.len(), SITES.len());
    assert!(again.is_empty());
}

#[test]
fn a_site_without_a_favicon_is_not_asked_again() {
    let mut h = harness_with(
        FakeFavicons {
            without: vec!["https://github.com"],
            ..FakeFavicons::default()
        },
        true,
    );

    h.download();
    h.download();

    assert_eq!(h.favicon_of(h.personal, "GitHub"), None);
    assert_eq!(h.source.asked(), SITES);
}

#[test]
fn a_changed_entry_url_has_its_new_site_downloaded() {
    let mut h = harness();
    h.download();
    let github = h
        .workspace
        .visible_entries(EntryScope::Vault(h.personal))
        .into_iter()
        .find(|listed| listed.entry.title() == "GitHub")
        .unwrap()
        .entry
        .id();
    let fields = EntryFields {
        title: "GitHub".into(),
        url: "gitlab.com/users/sign_in".into(),
        ..EntryFields::default()
    };
    h.workspace
        .update_entry(h.personal, github, &fields)
        .unwrap();

    h.download();

    assert!(h.source.asked().contains(&"https://gitlab.com".to_owned()));
    assert_eq!(
        h.favicon_of(h.personal, "GitHub"),
        Some(FakeFavicons::icon_for("https://gitlab.com"))
    );
}

#[test]
fn a_site_no_entry_is_for_any_more_is_forgotten() {
    let mut h = harness();
    h.download();
    let github = h
        .workspace
        .visible_entries(EntryScope::Vault(h.personal))
        .into_iter()
        .find(|listed| listed.entry.title() == "GitHub")
        .unwrap()
        .entry
        .id();
    let with_url = |url: &str| EntryFields {
        title: "GitHub".into(),
        url: url.into(),
        ..EntryFields::default()
    };
    h.workspace
        .update_entry(h.personal, github, &with_url("gitlab.com"))
        .unwrap();
    h.download();

    h.workspace
        .update_entry(h.personal, github, &with_url("https://github.com/login"))
        .unwrap();
    h.download();

    let asked_github = h
        .source
        .asked()
        .iter()
        .filter(|origin| *origin == "https://github.com")
        .count();
    assert_eq!(asked_github, 2);
}

#[test]
fn turning_favicon_download_off_forgets_the_favicons() {
    let mut h = harness();
    h.download();

    h.workspace.set_download_favicons(false);

    assert_eq!(h.favicon_of(h.personal, "GitHub"), None);
    assert!(h.workspace.begin_favicon_downloads().is_empty());
}

#[test]
fn a_download_finishing_after_favicon_download_was_turned_off_is_dropped() {
    let mut h = harness();
    let in_flight = h.workspace.begin_favicon_downloads();

    h.workspace.set_download_favicons(false);
    h.workspace.set_download_favicons(true);
    run(&mut h.workspace, in_flight);

    assert_eq!(h.favicon_of(h.personal, "GitHub"), None);
}

#[test]
fn locking_one_vault_keeps_the_favicons_another_vault_still_shows() {
    let mut h = harness();
    let work = h
        .workspace
        .open_vault(&work_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    h.download();

    h.workspace
        .lock_vault(h.personal, &mut FakeClipboard::default());

    assert_eq!(
        h.favicon_of(work, "GitHub"),
        Some(FakeFavicons::icon_for("https://github.com"))
    );
}

#[test]
fn locking_every_vault_forgets_the_favicons() {
    let mut h = harness();
    h.download();

    h.workspace.lock_all(&mut FakeClipboard::default());
    let personal = h
        .workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    assert_eq!(h.favicon_of(personal, "GitHub"), None);
    h.download();
    assert_eq!(h.source.asked().len(), 2 * SITES.len());
}

#[test]
fn a_download_finishing_after_its_vault_locked_is_dropped() {
    let mut h = harness();
    let in_flight = h.workspace.begin_favicon_downloads();

    h.workspace.lock_all(&mut FakeClipboard::default());
    run(&mut h.workspace, in_flight);
    let personal = h
        .workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    assert_eq!(h.favicon_of(personal, "GitHub"), None);
}

#[test]
fn locked_vaults_have_nothing_downloaded() {
    let mut h = harness();
    h.workspace.lock_all(&mut FakeClipboard::default());

    assert!(h.workspace.begin_favicon_downloads().is_empty());
}

#[test]
fn without_a_favicon_source_nothing_is_downloaded() {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let config = AppConfig {
        download_favicons: true,
        ..AppConfig::default()
    };
    let mut workspace =
        Workspace::new(storage).with_config(Arc::new(FakeConfigStore::holding(config)));
    workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();

    assert!(workspace.begin_favicon_downloads().is_empty());
}
