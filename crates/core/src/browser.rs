//! The Vault's side of browser integration: its identity in the KeePassXC-Browser protocol and
//! the Pairings stored in it.
//!
//! Pairings live in the Vault's custom data under KeePassXC's keys (ADR 0005), so a Vault paired
//! in KeePassXC works in keefast and the other way round.

use sha2::{Digest, Sha256};

use crate::edit::{child_or_insert, push, text_element};
use crate::xml::Element;
use crate::{EditError, Timestamp, Vault};

/// KeePassXC stores each Pairing's identification key under this prefix plus the Pairing's name.
const PAIRING_KEY_PREFIX: &str = "KPXC_BROWSER_";
/// KeePassXC records when a Pairing was made under this prefix plus the Pairing's name.
const PAIRING_CREATED_PREFIX: &str = "_CREATED_";

/// A browser profile's Companion extension trusted by a Vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    /// The name the user gave the Pairing; the extension sends it as its `id`.
    pub name: String,
    /// The extension's identification public key, Base64-encoded.
    pub key: String,
}

impl Vault {
    /// The Vault's identity in the KeePassXC-Browser protocol, as KeePassXC computes it: the
    /// SHA-256, in hex, of the root Group's UUID in hex.
    pub fn browser_hash(&self) -> String {
        let uuid = self.root().id().0.simple().to_string();
        hex(&Sha256::digest(uuid.as_bytes()))
    }

    /// The Pairings stored in the Vault, in Vault file order.
    pub fn pairings(&self) -> Vec<Pairing> {
        self.custom_data()
            .filter_map(|(key, value)| {
                let name = key.strip_prefix(PAIRING_KEY_PREFIX)?;
                Some(Pairing {
                    name: name.to_owned(),
                    key: value,
                })
            })
            .collect()
    }

    /// Whether the Vault trusts the extension that identifies itself as `name` with `key`.
    pub fn is_paired_with(&self, name: &str, key: &str) -> bool {
        self.pairings()
            .iter()
            .any(|pairing| pairing.name == name && pairing.key == key)
    }

    /// Stores a new Pairing. Names are unique within a Vault, so a Pairing is never silently
    /// replaced.
    pub fn add_pairing(&mut self, name: &str, key: &str, now: Timestamp) -> Result<(), EditError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EditError::PairingNameEmpty);
        }
        if self.pairings().iter().any(|pairing| pairing.name == name) {
            return Err(EditError::PairingNameTaken);
        }
        let custom_data = self.custom_data_mut();
        push(
            custom_data,
            custom_data_item(&format!("{PAIRING_KEY_PREFIX}{name}"), key),
        );
        push(
            custom_data,
            custom_data_item(&format!("{PAIRING_CREATED_PREFIX}{name}"), &iso_8601(now)),
        );
        Ok(())
    }

    /// The Vault's custom data, `Meta/CustomData`, as key and value pairs.
    fn custom_data(&self) -> impl Iterator<Item = (String, String)> {
        self.file
            .document
            .child("Meta")
            .and_then(|meta| meta.child("CustomData"))
            .into_iter()
            .flat_map(|data| data.children_named("Item"))
            .map(|item| {
                let text = |name| item.child(name).map(Element::text).unwrap_or_default();
                (text("Key"), text("Value"))
            })
    }

    fn custom_data_mut(&mut self) -> &mut Element {
        let meta = child_or_insert(&mut self.file.document, "Meta", &[]);
        child_or_insert(meta, "CustomData", &[])
    }
}

fn custom_data_item(key: &str, value: &str) -> Element {
    let mut item = Element::new("Item");
    push(&mut item, text_element("Key", key));
    push(&mut item, text_element("Value", value));
    item
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `2026-10-07T23:15:22Z`, the form KeePassXC reads its Pairing dates in.
fn iso_8601(time: Timestamp) -> String {
    let seconds = time.unix_seconds();
    let (days, of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day / 60 % 60,
        of_day % 60
    )
}

/// The Gregorian date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_written_in_iso_8601() {
        assert_eq!(
            iso_8601(Timestamp::from_unix_seconds(1_791_414_922)),
            "2026-10-07T23:15:22Z"
        );
        assert_eq!(
            iso_8601(Timestamp::from_unix_seconds(951_825_600)),
            "2000-02-29T12:00:00Z"
        );
        assert_eq!(
            iso_8601(Timestamp::from_unix_seconds(0)),
            "1970-01-01T00:00:00Z"
        );
    }
}
