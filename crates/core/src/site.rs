//! Which Entries a web page may be offered: URL matching by registrable domain, as
//! KeePassXC-Browser does it.
//!
//! An Entry URL is for a page when the page's host is the Entry's host or one of its
//! subdomains, without crossing a public suffix (the Public Suffix List decides where those
//! are). An `https` Entry is never for an `http` page, and an Entry URL with a port is only for
//! that port. IP addresses and hosts that are public suffixes themselves match only exactly.

use url::{Host, Url};

/// A web page asking for credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    https: bool,
    host: Host<String>,
    port: u16,
}

impl Site {
    /// The page at `url`, if it is an `http` or `https` URL with a host.
    pub fn parse(url: &str) -> Option<Self> {
        let url = Url::parse(url.trim()).ok()?;
        let (https, host) = web_host(&url)?;
        Some(Self {
            https,
            host,
            port: url.port_or_known_default()?,
        })
    }

    /// The page's host, which Access decisions are remembered for.
    pub fn host(&self) -> String {
        self.host.to_string()
    }

    /// Whether the Entry URL `entry_url` is for this page. An Entry URL without a scheme is
    /// taken to be `https`.
    pub fn matches(&self, entry_url: &str) -> bool {
        let entry_url = entry_url.trim();
        if entry_url.is_empty() {
            return false;
        }
        let parsed = if entry_url.contains("://") {
            Url::parse(entry_url)
        } else {
            Url::parse(&format!("https://{entry_url}"))
        };
        let Ok(entry) = parsed else {
            return false;
        };
        let Some((https, host)) = web_host(&entry) else {
            return false;
        };
        if https && !self.https {
            return false;
        }
        if entry.port().is_some_and(|port| port != self.port) {
            return false;
        }
        match (&host, &self.host) {
            (Host::Domain(entry), Host::Domain(site)) => {
                site == entry
                    || (site.ends_with(&format!(".{entry}")) && psl::domain_str(entry).is_some())
            }
            (entry, site) => entry == site,
        }
    }
}

/// Whether the URL is `https` (otherwise `http`), and its host, without a trailing dot. `None`
/// for other schemes and URLs without a host.
fn web_host(url: &Url) -> Option<(bool, Host<String>)> {
    let https = match url.scheme() {
        "https" => true,
        "http" => false,
        _ => return None,
    };
    let host = match url.host()?.to_owned() {
        Host::Domain(domain) => {
            let domain = domain.strip_suffix('.').unwrap_or(&domain).to_owned();
            if domain.is_empty() {
                return None;
            }
            Host::Domain(domain)
        }
        ip => ip,
    };
    Some((https, host))
}
