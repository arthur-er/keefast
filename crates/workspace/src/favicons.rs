//! Favicons for Entry URLs, downloaded only once the user turns favicon download on: keefast
//! makes no network requests they haven't agreed to.
//!
//! Each site an unlocked Vault's Entries are for has its favicon downloaded once, off the UI
//! thread, and kept in memory only: a favicon cache on disk would list the Vaults' sites in the
//! clear (ADR 0004). A favicon is forgotten once no unlocked Vault has an Entry for its site, so
//! locking every Vault leaves none behind.

mod http;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use keefast_core::{EntryId, Site};

use crate::{VaultId, Workspace};

pub use http::HttpFavicons;

/// A site's favicon, as a PNG image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favicon(Arc<[u8]>);

impl Favicon {
    pub fn from_png(png: Vec<u8>) -> Self {
        Self(png.into())
    }

    pub fn png(&self) -> &[u8] {
        &self.0
    }
}

/// Downloads sites' favicons. Injected into the Workspace.
pub trait FaviconSource: Send + Sync {
    /// The favicon of `site`, or `None` when it has none or it can't be had. Blocks.
    fn download(&self, site: &Site) -> Option<Favicon>;
}

#[derive(Default)]
pub(crate) struct Favicons {
    source: Option<Arc<dyn FaviconSource>>,
    /// By site origin.
    sites: HashMap<String, SiteFavicon>,
}

enum SiteFavicon {
    Downloading,
    /// The favicon, if the site had one. A site without one, or that couldn't be reached, is
    /// not asked again until its favicon is forgotten, as when every Vault locks.
    Downloaded(Option<Favicon>),
}

/// A favicon to download off the UI thread: [`Self::run`] it, then hand the result back with
/// [`Workspace::finish_favicon_download`].
pub struct PendingFavicon {
    site: Site,
    source: Arc<dyn FaviconSource>,
}

impl PendingFavicon {
    /// Downloads the favicon. Blocks on the network.
    pub fn run(self) -> DownloadedFavicon {
        let favicon = self.source.download(&self.site);
        DownloadedFavicon {
            origin: self.site.origin(),
            favicon,
        }
    }
}

/// The outcome of a [`PendingFavicon`].
pub struct DownloadedFavicon {
    origin: String,
    favicon: Option<Favicon>,
}

impl Workspace {
    /// Downloads favicons through `source` from now on, while favicon download is on.
    pub fn with_favicon_source(mut self, source: Arc<dyn FaviconSource>) -> Self {
        self.favicons.source = Some(source);
        self
    }

    pub fn downloads_favicons(&self) -> bool {
        self.config.download_favicons
    }

    /// Turns favicon download on or off, and remembers the choice in the app config. Turned
    /// off, the favicons downloaded are forgotten, and those still downloading are dropped.
    pub fn set_download_favicons(&mut self, download: bool) {
        self.config.download_favicons = download;
        if !download {
            self.favicons.sites.clear();
        }
        self.save_config();
    }

    /// Starts downloading the favicons of the sites the unlocked Vaults' Entries are for that
    /// aren't downloaded or downloading yet, with favicon download on. Call it when Vaults
    /// unlock and Entries change, and run the downloads off the UI thread.
    pub fn begin_favicon_downloads(&mut self) -> Vec<PendingFavicon> {
        let Some(source) = self.favicons.source.clone() else {
            return Vec::new();
        };
        if !self.config.download_favicons {
            return Vec::new();
        }
        let sites: HashMap<String, Site> = self
            .entry_sites()
            .map(|site| (site.origin(), site))
            .collect();
        // Sites no Entry is for any more, as after an Entry's URL changed, are forgotten.
        self.favicons
            .sites
            .retain(|origin, _| sites.contains_key(origin));
        let new: Vec<(String, Site)> = sites
            .into_iter()
            .filter(|(origin, _)| !self.favicons.sites.contains_key(origin))
            .collect();
        new.into_iter()
            .map(|(origin, site)| {
                self.favicons.sites.insert(origin, SiteFavicon::Downloading);
                PendingFavicon {
                    site,
                    source: Arc::clone(&source),
                }
            })
            .collect()
    }

    /// Keeps a downloaded favicon, unless it is no longer wanted: favicon download was turned
    /// off, or no unlocked Vault has an Entry for its site any more, since it started.
    pub fn finish_favicon_download(&mut self, downloaded: DownloadedFavicon) {
        if let Some(slot @ SiteFavicon::Downloading) =
            self.favicons.sites.get_mut(&downloaded.origin)
        {
            *slot = SiteFavicon::Downloaded(downloaded.favicon);
        }
    }

    /// The favicon of the site an Entry's URL is for, once downloaded.
    pub fn favicon(&self, vault: VaultId, entry: EntryId) -> Option<Favicon> {
        let site = Site::of_entry_url(self.entry(vault, entry)?.url())?;
        match self.favicons.sites.get(&site.origin())? {
            SiteFavicon::Downloaded(favicon) => favicon.clone(),
            SiteFavicon::Downloading => None,
        }
    }

    /// Forgets the favicons of sites no unlocked Vault has an Entry for, as after locking.
    pub(crate) fn forget_unused_favicons(&mut self) {
        if self.favicons.sites.is_empty() {
            return;
        }
        let used: HashSet<String> = self.entry_sites().map(|site| site.origin()).collect();
        self.favicons
            .sites
            .retain(|origin, _| used.contains(origin));
    }

    /// The sites of the unlocked Vaults' Entries, with repeats, leaving out Vaults waiting to lock.
    fn entry_sites(&self) -> impl Iterator<Item = Site> + '_ {
        // A Vault waiting to lock is hidden like a locked one: no site of it is looked up.
        self.readable_vaults()
            .flat_map(|open| open.vault.root().all_entries())
            .filter_map(|entry| Site::of_entry_url(entry.url()))
    }
}
