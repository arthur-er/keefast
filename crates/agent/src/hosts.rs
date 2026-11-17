//! Hosts for `http_request` (ADR 0012 "http_request"): an Entry's allowed hosts, how a request's
//! host is matched against them, and which addresses count as private.
//!
//! An allowed host is a host name, an IP address, or `*.` and a domain, which allows that
//! domain's subdomains, but not the domain itself. A host matched exactly is *allowed by name*:
//! only then may it be reached at a private, loopback or link-local address, as a NAS on the
//! user's own network is. A host matched by a wildcard may not, so a subdomain whose DNS
//! someone else controls can't point keefast, and the secret, at the host's own services.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Host;

/// `input` as an allowed host: lower case, without a trailing dot, scheme or path, and with
/// IPv6 addresses in brackets. `None` when it isn't a host, or has a port.
pub fn normalize_allowed_host(input: &str) -> Option<String> {
    let input = input.trim();
    let host = match input.split_once("://") {
        Some(_) => {
            let url = url::Url::parse(input).ok()?;
            if url.port().is_some() {
                return None;
            }
            url.host_str()?.to_owned()
        }
        None => input.to_owned(),
    };
    let host = host.to_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    if let Some(domain) = host.strip_prefix("*.") {
        // `*.com` would allow a whole top-level domain.
        let domain = normalize_host(domain)?;
        return (is_domain(&domain) && domain.contains('.')).then(|| format!("*.{domain}"));
    }
    normalize_host(host)
}

/// A host name or address as a request's URL has it, normalized as allowed hosts are.
fn normalize_host(host: &str) -> Option<String> {
    if let Ok(v6) = host.parse::<Ipv6Addr>() {
        return Some(format!("[{v6}]"));
    }
    let parsed = Host::parse(host).ok()?;
    match parsed {
        Host::Domain(domain) => {
            let domain = domain.strip_suffix('.').unwrap_or(&domain).to_owned();
            is_domain(&domain).then_some(domain)
        }
        Host::Ipv4(v4) => Some(v4.to_string()),
        Host::Ipv6(v6) => Some(format!("[{v6}]")),
    }
}

/// Whether `domain` is made of labels of letters, digits, `-` and `_`, as DNS names are once
/// IDNA has made them ASCII.
fn is_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// The host of a request's URL as allowed hosts are written.
pub(crate) fn url_host(url: &url::Url) -> Option<String> {
    match url.host()? {
        Host::Domain(domain) => normalize_host(domain),
        Host::Ipv4(v4) => Some(v4.to_string()),
        Host::Ipv6(v6) => Some(format!("[{v6}]")),
    }
}

/// How an Entry's allowed hosts allow a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Allowed {
    /// Named in the list: it may be at a private address.
    ByName,
    /// Matched by a `*.` wildcard: it must be at a public address.
    ByWildcard,
}

/// Whether `allowed` lets a secret go to `host`, and how.
pub(crate) fn allows(allowed: &[String], host: &str) -> Option<Allowed> {
    if allowed.iter().any(|rule| rule == host) {
        return Some(Allowed::ByName);
    }
    allowed
        .iter()
        .filter_map(|rule| rule.strip_prefix("*."))
        .any(|domain| {
            host.strip_suffix(domain)
                .is_some_and(|sub| sub.ends_with('.') && sub.len() > 1)
        })
        .then_some(Allowed::ByWildcard)
}

/// Whether `address` is anything but a public unicast address: private, loopback, link-local,
/// shared (carrier-grade NAT), unspecified, multicast, broadcast, documentation or reserved,
/// including IPv4 addresses inside IPv6 ones.
pub fn is_private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_v4(v4);
            }
            let segments = v6.segments();
            // NAT64 (64:ff9b::/96) reaches the IPv4 address in its last 32 bits.
            if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                let [.., hi, lo] = segments;
                return is_private_v4(Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo)));
            }
            v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // IPv4-compatible (::a.b.c.d), deprecated, and anything else in ::/96.
                || segments[..6] == [0; 6]
                || (segments[0] & 0xfe00) == 0xfc00 // unique local, fc00::/7
                || (segments[0] & 0xffc0) == 0xfe80 // link-local, fe80::/10
                || (segments[0] & 0xffc0) == 0xfec0 // site-local, fec0::/10
                || (segments[0] == 0x2001 && segments[1] == 0x0db8) // documentation
                || segments[0] == 0x2002 // 6to4, which may wrap any IPv4 address
                || (segments[0] == 0x2001 && segments[1] == 0) // Teredo, 2001::/32
                || segments[..3] == [0x64, 0xff9b, 1] // local-use NAT64, 64:ff9b:1::/48
        }
    }
}

fn is_private_v4(v4: Ipv4Addr) -> bool {
    let [a, b, c, _] = v4.octets();
    v4.is_unspecified()
        || v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        || a == 0 // "this network"
        || a >= 240 // reserved
        || (a == 100 && (64..128).contains(&b)) // shared address space, 100.64.0.0/10
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
}
