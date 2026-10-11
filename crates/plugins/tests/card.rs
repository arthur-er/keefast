//! The Card Plugin, compiled into keefast, run through the host as the app runs it.

use keefast_plugins::{EntryField, FieldKind, MonthYear, PluginError, PluginHost, Severity};

const MANIFEST: &str = include_str!("../../../plugins/card/plugin.json");
const COMPONENT: &[u8] = include_bytes!("../../../plugins/card/card.wasm");

fn field<'a>(name: &'a str, value: &'a str, protected: bool) -> EntryField<'a> {
    EntryField {
        name,
        value,
        protected,
    }
}

fn card() -> Vec<EntryField<'static>> {
    vec![
        field("Account", "0001", false),
        field("Card number", "4111 1111 1111 1111", true),
        field("Cardholder", "ARTHUR REZENDE", false),
        field("Expiry", "08/31", false),
        field("CVV", "123", true),
        field("PIN", "8271", true),
    ]
}

#[test]
fn an_entry_with_a_card_number_has_the_card_facet_labelled_by_its_number() {
    let host = PluginHost::bundled();
    let found = host.facets(&card());
    assert_eq!(found.len(), 1);
    let card = &found[0];
    assert_eq!(card.plugin.id, "keefast.card");
    assert_eq!(card.facet.title, "Card");
    assert_eq!(card.label.as_deref(), Some("Visa •••• 1111"));

    let rows: Vec<_> = card
        .fields
        .iter()
        .map(|f| (f.owned.key.as_str(), f.index, f.text.as_deref()))
        .collect();
    assert_eq!(
        rows,
        [
            ("number", Some(1), Some("Visa •••• 1111")),
            ("holder", Some(2), None),
            ("expiry", Some(3), None),
            ("cvv", Some(4), None),
            ("pin", Some(5), None),
        ]
    );
    assert_eq!(card.fields[2].owned.kind, FieldKind::MonthYear);
}

#[test]
fn missing_owned_fields_are_found_as_missing() {
    let host = PluginHost::bundled();
    let found = host.facets(&[field("Card number", "5162 9200 1234 5676", true)]);
    assert_eq!(found[0].label.as_deref(), Some("Mastercard •••• 5676"));
    assert!(found[0].fields[1..].iter().all(|f| f.index.is_none()));
}

#[test]
fn an_entry_without_a_card_number_has_no_card() {
    let host = PluginHost::bundled();
    let fields = [
        field("Cardholder", "ARTHUR", false),
        field("CVV", "1", true),
    ];
    assert!(host.facets(&fields).is_empty());
    assert!(host.facets(&[]).is_empty());
}

#[test]
fn without_the_capability_a_plugin_never_gets_protected_values() {
    let mut json: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    json["capabilities"] = serde_json::json!(["copy", "browserFill"]);
    let mut host = PluginHost::new();
    host.load(&json.to_string(), COMPONENT).unwrap();

    let found = host.facets(&card());
    assert_eq!(found[0].label, None, "the number is protected");
    // An unprotected number is the Plugin's to read.
    let found = host.facets(&[field("Card number", "4111 1111 1111 1111", false)]);
    assert_eq!(found[0].label.as_deref(), Some("Visa •••• 1111"));
}

#[test]
fn format_and_validate_run_the_plugins_hooks() {
    let host = PluginHost::bundled();
    let format = |key, value| host.format("keefast.card", "card", key, value).unwrap();
    assert_eq!(format("number", "4111111111111111"), "4111 1111 1111 1111");
    assert_eq!(format("expiry", "0831"), "08/31");
    assert_eq!(format("holder", "Arthur"), "Arthur");

    let today = MonthYear {
        year: 2026,
        month: 10,
    };
    let validate = |key, value| {
        host.validate("keefast.card", "card", key, value, today)
            .unwrap()
    };
    assert!(validate("number", "4111 1111 1111 1111").is_none());
    let problem = validate("number", "4111 1111 1111 1112").unwrap();
    assert_eq!(problem.severity, Severity::Warning);
    // Problems with the expiry are warnings too: the user may know better.
    assert_eq!(
        validate("expiry", "13/31").unwrap().severity,
        Severity::Warning
    );
    assert!(validate("expiry", "12/26").is_none());
    assert_eq!(
        validate("expiry", "11/26").unwrap().message,
        "This card expires soon."
    );
    assert!(
        validate("expiry", "09/26")
            .unwrap()
            .message
            .contains("expired")
    );

    assert!(matches!(
        host.format("keefast.card", "passport", "number", "1"),
        Err(PluginError::NoFacet(_))
    ));
}

#[test]
fn a_plugin_owning_the_same_fields_is_refused() {
    let mut host = PluginHost::bundled();
    assert!(matches!(
        host.load(MANIFEST, COMPONENT),
        Err(PluginError::Taken(_))
    ));
    let mut json: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    json["id"] = "example.cards".into();
    assert!(matches!(
        host.load(&json.to_string(), COMPONENT),
        Err(PluginError::Clash { field, .. }) if field == "Card number"
    ));
}

#[test]
fn the_card_plugin_owns_its_fields() {
    let host = PluginHost::bundled();
    assert_eq!(
        host.owner_of("CVV").map(|m| m.id.as_str()),
        Some("keefast.card")
    );
    assert!(host.owner_of("Password").is_none());
}
