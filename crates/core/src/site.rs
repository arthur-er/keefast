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

    /// The page's host, which Access decisions are remembered for: written as KeePassXC
    /// writes it, so IPv6 addresses go without brackets.
    pub fn host(&self) -> String {
        match &self.host {
            Host::Ipv6(address) => address.to_string(),
            host => host.to_string(),
        }
    }

    /// The site an Entry URL is for, if it is a web address. An Entry URL without a scheme is
    /// taken to be `https`.
    pub fn of_entry_url(entry_url: &str) -> Option<Self> {
        let entry = parse_entry_url(entry_url)?;
        let (https, host) = web_host(&entry)?;
        Some(Self {
            https,
            host,
            port: entry.port_or_known_default()?,
        })
    }

    /// The site's scheme, host and port, as in `https://example.com:8443`; the port only when
    /// it isn't the scheme's own.
    pub fn origin(&self) -> String {
        let (scheme, default_port) = match self.https {
            true => ("https", 443),
            false => ("http", 80),
        };
        match self.port == default_port {
            true => format!("{scheme}://{}", self.host),
            false => format!("{scheme}://{}:{}", self.host, self.port),
        }
    }

    /// Whether the Entry URL `entry_url` is for this page. An Entry URL without a scheme is
    /// taken to be `https`.
    pub fn matches(&self, entry_url: &str) -> bool {
        let Some(entry) = parse_entry_url(entry_url) else {
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

/// An Entry URL, taken to be `https` without a scheme. `None` when empty or not a URL.
fn parse_entry_url(entry_url: &str) -> Option<Url> {
    let entry_url = entry_url.trim();
    if entry_url.is_empty() {
        return None;
    }
    let parsed = if entry_url.contains("://") {
        Url::parse(entry_url)
    } else {
        Url::parse(&format!("https://{entry_url}"))
    };
    parsed.ok()
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
