//! Which certificates a client trusts for a Headless host (ADR 0012 update "Self-signed
//! certificates").
//!
//! By default the desktop's Approver client and `keefast-agent run`/`store` trust the usual
//! public roots, which a host behind `tailscale serve`, a reverse proxy with ACME, or ACME
//! certificates of its own satisfies. An operator with a self-signed certificate or their own
//! CA gives its PEM instead: the desktop keeps it with the Agent host, `run` and `store` read it
//! from the file `KEEFAST_AGENT_CA` names. That certificate is then the only one trusted for the
//! host; the public roots are not.

use std::sync::Arc;

use ureq::tls::{Certificate, PemItem, RootCerts, TlsConfig};

/// The certificates in `pem`: at least one, or why not.
pub fn certificates(pem: &str) -> Result<Vec<Certificate<'static>>, String> {
    let mut certificates = Vec::new();
    for item in ureq::tls::parse_pem(pem.as_bytes()) {
        match item {
            Ok(PemItem::Certificate(certificate)) => certificates.push(certificate),
            Ok(_) => {}
            Err(error) => return Err(format!("the certificate's PEM can't be read: {error}")),
        }
    }
    if certificates.is_empty() {
        return Err(
            "that isn't a certificate: give the host's certificate, or its CA's, as PEM \
             (-----BEGIN CERTIFICATE-----)"
                .to_owned(),
        );
    }
    Ok(certificates)
}

/// TLS that trusts `certificate`'s PEM, or the public roots without one. A PEM that can't be
/// read trusts nothing, so a connection fails rather than falling back to the public roots.
pub(crate) fn tls(certificate: Option<&str>) -> TlsConfig {
    let roots = match certificate {
        None => RootCerts::WebPki,
        Some(pem) => RootCerts::Specific(Arc::new(certificates(pem).unwrap_or_default())),
    };
    TlsConfig::builder().root_certs(roots).build()
}
