//! Where a Headless host listens for Agents, and with what TLS (ADR 0012, ADR 0004 update
//! 2026-11-16).
//!
//! Agents reach the host over the network with a bearer token, so a token sent in the clear
//! to an address other machines can reach is a token anyone on the path can take. The host
//! therefore listens on loopback without TLS, where only this machine (or a TLS-terminating
//! proxy such as `tailscale serve` on it) can connect, and anywhere else only with TLS.

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Where `keefast-agent serve` listens unless told otherwise: loopback, so it is never open to
/// the network by accident.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7807";

/// The address a Headless host listens on, and its TLS files when it has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listen {
    pub address: SocketAddr,
    pub tls: Option<TlsFiles>,
}

/// The PEM files of the host's own TLS: its certificate chain and private key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// Why a listen address can't be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ListenError {
    #[error("--listen {0}: not an IP address and port, such as 127.0.0.1:7807 or [::]:7807")]
    NotAnAddress(String),
    #[error("{0} needs {1} too")]
    HalfTls(&'static str, &'static str),
    #[error(
        "--listen {0} is open to other machines, and Agents' tokens would cross the network in \
         the clear: give the host TLS with --tls-cert and --tls-key, or listen on loopback \
         (127.0.0.1 or [::1]) behind a proxy that does TLS"
    )]
    NeedsTls(SocketAddr),
}

impl Listen {
    /// The listen address as the operator wrote it, and the TLS files if they named them.
    /// An address other machines can reach is refused without TLS.
    pub fn new(
        address: &str,
        cert: Option<PathBuf>,
        key: Option<PathBuf>,
    ) -> Result<Self, ListenError> {
        let address: SocketAddr = address
            .parse()
            .map_err(|_| ListenError::NotAnAddress(address.to_owned()))?;
        let tls = match (cert, key) {
            (Some(cert), Some(key)) => Some(TlsFiles { cert, key }),
            (None, None) => None,
            (Some(_), None) => return Err(ListenError::HalfTls("--tls-cert", "--tls-key")),
            (None, Some(_)) => return Err(ListenError::HalfTls("--tls-key", "--tls-cert")),
        };
        if tls.is_none() && !address.ip().is_loopback() {
            return Err(ListenError::NeedsTls(address));
        }
        Ok(Self { address, tls })
    }

    /// The MCP endpoint's URL as an Agent on this machine would write it.
    pub fn url(&self) -> String {
        let scheme = if self.tls.is_some() { "https" } else { "http" };
        format!("{scheme}://{}/mcp", self.address)
    }
}

impl fmt::Display for Listen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_needs_no_tls() {
        for address in ["127.0.0.1:7807", "[::1]:7807", DEFAULT_LISTEN] {
            assert!(Listen::new(address, None, None).is_ok(), "{address}");
        }
    }

    #[test]
    fn any_other_address_needs_tls() {
        let tls = || (Some("cert.pem".into()), Some("key.pem".into()));
        for address in ["0.0.0.0:7807", "[::]:7807", "100.64.0.1:7807"] {
            let (cert, key) = tls();
            assert!(matches!(
                Listen::new(address, None, None),
                Err(ListenError::NeedsTls(_))
            ));
            assert_eq!(
                Listen::new(address, cert, key).unwrap().url(),
                format!("https://{address}/mcp")
            );
        }
    }
}
