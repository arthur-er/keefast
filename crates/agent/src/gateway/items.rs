//! The Entry mapping (ADR 0015): which Entries a Gateway lists, and as what. It is pure, so the
//! rules live and are tested here, and the Gateway's client stays thin.
//!
//! - **login**: an `https` URL and a non-empty password. Its websites are that URL and each
//!   `https` `KP2A_URL*` field, matched exactly; its username is the Entry's.
//! - **key**: tagged `key` (in any case), or with no URL and an `UPPER_SNAKE` title, which names
//!   the key. A `key`-tagged Entry whose title isn't `UPPER_SNAKE` is left out.
//! - A login wins over a key. Anything else is left out, and no item ever carries a value,
//!   the notes or a custom field.

use keefast_core::{Entry, EntryId, Timestamp};
use secrecy::ExposeSecret;
use serde::Serialize;

/// An Entry as a Gateway lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GatewayItem {
    /// The Entry's UUID, hyphenated in lower case: a rename, a move or a Merge keeps it.
    #[serde(rename = "ref")]
    pub reference: String,
    pub title: String,
    /// When the Entry last changed, in ISO 8601.
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(flatten)]
    pub details: ItemDetails,
}

/// What kind of item it is, with what only that kind has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemDetails {
    Login {
        username: String,
        websites: Vec<Website>,
    },
    Key {
        #[serde(rename = "keyName")]
        key_name: String,
    },
}

/// A site a login fills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Website {
    pub url: String,
    #[serde(rename = "match")]
    pub matching: Matching,
}

/// How a Gateway matches a website against a page: only exactly, for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Matching {
    Exact,
}

/// The item `entry` is listed as, or `None` when it is left out.
pub fn gateway_item(entry: &Entry) -> Option<GatewayItem> {
    let details = login(entry).or_else(|| key(entry))?;
    Some(GatewayItem {
        reference: reference(entry.id()),
        title: entry.title().to_owned(),
        updated_at: entry
            .last_modified()
            .or(entry.created())
            .unwrap_or_else(|| Timestamp::from_unix_seconds(0))
            .to_iso_8601(),
        details,
    })
}

/// The tag that makes an Entry with a password a login even without an `https` URL: what a
/// Gateway's `POST /logins` with no website makes, so the login it was answered is listed.
pub const LOGIN_TAG: &str = "login";

fn login(entry: &Entry) -> Option<ItemDetails> {
    let site = is_https(entry.url()) || has_tag(entry, LOGIN_TAG);
    if !site || entry.password().expose_secret().is_empty() {
        return None;
    }
    let websites = std::iter::once(entry.url())
        .chain(entry.additional_urls().iter().map(String::as_str))
        .filter(|url| is_https(url))
        .map(|url| Website {
            url: url.to_owned(),
            matching: Matching::Exact,
        })
        .collect();
    Some(ItemDetails::Login {
        username: entry.username().to_owned(),
        websites,
    })
}

fn key(entry: &Entry) -> Option<ItemDetails> {
    let tagged = has_tag(entry, "key");
    let unnamed_site = entry.url().trim().is_empty();
    let title = entry.title();
    ((tagged || unnamed_site) && is_upper_snake(title)).then(|| ItemDetails::Key {
        key_name: title.to_owned(),
    })
}

/// Whether `entry` is tagged `tag`, in any case.
fn has_tag(entry: &Entry, tag: &str) -> bool {
    entry.tags().iter().any(|t| t.eq_ignore_ascii_case(tag))
}

/// `https://` and a host: a password is never typed into a page that isn't encrypted.
pub(super) fn is_https(url: &str) -> bool {
    url::Url::parse(url.trim())
        .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some_and(|h| !h.is_empty()))
}

/// `^[A-Z][A-Z0-9_]*$`: the name a command's environment knows a key by.
fn is_upper_snake(title: &str) -> bool {
    let mut chars = title.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// An Entry's UUID as a Gateway names it: hyphenated, in lower case.
pub fn reference(entry: EntryId) -> String {
    let hex = entry.to_hex();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
