//! A Plugin's manifest (`plugin.json`): who it is, which API version it is written against, the
//! Plugin capabilities it asks for, and what it contributes. Read strictly: an unknown key, kind
//! or capability is refused rather than ignored, so a Plugin can never be granted something
//! keefast doesn't know it asked for.

use std::collections::HashSet;

use serde::Deserialize;

use crate::PluginError;

/// The host API version this keefast runs Plugins of (`crates/plugins/wit`).
pub const API_VERSION: u32 = 1;

/// The longest id a Plugin or a facet may have.
const MAX_ID: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// Unique among Plugins, reverse-domain style: `keefast.card`.
    pub id: String,
    /// What the user knows it as.
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub contributions: Contributions,
}

/// Something a Plugin may do, granted by the user when installing it (ADR 0009). Its Owned
/// fields are the facets' fields, so they aren't listed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// Its hooks get the values of protected Owned fields, so `derive` can label them.
    ReadProtected,
    /// The UI offers to copy its Owned fields (copied by the host, Rust-to-clipboard).
    Copy,
}

/// What a Plugin adds to keefast. Later API versions add list subtitles, palette commands and
/// browser fill; each is a key here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contributions {
    #[serde(default)]
    pub facets: Vec<Facet>,
}

/// A kind of data an Entry can have, such as a Card: found by its detection rule, kept in its
/// Owned fields, and shown as a section in the Entry detail.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Facet {
    /// Unique within the Plugin.
    pub id: String,
    /// The section's title, in sentence case.
    pub title: String,
    /// One of the icons keefast's kit draws (`credit-card`, …); the UI falls back to a generic
    /// one for a name it doesn't know.
    #[serde(default)]
    pub icon: Option<String>,
    pub detect: Detect,
    /// In the order the section shows them.
    pub fields: Vec<OwnedField>,
}

/// When an Entry has a facet: when it has a custom field of each of these names.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Detect {
    pub fields: Vec<String>,
}

/// A custom field a Plugin declared as its own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedField {
    /// How hooks name it; unique within the facet.
    pub key: String,
    /// The custom field's name in the Vault file, such as `Card number`.
    pub name: String,
    /// What the section calls it.
    pub label: String,
    pub kind: FieldKind,
    /// Whether the value is kept protected, like a password.
    #[serde(default)]
    pub protected: bool,
}

/// What an Owned field holds, which says how keefast's kit shows and edits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Text,
    /// Digits, typed on a number pad and shown in a monospace font.
    Digits,
    /// A month as `MM/YY`, such as a card's expiry: the host says when it has passed or is near,
    /// since Plugins have no clock.
    MonthYear,
}

impl Manifest {
    /// Reads and checks a manifest.
    pub fn parse(json: &str) -> Result<Self, PluginError> {
        let manifest: Self =
            serde_json::from_str(json).map_err(|error| PluginError::Manifest(error.to_string()))?;
        manifest.check()?;
        Ok(manifest)
    }

