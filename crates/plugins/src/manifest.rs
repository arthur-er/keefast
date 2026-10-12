//! A Plugin's manifest (`plugin.json`): who it is, which API version it is written against, the
//! Plugin capabilities it asks for, and what it contributes. Read strictly: an unknown key, kind
//! or capability is refused rather than ignored, so a Plugin can never be granted something
//! keefast doesn't know it asked for.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

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
    /// Who made it, as they call themselves. Only their key is checked: a package signed by a
    /// key keefast hasn't seen shows this name as unverified.
    #[serde(default)]
    pub publisher: Option<String>,
    /// The publisher's ed25519 public key, base64, which signs the Plugin's packages
    /// (docs/plugins/package.md). The bundled Plugins' manifests leave it out: keefast's build
    /// adds the key it signs them with.
    #[serde(default)]
    pub publisher_key: Option<String>,
    pub api_version: u32,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub contributions: Contributions,
}

/// Something a Plugin may do, granted by the user when installing it (ADR 0009). Its Owned
/// fields are the facets' fields, so they aren't listed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// Its hooks get the values of protected Owned fields, so `derive` can label them.
    ReadProtected,
    /// The UI offers to copy its Owned fields (copied by the host, Rust-to-clipboard).
    Copy,
    /// The Companion extension fills its facets' `fill` Owned fields into forms of the kinds
    /// they name, for an Entry the user picks there, as Access decisions allow.
    BrowserFill,
}

/// What a Plugin adds to keefast: facets, each with its sections, Entry list subtitle, command
/// palette commands and browser fill.
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
    /// Its section in the Entry editor, if it has one: without it, its Owned fields are edited
    /// as custom fields.
    #[serde(default)]
    pub editor: Option<Editor>,
    /// Whether the facet's label (from `derive`) shows in the Entry list under the title of an
    /// Entry with no username, such as "Visa •••• 1111".
    #[serde(default)]
    pub subtitle: bool,
    /// Commands in the command palette's Entry section for an Entry with the facet.
    #[serde(default)]
    pub commands: Vec<Command>,
    /// The forms the Companion extension fills from the facet, and with which Owned fields.
    #[serde(default)]
    pub fill: Option<Fill>,
}

/// A facet's browser fill: the kind of form the Companion extension fills from it, and the
/// form field each Owned field goes into, named by its HTML `autocomplete` token. Needs the
/// `browserFill` Plugin capability.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fill {
    pub form: FormKind,
    /// By Owned field key. Owned fields left out are never filled.
    pub fields: BTreeMap<String, Autocomplete>,
}

/// A kind of web form the Companion extension fills from a facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormKind {
    /// A payment form: a card's number, cardholder, expiry and security code.
    Payment,
}

impl FormKind {
    /// As the protocol and the Companion extension name it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Payment => "payment",
        }
    }
}

/// An HTML `autocomplete` token naming a form field the Companion extension can fill. Each
/// belongs to one [`FormKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Autocomplete {
    /// The card number.
    CcNumber,
    /// The cardholder's name, as on the card.
    CcName,
    /// The expiry, month and year in one field. Its Owned field must be a `monthYear`.
    CcExp,
    CcExpMonth,
    CcExpYear,
    /// The security code (CVV). The Companion extension fills it only into a field marked
    /// `cc-csc`, never into one it guessed.
    CcCsc,
}

impl Autocomplete {
    pub fn form(self) -> FormKind {
        match self {
            Self::CcNumber
            | Self::CcName
            | Self::CcExp
            | Self::CcExpMonth
            | Self::CcExpYear
            | Self::CcCsc => FormKind::Payment,
        }
    }

    /// The token, as HTML writes it: `cc-number`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CcNumber => "cc-number",
            Self::CcName => "cc-name",
            Self::CcExp => "cc-exp",
            Self::CcExpMonth => "cc-exp-month",
            Self::CcExpYear => "cc-exp-year",
            Self::CcCsc => "cc-csc",
        }
    }
}

