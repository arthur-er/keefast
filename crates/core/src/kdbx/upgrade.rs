//! Converts a KDBX 3.1 document into its KDBX 4 form, which is the one keefast keeps in memory
//! and writes. Only what KDBX 4 stores differently changes; everything else is left alone.

use std::collections::HashMap;
use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::Binary;
use crate::OpenError;
use crate::xml::{Element, Node, Secret};

/// The timestamps KeePass defines, as (parent, element) pairs. KDBX 3.1 writes them as ISO 8601
/// text; KDBX 4 as Base64 of little-endian seconds since 0001-01-01 UTC.
const TIMESTAMPS: &[(&str, &str)] = &[
    ("Times", "CreationTime"),
    ("Times", "LastModificationTime"),
    ("Times", "LastAccessTime"),
    ("Times", "ExpiryTime"),
    ("Times", "LocationChanged"),
    ("Meta", "DatabaseNameChanged"),
    ("Meta", "DatabaseDescriptionChanged"),
    ("Meta", "DefaultUserNameChanged"),
    ("Meta", "MasterKeyChanged"),
    ("Meta", "RecycleBinChanged"),
    ("Meta", "EntryTemplatesGroupChanged"),
    ("Meta", "SettingsChanged"),
    ("DeletedObject", "DeletionTime"),
    ("Icon", "LastModificationTime"),
    ("Item", "LastModificationTime"),
];

/// Moves attachments from `Meta/Binaries` into the KDBX 4 binary pool, renumbering references
/// to them; drops `Meta/HeaderHash`, which hashes the KDBX 3.1 header and means nothing in
/// KDBX 4; and rewrites timestamps in the KDBX 4 encoding.
pub(crate) fn upgrade(document: &mut Element) -> Result<Vec<Binary>, OpenError> {
    let mut pool = Vec::new();
    let mut renumbered = HashMap::new();
    if let Some(meta) = document.child_mut("Meta") {
        meta.take_child("HeaderHash");
        if let Some(binaries) = meta.take_child("Binaries") {
            for binary in binaries.children_named("Binary") {
                if let Some(id) = binary.attribute("ID") {
                    renumbered.insert(id.to_owned(), pool.len().to_string());
                }
                pool.push(Binary {
                    protected: binary.is_protected(),
                    data: binary_data(binary)?,
                });
            }
        }
    }
    if let Some(root) = document.child_mut("Root") {
        renumber_references(root, &renumbered);
    }
    convert_timestamps(document);
    Ok(pool)
}

fn binary_data(binary: &Element) -> Result<Secret, OpenError> {
    let corrupt = |reason: String| OpenError::Corrupt(format!("attachment: {reason}"));
    if let Some(Node::Protected(secret)) = binary.children.first() {
        return Ok(secret.clone());
    }
    let data = BASE64
        .decode(binary.text().trim())
        .map_err(|error| corrupt(error.to_string()))?;
    let compressed = binary
        .attribute("Compressed")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    if !compressed {
        return Ok(Secret::new(data));
    }
    let mut plain = Vec::new();
    flate2::read::GzDecoder::new(data.as_slice())
        .read_to_end(&mut plain)
        .map_err(|error| corrupt(error.to_string()))?;
    Ok(Secret::new(plain))
}

/// Entries refer to attachments with `<Binary><Value Ref="ID"/></Binary>`.
fn renumber_references(element: &mut Element, renumbered: &HashMap<String, String>) {
    for child in element.elements_mut() {
        if child.name == "Binary" {
            if let Some(value) = child.child_mut("Value") {
                for (key, reference) in &mut value.attributes {
                    if key == "Ref"
                        && let Some(index) = renumbered.get(reference.as_str())
                    {
                        reference.clone_from(index);
                    }
                }
            }
        } else {
            renumber_references(child, renumbered);
        }
    }
}