    pub fn has(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn facet(&self, id: &str) -> Option<&Facet> {
        self.contributions
            .facets
            .iter()
            .find(|facet| facet.id == id)
    }

    /// The names of the custom fields it owns.
    pub fn owned_names(&self) -> impl Iterator<Item = &str> {
        self.contributions
            .facets
            .iter()
            .flat_map(|facet| facet.fields.iter().map(|field| field.name.as_str()))
    }

    fn check(&self) -> Result<(), PluginError> {
        let invalid = |message: String| Err(PluginError::Manifest(message));
        if self.api_version != API_VERSION {
            return Err(PluginError::ApiVersion(self.api_version));
        }
        if !is_id(&self.id, true) {
            return invalid(format!(
                "\"{}\" is not a Plugin id, such as keefast.card",
                self.id
            ));
        }
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return invalid("a Plugin needs a name and a version".into());
        }
        let mut facets = HashSet::new();
        let mut names = HashSet::new();
        for facet in &self.contributions.facets {
            if !is_id(&facet.id, false) || !facets.insert(facet.id.as_str()) {
                return invalid(format!("facet id \"{}\" is invalid or taken", facet.id));
            }
            if facet.fields.is_empty() {
                return invalid(format!("facet \"{}\" has no fields", facet.id));
            }
            let mut keys = HashSet::new();
            for field in &facet.fields {
                if !is_id(&field.key, false) || !keys.insert(field.key.as_str()) {
                    return invalid(format!("field key \"{}\" is invalid or taken", field.key));
                }
                if field.name.trim() != field.name || field.name.is_empty() {
                    return invalid(format!("field name \"{}\" is invalid", field.name));
                }
                if !names.insert(field.name.as_str()) {
                    return invalid(format!("two fields are named \"{}\"", field.name));
                }
            }
            let owned = |name: &String| facet.fields.iter().any(|field| &field.name == name);
            if facet.detect.fields.is_empty() || !facet.detect.fields.iter().all(owned) {
                return invalid(format!(
                    "facet \"{}\" must be detected by fields it owns",
                    facet.id
                ));
            }
        }
        Ok(())
    }
}

/// Lowercase letters, digits and dashes; Plugin ids also have dots, as `keefast.card` does.
fn is_id(text: &str, dots: bool) -> bool {
    !text.is_empty()
        && text.len() <= MAX_ID
        && text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || (dots && c == '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = include_str!("../../../plugins/card/plugin.json");

    fn with(change: impl FnOnce(&mut serde_json::Value)) -> Result<Manifest, PluginError> {
        let mut json: serde_json::Value = serde_json::from_str(CARD).unwrap();
        change(&mut json);
        Manifest::parse(&json.to_string())
    }

    #[test]
    fn the_card_plugins_manifest_reads() {
        let manifest = Manifest::parse(CARD).unwrap();
        assert_eq!(manifest.id, "keefast.card");
        assert!(manifest.has(Capability::ReadProtected));
        let card = manifest.facet("card").unwrap();
        assert_eq!(card.detect.fields, ["Card number"]);
        let names: Vec<_> = manifest.owned_names().collect();
        assert_eq!(names, ["Card number", "Cardholder", "Expiry", "CVV", "PIN"]);
        assert_eq!(card.fields[2].kind, FieldKind::MonthYear);
        assert!(card.fields[0].protected && !card.fields[1].protected);
    }

    #[test]
    fn another_api_version_is_refused() {
        let result = with(|json| json["apiVersion"] = 2.into());
        assert!(matches!(result, Err(PluginError::ApiVersion(2))));
    }

    #[test]
    fn unknown_capabilities_and_keys_are_refused() {
        assert!(with(|json| json["capabilities"] = serde_json::json!(["network"])).is_err());
        assert!(with(|json| json["homepage"] = "https://example.com".into()).is_err());
        let kind = |json: &mut serde_json::Value| {
            json["contributions"]["facets"][0]["fields"][0]["kind"] = "colour".into()
        };
        assert!(with(kind).is_err());
    }

    #[test]
    fn a_facet_is_detected_by_fields_it_owns() {
        let result = with(|json| {
            json["contributions"]["facets"][0]["detect"]["fields"] = serde_json::json!(["Title"])
        });
        assert!(result.is_err());
    }

    #[test]
    fn field_names_and_keys_are_unique() {
        let result = with(|json| {
            json["contributions"]["facets"][0]["fields"][1]["name"] = "Card number".into()
        });
        assert!(result.is_err());
        let result =
            with(|json| json["contributions"]["facets"][0]["fields"][1]["key"] = "number".into());
        assert!(result.is_err());
    }

    #[test]
    fn ids_are_lowercase() {
        assert!(with(|json| json["id"] = "Keefast.Card".into()).is_err());
        assert!(with(|json| json["id"] = "".into()).is_err());
    }
}