/// A command palette command acting on the selected Entry when it has the facet. v1 has one
/// kind: copying an Owned field, which needs the `copy` Plugin capability.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    /// Unique within the facet.
    pub id: String,
    /// As the command palette lists it, in sentence case: "Copy card number".
    pub title: String,
    /// The key of the Owned field it copies, Rust-to-clipboard as any copy.
    pub copy: String,
    /// The keys that run it, such as "Ctrl Shift C". The host refuses one that keefast or a
    /// Plugin loaded earlier has (`shortcut.rs`); the command stays, without keys.
    #[serde(default)]
    pub shortcut: Option<String>,
}

/// A facet's section in the Entry editor, and the actions that add it to an Entry and take it
/// off again.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Editor {
    /// The action adding the facet, such as "Add card".
    pub add: String,
    /// The action removing it with its Owned fields, such as "Remove card".
    pub remove: String,
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
    /// What its input shows while empty in the Entry editor, such as "MM/YY".
    #[serde(default)]
    pub placeholder: Option<String>,
}

/// What an Owned field holds, which says how keefast's kit shows and edits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Text,
    /// Digits, typed on a number pad and shown in a monospace font.
    Digits,
    /// A month as `MM/YY`, such as a card's expiry: the host says when it has passed or is near,
    /// since Plugins have no clock (`validate` is passed the month, to warn about it).
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

    /// The names of the Owned fields the Companion extension may fill into web forms, in the
    /// manifest's order.
    pub fn filled_names(&self) -> impl Iterator<Item = &str> {
        self.contributions.facets.iter().flat_map(|facet| {
            facet
                .fields
                .iter()
                .filter(|field| {
                    facet
                        .fill
                        .as_ref()
                        .is_some_and(|fill| fill.fields.contains_key(&field.key))
                })
                .map(|field| field.name.as_str())
        })
    }

    /// The kinds of form the Companion extension may fill from it, each once.
    pub fn fill_forms(&self) -> Vec<FormKind> {
        let mut forms: Vec<FormKind> = self
            .contributions
            .facets
            .iter()
            .filter_map(|facet| facet.fill.as_ref().map(|fill| fill.form))
            .collect();
        forms.sort();
        forms.dedup();
        forms
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
        let too_long = |text: &str| text.chars().count() > MAX_ID || text.contains('\n');
        if too_long(&self.name) || too_long(&self.version) {
            return invalid("a Plugin's name and version must be one short line".into());
        }
        if let Some(publisher) = &self.publisher
            && (publisher.trim().is_empty() || too_long(publisher))
        {
            return invalid("a Plugin's publisher must be one short line".into());
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
            if let Some(editor) = &facet.editor
                && (editor.add.trim().is_empty() || editor.remove.trim().is_empty())
            {
                return invalid(format!(
                    "facet \"{}\" needs its editor's actions named",
                    facet.id
                ));
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
            let mut commands = HashSet::new();
            for command in &facet.commands {
                if !is_id(&command.id, false) || !commands.insert(command.id.as_str()) {
                    return invalid(format!("command id \"{}\" is invalid or taken", command.id));
                }
                if command.title.trim().is_empty() {
                    return invalid(format!("command \"{}\" needs a title", command.id));
                }
                if !keys.contains(command.copy.as_str()) {
                    return invalid(format!(
                        "command \"{}\" copies \"{}\", which facet \"{}\" hasn't",
                        command.id, command.copy, facet.id
                    ));
                }
                if !self.has(Capability::Copy) {
                    return invalid(format!(
                        "command \"{}\" copies, which needs the copy Plugin capability",
                        command.id
                    ));
                }
            }
            if let Some(fill) = &facet.fill {
                check_fill(self, facet, fill).or_else(invalid)?;
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

/// Why a facet's browser fill is invalid, if it is: it fills only the facet's own Owned
/// fields, each into a form field of its form's kind that no other fills, and needs the
/// `browserFill` Plugin capability.
fn check_fill(manifest: &Manifest, facet: &Facet, fill: &Fill) -> Result<(), String> {
    if !manifest.has(Capability::BrowserFill) {
        return Err(format!(
            "facet \"{}\" fills forms, which needs the browserFill Plugin capability",
            facet.id
        ));
    }
    if fill.fields.is_empty() {
        return Err(format!("facet \"{}\" fills no fields", facet.id));
    }
    let mut tokens = HashSet::new();
    for (key, token) in &fill.fields {
        let Some(owned) = facet.fields.iter().find(|field| &field.key == key) else {
            return Err(format!(
                "facet \"{}\" fills \"{key}\", which isn't one of its Owned fields",
                facet.id
            ));
        };
        if token.form() != fill.form {
            return Err(format!(
                "{} isn't a field of a {} form",
                token.as_str(),
                fill.form.as_str()
            ));
        }
        if !tokens.insert(*token) {
            return Err(format!(
                "facet \"{}\" fills {} with two fields",
                facet.id,
                token.as_str()
            ));
        }
        if *token == Autocomplete::CcExp && owned.kind != FieldKind::MonthYear {
            return Err(format!(
                "cc-exp is filled from a monthYear field, and \"{key}\" isn't one"
            ));
        }
    }
    Ok(())
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
        let editor = card.editor.as_ref().unwrap();
        assert_eq!(
            (editor.add.as_str(), editor.remove.as_str()),
            ("Add card", "Remove card")
        );
        assert_eq!(card.fields[2].placeholder.as_deref(), Some("MM/YY"));
    }

    #[test]
    fn editor_actions_are_named() {
        let result = with(|json| json["contributions"]["facets"][0]["editor"]["add"] = " ".into());
        assert!(result.is_err());
        let result = with(|json| json["contributions"]["facets"][0]["editor"]["icon"] = "x".into());
        assert!(result.is_err());
        let without = with(|json| {
            json["contributions"]["facets"][0]
                .as_object_mut()
                .unwrap()
                .remove("editor");
        });
        assert_eq!(without.unwrap().facet("card").unwrap().editor, None);
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
    fn the_card_fills_payment_forms_without_its_pin() {
        let manifest = Manifest::parse(CARD).unwrap();
        assert!(manifest.has(Capability::BrowserFill));
        let fill = manifest.facet("card").unwrap().fill.as_ref().unwrap();
        assert_eq!(fill.form, FormKind::Payment);
        assert_eq!(fill.fields["cvv"], Autocomplete::CcCsc);
        assert_eq!(fill.fields["expiry"], Autocomplete::CcExp);
        let filled: Vec<_> = manifest.filled_names().collect();
        assert_eq!(filled, ["Card number", "Cardholder", "Expiry", "CVV"]);
        assert_eq!(manifest.fill_forms(), [FormKind::Payment]);
    }

    #[test]
    fn browser_fill_needs_its_capability() {
        let result =
            with(|json| json["capabilities"] = serde_json::json!(["readProtected", "copy"]));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("browserFill"), "{error}");
    }

    #[test]
    fn only_known_tokens_fill_only_owned_fields() {
        fn fill(json: &mut serde_json::Value) -> &mut serde_json::Value {
            &mut json["contributions"]["facets"][0]["fill"]
        }
        // Not a token keefast fills, and not a payment form's.
        assert!(with(|json| fill(json)["fields"]["pin"] = "cc-pin".into()).is_err());
        assert!(with(|json| fill(json)["fields"]["holder"] = "username".into()).is_err());
        assert!(with(|json| fill(json)["fields"]["holder"] = "current-password".into()).is_err());
        // A field the facet doesn't own, such as the Entry's password.
        assert!(with(|json| fill(json)["fields"]["password"] = "cc-name".into()).is_err());
        // A form kind keefast doesn't know.
        assert!(with(|json| fill(json)["form"] = "login".into()).is_err());
        // Two fields into one form field.
        assert!(with(|json| fill(json)["fields"]["pin"] = "cc-csc".into()).is_err());
        // cc-exp from a field that isn't a month.
        assert!(with(|json| fill(json)["fields"]["holder"] = "cc-exp".into()).is_err());
        assert!(with(|json| fill(json)["fields"] = serde_json::json!({})).is_err());
        assert!(with(|json| fill(json)["autofill"] = true.into()).is_err());
        // Split expiry fields are tokens too.
        let split = with(|json| {
            fill(json)["fields"] =
                serde_json::json!({ "number": "cc-number", "expiry": "cc-exp-month" })
        });
        assert!(split.is_ok());
    }

    #[test]
    fn ids_are_lowercase() {
        assert!(with(|json| json["id"] = "Keefast.Card".into()).is_err());
        assert!(with(|json| json["id"] = "".into()).is_err());
    }
}