fn convert_timestamps(element: &mut Element) {
    let parent = element.name.clone();
    for child in element.elements_mut() {
        if TIMESTAMPS.contains(&(parent.as_str(), child.name.as_str())) {
            if let Some(seconds) = parse_iso8601(child.text().trim()) {
                child.set_text(BASE64.encode(seconds.to_le_bytes()));
            }
        } else {
            convert_timestamps(child);
        }
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fraction][Z|±HH:MM]` into seconds since 0001-01-01 UTC. Times
/// without a zone are taken as UTC, as KeePass does.
fn parse_iso8601(text: &str) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = text.get(range)?;
        digits
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| digits.parse().ok())?
    };
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if separators
        .iter()
        .any(|&(i, ch)| text.as_bytes().get(i) != Some(&ch))
    {
        return None;
    }
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "" | "Z" => 0,
        _ => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let zone = &rest[1..];
            if zone.len() != 5 || zone.as_bytes()[2] != b':' {
                return None;
            }
            let hours: i64 = zone[..2].parse().ok()?;
            let minutes: i64 = zone[3..].parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };

    let days = days_from_civil(year, month, day) - days_from_civil(1, 1, 1);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    const UNIX_EPOCH: i64 = 62_135_596_800;

    #[test]
    fn iso_timestamps_become_seconds_since_year_one() {
        assert_eq!(parse_iso8601("0001-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(UNIX_EPOCH));
        assert_eq!(
            parse_iso8601("2000-03-01T12:30:15Z"),
            Some(UNIX_EPOCH + 951_913_815)
        );
        assert_eq!(parse_iso8601("1970-01-01T00:00:00.123Z"), Some(UNIX_EPOCH));
        assert_eq!(parse_iso8601("1970-01-01T01:00:00+01:00"), Some(UNIX_EPOCH));
        assert_eq!(parse_iso8601("1970-01-01T00:00:00"), Some(UNIX_EPOCH));
        assert_eq!(parse_iso8601("not a time"), None);
        assert_eq!(parse_iso8601("1970-13-01T00:00:00Z"), None);
    }

    #[test]
    fn upgrading_moves_attachments_to_the_pool_and_renumbers_references() {
        let gzip_hello = "H4sIAAAAAAAAA8tIzcnJ5wIAIDA6NgYAAAA=";
        let mut document = xml::parse(&format!(
            r#"<KeePassFile><Meta><HeaderHash>x</HeaderHash><Binaries>
                <Binary ID="7" Compressed="True">{gzip_hello}</Binary>
                <Binary ID="3">cGxhaW4=</Binary>
            </Binaries></Meta><Root><Group><Entry>
                <Binary><Key>a</Key><Value Ref="3"/></Binary>
                <History><Entry><Binary><Key>b</Key><Value Ref="7"/></Binary></Entry></History>
            </Entry></Group></Root></KeePassFile>"#
        ))
        .unwrap();

        let pool = upgrade(&mut document).unwrap();

        let data: Vec<&[u8]> = pool.iter().map(|binary| binary.data.bytes()).collect();
        assert_eq!(data, [&b"hello\n"[..], b"plain"]);
        let meta = document.child("Meta").unwrap();
        assert!(meta.child("Binaries").is_none());
        assert!(meta.child("HeaderHash").is_none());
        let entry = document
            .child("Root")
            .unwrap()
            .child("Group")
            .unwrap()
            .child("Entry")
            .unwrap();
        let reference = |entry: &Element| {
            let value = entry.child("Binary").unwrap().child("Value").unwrap();
            value.attribute("Ref").unwrap().to_owned()
        };
        assert_eq!(reference(entry), "1");
        let historical = entry.child("History").unwrap().child("Entry").unwrap();
        assert_eq!(reference(historical), "0");
    }

    #[test]
    fn upgrading_rewrites_known_timestamps_only() {
        let mut document = xml::parse(
            "<KeePassFile><Meta><SettingsChanged>1970-01-01T00:00:00Z</SettingsChanged>\
             <Future>1970-01-01T00:00:00Z</Future></Meta></KeePassFile>",
        )
        .unwrap();

        upgrade(&mut document).unwrap();

        let meta = document.child("Meta").unwrap();
        let expected = BASE64.encode(UNIX_EPOCH.to_le_bytes());
        assert_eq!(meta.child("SettingsChanged").unwrap().text(), expected);
        assert_eq!(meta.child("Future").unwrap().text(), "1970-01-01T00:00:00Z");
    }
}
