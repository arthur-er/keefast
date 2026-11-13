//! How an Entry is marked to find it in the list: its tags, colour and icon, and the colours
//! chosen for tags.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use uuid::Uuid;

use crate::edit::{EditError, child_or_insert, push, text_element};
use crate::vault::{decode_uuid, encode_uuid};
use crate::xml::Element;
use crate::{Timestamp, Vault};

/// The colours an Entry can be marked with, as KeeWeb offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Violet,
}

impl EntryColor {
    pub const ALL: [EntryColor; 6] = [
        EntryColor::Red,
        EntryColor::Orange,
        EntryColor::Yellow,
        EntryColor::Green,
        EntryColor::Blue,
        EntryColor::Violet,
    ];

    /// The `#RRGGBB` keefast writes as the Entry's background colour, which KeePassXC and
    /// KeePass draw behind the Entry's row.
    pub fn hex(self) -> &'static str {
        match self {
            EntryColor::Red => "#E06666",
            EntryColor::Orange => "#F6B26B",
            EntryColor::Yellow => "#FFD966",
            EntryColor::Green => "#93C47D",
            EntryColor::Blue => "#6FA8DC",
            EntryColor::Violet => "#A58BD8",
        }
    }

    /// The hue each colour stands for, in degrees.
    fn hue(self) -> f32 {
        match self {
            EntryColor::Red => 0.,
            EntryColor::Orange => 30.,
            EntryColor::Yellow => 55.,
            EntryColor::Green => 120.,
            EntryColor::Blue => 215.,
            EntryColor::Violet => 280.,
        }
    }

    /// The colour nearest to `#RRGGBB` (or `#RGB`) as other clients write it, as KeeWeb reads
    /// any colour as one of its own. Greys and unreadable text have none.
    pub(crate) fn nearest(text: &str) -> Option<Self> {
        let [r, g, b] = parse_rgb(text)?.map(|channel| f32::from(channel) / 255.);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let lightness = (max + min) / 2.;
        let chroma = max - min;
        let saturation = match lightness {
            0. | 1. => 0.,
            _ => chroma / (1. - (2. * lightness - 1.).abs()),
        };
        if saturation < 0.2 || chroma < 0.08 {
            return None;
        }
        let hue = if max == r {
            60. * ((g - b) / chroma).rem_euclid(6.)
        } else if max == g {
            60. * ((b - r) / chroma + 2.)
        } else {
            60. * ((r - g) / chroma + 4.)
        };
        let distance = |color: &EntryColor| {
            let apart = (hue - color.hue()).abs();
            apart.min(360. - apart)
        };
        Self::ALL
            .into_iter()
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
    }
}

fn parse_rgb(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }
    let channel = |digits: &str| u8::from_str_radix(digits, 16).ok();
    match hex.len() {
        6 => Some([
            channel(&hex[0..2])?,
            channel(&hex[2..4])?,
            channel(&hex[4..6])?,
        ]),
        3 => {
            let short = |i: usize| channel(&hex[i..=i]).map(|value| value * 17);
            Some([short(0)?, short(1)?, short(2)?])
        }
        _ => None,
    }
}

/// How many standard icons KeePass has, numbered from 0. Clients draw them each their own way.
pub const STANDARD_ICON_COUNT: u32 = 69;

/// The icon KeePass gives new Entries: a key.
const DEFAULT_ICON: u32 = 0;

/// The icon KeePass gives new Groups, and a Group without one shows: a folder.
pub(crate) const FOLDER_ICON: u32 = 48;

/// The largest custom icon keefast imports. KeePass scales icons down to 128 pixels, so an
/// icon this big is far more than any client needs.
pub const MAX_CUSTOM_ICON_BYTES: usize = 256 * 1024;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A custom icon of a Vault, known by its UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CustomIconId(pub(crate) Uuid);

impl CustomIconId {
    /// The UUID in lowercase hex without dashes, as [`crate::EntryId::to_hex`] writes Entries'.
    pub fn to_hex(self) -> String {
        self.0.simple().to_string()
    }

