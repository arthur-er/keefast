//! The `kficon:` protocol, which serves the web UI the pictures it draws for Entries: favicons
//! downloaded for their sites, and the Vaults' custom icons. They come from the Workspace's
//! memory as they are asked for, so nothing is written to disk (ADR 0004) and nothing remote is
//! loaded into the webview: favicons are downloaded by the backend, once the user turns that on.
//!
//! - `kficon://localhost/favicon/<vault>/<entry>?site=<origin>`: the favicon of the site the
//!   Entry's URL is for. The site is in the query so the URL changes with it, and the webview
//!   never shows a cached favicon of the site the Entry used to be for.
//! - `kficon://localhost/custom/<vault>/<icon>`: a custom icon, which never changes.
//!
//! On Windows, WebView2 reaches custom protocols as `http://kficon.localhost/…`.

use std::sync::Arc;

use keefast_core::{CustomIconId, EntryId};
use keefast_workspace::{VaultId, Workspace};
use tauri::http::{Response, StatusCode, header};

use crate::core::AppCore;

pub const SCHEME: &str = "kficon";

#[cfg(windows)]
const BASE: &str = "http://kficon.localhost";
#[cfg(not(windows))]
const BASE: &str = "kficon://localhost";

pub fn favicon_url(vault: VaultId, entry: EntryId, site_origin: &str) -> String {
    let site: String = url_component(site_origin);
    format!(
        "{BASE}/favicon/{}/{}?site={site}",
        vault.as_u64(),
        entry.to_hex()
    )
}

pub fn custom_icon_url(vault: VaultId, icon: CustomIconId) -> String {
    format!("{BASE}/custom/{}/{}", vault.as_u64(), icon.to_hex())
}

/// Percent-encodes everything but letters and digits.
fn url_component(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Answers a request for a picture.
pub fn respond(core: &Arc<AppCore>, path: &str) -> Response<Vec<u8>> {
    let png = core.read(|workspace| picture(workspace, path));
    match png {
        Some((png, cache)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, cache)
            .body(png)
            .expect("a valid response"),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Vec::new())
            .expect("a valid response"),
    }
}

/// The answer before the app has started.
pub fn respond_missing() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Vec::new())
        .expect("a valid response")
}

/// The PNG at `path`, with how long the webview may keep it.
fn picture(workspace: &Workspace, path: &str) -> Option<(Vec<u8>, &'static str)> {
    let mut parts = path.trim_start_matches('/').split('/');
    let kind = parts.next()?;
    let vault: u64 = parts.next()?.parse().ok()?;
    let id = parts.next()?;
    // Only unlocked Vaults that aren't waiting to lock show anything.
    let open = workspace
        .readable_vaults()
        .find(|open| open.id().as_u64() == vault)?;
    match kind {
        "favicon" => {
            let entry = EntryId::from_hex(id)?;
            let favicon = workspace.favicon(open.id(), entry)?;
            Some((favicon.png().to_vec(), "no-cache"))
        }
        "custom" => {
            let icon = CustomIconId::from_hex(id)?;
            let custom = open.vault().custom_icon(icon)?;
            Some((custom.data().to_vec(), "max-age=31536000, immutable"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_origin_is_escaped_in_the_url() {
        assert_eq!(
            url_component("https://example.com:8443"),
            "https%3A%2F%2Fexample%2Ecom%3A8443"
        );
    }
}