    /// The custom icon named by its UUID in hex, as [`Self::to_hex`] writes it.
    pub fn from_hex(hex: &str) -> Option<Self> {
        Uuid::try_parse(hex.trim()).ok().map(Self)
    }
}

/// An Entry's icon: one of KeePass's standard icons, or a custom icon stored in the Vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryIcon {
    /// From 0 to [`STANDARD_ICON_COUNT`], exclusive.
    Standard(u32),
    Custom(CustomIconId),
}

impl Default for EntryIcon {
    fn default() -> Self {
        EntryIcon::Standard(DEFAULT_ICON)
    }
}

/// An image the Vault keeps in its `Meta`, which Entries can use as their icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomIcon {
    id: CustomIconId,
    data: Vec<u8>,
}

impl CustomIcon {
    pub fn id(&self) -> CustomIconId {
        self.id
    }

    /// The image file, a PNG as KeePass writes them.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

impl EntryIcon {
    /// An Entry's icon: its custom icon if it has one, or its standard icon.
    pub(crate) fn read(element: &Element) -> Self {
        Self::read_or(element, DEFAULT_ICON)
    }

    /// An Entry's or Group's icon: its custom icon if it has one, or its standard icon, or else
    /// `default`.
    pub(crate) fn read_or(element: &Element, default: u32) -> Self {
        let custom = element
            .child("CustomIconUUID")
            .and_then(|uuid| decode_uuid(&uuid.text()))
            .filter(|uuid| !uuid.is_nil());
        if let Some(uuid) = custom {
            return EntryIcon::Custom(CustomIconId(uuid));
        }
        let standard = element
            .child("IconID")
            .and_then(|id| id.text().trim().parse().ok())
            .filter(|id| *id < STANDARD_ICON_COUNT)
            .unwrap_or(default);
        EntryIcon::Standard(standard)
    }

    /// Sets an Entry's icon. A custom icon keeps the standard icon there was, which clients
    /// that can't show the custom one fall back to.
    pub(crate) fn write(self, entry: &mut Element) {
        match self {
            EntryIcon::Standard(id) => {
                entry.take_child("CustomIconUUID");
                child_or_insert(entry, "IconID", &["UUID"]).set_text(id.to_string());
            }
            EntryIcon::Custom(CustomIconId(uuid)) => {
                child_or_insert(entry, "CustomIconUUID", &["IconID", "UUID"])
                    .set_text(encode_uuid(uuid));
            }
        }
    }
}

/// The custom icons in a Vault's `Meta`, in Vault file order. Icons whose data can't be read
/// are left out; they stay in the Vault file.
pub(crate) fn custom_icons(document: &Element) -> Vec<CustomIcon> {
    document
        .child("Meta")
        .and_then(|meta| meta.child("CustomIcons"))
        .into_iter()
        .flat_map(|icons| icons.children_named("Icon"))
        .filter_map(|icon| {
            let uuid = decode_uuid(&icon.child("UUID")?.text())?;
            let data = BASE64.decode(icon.child("Data")?.text().trim()).ok()?;
            Some(CustomIcon {
                id: CustomIconId(uuid),
                data,
            })
        })
        .collect()
}

impl Vault {
    /// The custom icons Entries can use, in the order they were added.
    pub fn custom_icons(&self) -> &[CustomIcon] {
        &self.custom_icons
    }

    /// The Vault's custom icon `id`, if it has it.
    pub fn custom_icon(&self, id: CustomIconId) -> Option<&CustomIcon> {
        self.custom_icons.iter().find(|icon| icon.id == id)
    }

    /// The custom icon the Vault already has for this image, if any.
    pub fn custom_icon_with(&self, png: &[u8]) -> Option<CustomIconId> {
        let icon = self.custom_icons.iter().find(|icon| icon.data == png)?;
        Some(icon.id)
    }

    /// Adds a PNG image to the Vault's custom icons, for Entries to use. An image the Vault
    /// already has is not added again: its icon is returned.
    pub fn add_custom_icon(&mut self, png: &[u8]) -> Result<CustomIconId, EditError> {
        if !png.starts_with(PNG_SIGNATURE) {
            return Err(EditError::IconNotPng);
        }
        if png.len() > MAX_CUSTOM_ICON_BYTES {
            return Err(EditError::IconTooLarge);
        }
        if let Some(icon) = self.custom_icon_with(png) {
            return Ok(icon);
        }
        let uuid = crate::edit::new_uuid();
        let mut icon = Element::new("Icon");
        push(&mut icon, text_element("UUID", encode_uuid(uuid)));
        push(&mut icon, text_element("Data", BASE64.encode(png)));
        let meta = child_or_insert(&mut self.file.document, "Meta", &[]);
        let icons = child_or_insert(meta, "CustomIcons", &["MemoryProtection"]);
        push(icons, icon);
        self.refresh();
        Ok(CustomIconId(uuid))
    }
}

/// The tags in an Entry's `Tags`, or typed by the user: separated by commas, semicolons or
/// colons, as KeePass reads them, with blanks and repeats left out.
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in text.split([',', ';', ':']).map(str::trim) {
        if !tag.is_empty() && !tags.iter().any(|known| known == tag) {
            tags.push(tag.to_owned());
        }
    }
    tags
}

/// Tags as KeePassXC and KeePass write them.
pub(crate) fn join_tags(tags: &[String]) -> String {
    tags.join(";")
}

/// The colour chosen for a tag is in the Vault's custom data under this prefix plus the tag, as
/// the `#RRGGBB` of [`EntryColor::hex`]; empty once the choice is undone. KDBX tags are plain
/// text and no other client colours them, so the key is keefast's own.
const TAG_COLOR_PREFIX: &str = "keefast_TAG_COLOR_";

impl Vault {
    /// The colours chosen for the Vault's tags, by tag. A tag without one is drawn in a colour
    /// of the UI's choosing.
    pub fn tag_colors(&self) -> BTreeMap<String, EntryColor> {
        self.custom_data()
            .filter_map(|(key, value)| {
                let tag = key.strip_prefix(TAG_COLOR_PREFIX)?;
                Some((tag.to_owned(), EntryColor::nearest(&value)?))
            })
            .collect()
    }

    /// Chooses the colour `tag` is drawn in, or with `None` undoes the choice. The choice is
    /// timed, so a Merge keeps the newest, and an undone one is kept as empty so that a Merge
    /// with a Vault file that still has it doesn't bring it back.
    pub fn set_tag_color(&mut self, tag: &str, color: Option<EntryColor>, now: Timestamp) {
        let key = format!("{TAG_COLOR_PREFIX}{tag}");
        let current = self.custom_data().find(|(known, _)| *known == key);
        let value = color.map_or("", EntryColor::hex);
        match current {
            Some((_, known)) if known == value => {}
            None if color.is_none() => {}
            _ => self.set_vault_custom_data(&key, value, now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_any_colour_as_the_nearest_of_its_own() {
        for color in EntryColor::ALL {
            assert_eq!(EntryColor::nearest(color.hex()), Some(color));
        }
        let nearest = |text| EntryColor::nearest(text);
        assert_eq!(nearest("#FF0000"), Some(EntryColor::Red));
        assert_eq!(nearest("#f80"), Some(EntryColor::Orange));
        assert_eq!(nearest("#FFFF00"), Some(EntryColor::Yellow));
        assert_eq!(nearest("#00FF00"), Some(EntryColor::Green));
        assert_eq!(nearest("#0000FF"), Some(EntryColor::Blue));
        assert_eq!(nearest("#FF00FF"), Some(EntryColor::Violet));
        assert_eq!(nearest("#000000"), None);
        assert_eq!(nearest("#808080"), None);
        assert_eq!(nearest("#FFFFFF"), None);
        assert_eq!(nearest(""), None);
        assert_eq!(nearest("red"), None);
        assert_eq!(nearest("#ÿÿÿ"), None);
    }

    #[test]
    fn splits_tags_as_keepass_does() {
        assert_eq!(parse_tags("dev, work"), ["dev", "work"]);
        assert_eq!(parse_tags("a;b:c,,  ; a "), ["a", "b", "c"]);
        assert!(parse_tags(" ; ").is_empty());
        assert_eq!(join_tags(&parse_tags("dev, work")), "dev;work");
    }
}
