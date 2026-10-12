//! What Plugins add to the Entry list, the command palette and the browser: the Card's subtitle,
//! its copy commands, the shortcuts the host lets a Plugin have, and the forms it fills.

use std::time::Instant;

use keefast_plugins::{
    Autocomplete, BrowserFill, EntryField, FormKind, PluginError, PluginHost, ShortcutRefusal,
};

const MANIFEST: &str = include_str!("../../../plugins/card/plugin.json");
const COMPONENT: &[u8] = include_bytes!("../../../plugins/card/card.wasm");

fn field<'a>(name: &'a str, value: &'a str, protected: bool) -> EntryField<'a> {
    EntryField {
        name,
        value,
        protected,
    }
}

/// The Card Plugin's manifest, changed.
fn card_with(change: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut json: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    change(&mut json);
    json.to_string()
}

/// Another Plugin, with the Card's component under other field names, whose commands declare
/// these shortcuts.
fn passport(shortcuts: &[&str]) -> String {
    card_with(|json| {
        json["id"] = "example.passport".into();
        json["name"] = "Passport".into();
        let facet = &mut json["contributions"]["facets"][0];
        for field in facet["fields"].as_array_mut().unwrap() {
            let name = format!("Passport {}", field["name"].as_str().unwrap());
            field["name"] = name.into();
        }
        facet["detect"]["fields"] = serde_json::json!(["Passport Card number"]);
        let commands = facet["commands"].as_array_mut().unwrap();
        commands.truncate(shortcuts.len());
        for (command, shortcut) in commands.iter_mut().zip(shortcuts) {
            command["shortcut"] = (*shortcut).into();
        }
    })
}

#[test]
fn the_card_has_copy_commands_with_shortcuts() {
    let host = PluginHost::bundled();
    assert!(host.refused_shortcuts().is_empty());
    let found = host.facets(&[field("Card number", "4111 1111 1111 1111", true)]);
    let commands: Vec<_> = found[0]
        .commands
        .iter()
        .map(|c| (c.command.title.as_str(), c.owned.name.as_str(), c.shortcut))
        .collect();
    assert_eq!(
        commands,
        [
            ("Copy card number", "Card number", Some("Ctrl Shift C")),
            ("Copy CVV", "CVV", Some("Ctrl Shift V")),
            ("Copy expiry", "Expiry", Some("Ctrl Shift E")),
        ]
    );
}

#[test]
fn a_shortcut_keefast_has_is_refused_and_the_plugin_still_loads() {
    let mut host = PluginHost::new();
    host.reserve_shortcuts(&["Ctrl Shift C", "Ctrl N"]);
    // Written another way, it is the same keys.
    let manifest = card_with(|json| {
        json["contributions"]["facets"][0]["commands"][2]["shortcut"] = "ctrl+n".into();
    });
    host.load(&manifest, COMPONENT).unwrap();

    let refused: Vec<_> = host
        .refused_shortcuts()
        .iter()
        .map(|r| (r.command.as_str(), r.reason.clone()))
        .collect();
    assert_eq!(
        refused,
        [
            (
                "copy-number",
                ShortcutRefusal::Keefast("Ctrl Shift C".into())
            ),
            ("copy-expiry", ShortcutRefusal::Keefast("Ctrl N".into())),
        ]
    );
    let found = host.facets(&[field("Card number", "1", true)]);
    let shortcuts: Vec<_> = found[0].commands.iter().map(|c| c.shortcut).collect();
    assert_eq!(shortcuts, [None, Some("Ctrl Shift V"), None]);
    assert_eq!(
        host.refused_shortcuts()[0].to_string(),
        "the keefast.card Plugin's command card/copy-number has no shortcut: \
         Ctrl Shift C is one of keefast's own shortcuts"
    );
}

#[test]
fn every_host_keeps_keefasts_own_shortcuts() {
    // As the store and `keefast-plugin pack` make it: nothing reserved by hand.
    let mut host = PluginHost::new();
    let manifest = card_with(|json| {
        json["contributions"]["facets"][0]["commands"][0]["shortcut"] = "Ctrl B".into();
    });
    host.load(&manifest, COMPONENT).unwrap();
    assert_eq!(
        host.refused_shortcuts()[0].reason,
        ShortcutRefusal::Keefast("Ctrl B".into())
    );
}

#[test]
fn the_keys_text_fields_own_are_refused_without_being_reserved() {
    let mut host = PluginHost::new();
    let manifest = card_with(|json| {
        json["contributions"]["facets"][0]["commands"][0]["shortcut"] = "Ctrl V".into();
    });
    host.load(&manifest, COMPONENT).unwrap();
    assert_eq!(
        host.refused_shortcuts()[0].reason,
        ShortcutRefusal::Keefast("Ctrl V".into())
    );
}

#[test]
fn a_shortcut_another_plugin_has_is_refused() {
    let mut host = PluginHost::bundled();
    host.load(&passport(&["Ctrl Shift C", "Ctrl Shift J"]), COMPONENT)
        .unwrap();
    let refused = host.refused_shortcuts();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].plugin, "example.passport");
    assert_eq!(
        refused[0].reason,
        ShortcutRefusal::Taken {
            shortcut: "Ctrl Shift C".into(),
            other: "Card".into(),
        }
    );
    let found = host.facets(&[field("Passport Card number", "1", true)]);
    let shortcuts: Vec<_> = found[0].commands.iter().map(|c| c.shortcut).collect();
    assert_eq!(shortcuts, [None, Some("Ctrl Shift J")]);
    // The Card, loaded first, keeps its keys.
    let found = host.facets(&[field("Card number", "1", true)]);
    assert_eq!(found[0].commands[0].shortcut, Some("Ctrl Shift C"));
}

#[test]
fn unloading_a_plugin_frees_its_shortcuts_for_the_others() {
    let mut host = PluginHost::bundled();
    host.load(&passport(&["Ctrl Shift C"]), COMPONENT).unwrap();
    assert_eq!(host.refused_shortcuts().len(), 1);

    assert!(host.unload("keefast.card"));
    assert!(host.refused_shortcuts().is_empty());
    let found = host.facets(&[field("Passport Card number", "1", true)]);
    assert_eq!(found[0].commands[0].shortcut, Some("Ctrl Shift C"));
    assert!(!host.unload("keefast.card"));
}

#[test]
fn a_plugin_cant_give_two_commands_the_same_keys_or_keys_that_type() {
    let mut host = PluginHost::new();
    let manifest = card_with(|json| {
        let commands = &mut json["contributions"]["facets"][0]["commands"];
        commands[1]["shortcut"] = "Ctrl Shift C".into();
        commands[2]["shortcut"] = "Shift E".into();
    });
    host.load(&manifest, COMPONENT).unwrap();
    let reasons: Vec<_> = host
        .refused_shortcuts()
        .iter()
        .map(|r| r.reason.clone())
        .collect();
    assert_eq!(
        reasons,
        [
            ShortcutRefusal::Taken {
                shortcut: "Ctrl Shift C".into(),
                other: "Card".into(),
            },
            ShortcutRefusal::Invalid("Shift E".into()),
        ]
    );
}

#[test]
fn copy_commands_need_the_copy_capability() {
    let manifest = card_with(|json| json["capabilities"] = serde_json::json!(["readProtected"]));
    let result = PluginHost::new().load(&manifest, COMPONENT).map(|_| ());
    assert!(
        matches!(result, Err(PluginError::Manifest(m)) if m.contains("copy Plugin capability"))
    );
    // Without commands, it needs no such thing.
    let manifest = card_with(|json| {
        json["capabilities"] = serde_json::json!(["readProtected", "browserFill"]);
        json["contributions"]["facets"][0]["commands"] = serde_json::json!([]);
    });
    assert!(PluginHost::new().load(&manifest, COMPONENT).is_ok());
}

#[test]
fn the_card_fills_payment_forms_with_four_of_its_fields() {
    let host = PluginHost::bundled();
    let fills = host.browser_fills();
    let field = |name: &str, token| (name.to_owned(), token);
    assert_eq!(
        fills,
        [BrowserFill {
            plugin: "keefast.card".into(),
            form: FormKind::Payment,
            detect: vec!["Card number".into()],
            fields: vec![
                field("Card number", Autocomplete::CcNumber),
                field("Cardholder", Autocomplete::CcName),
                field("Expiry", Autocomplete::CcExp),
                field("CVV", Autocomplete::CcCsc),
            ],
        }]
    );
}

#[test]
fn a_plugin_without_browser_fill_fills_nothing() {
    let mut host = PluginHost::new();
    let manifest = card_with(|json| {
        json["capabilities"] = serde_json::json!(["readProtected", "copy"]);
        json["contributions"]["facets"][0]
            .as_object_mut()
            .unwrap()
            .remove("fill");
    });
    host.load(&manifest, COMPONENT).unwrap();
    assert_eq!(host.browser_fills(), []);
    // Nor once unloaded.
    let mut host = PluginHost::bundled();
    assert!(host.unload("keefast.card"));
    assert_eq!(host.browser_fills(), []);
}

#[test]
fn the_list_shows_the_cards_label() {
    let host = PluginHost::bundled();
    let card = [
        field("Note", "hi", false),
        field("Card number", "4111 1111 1111 1111", true),
        field("CVV", "123", true),
    ];
    assert_eq!(host.list_label(&card).as_deref(), Some("Visa •••• 1111"));
    assert!(host.lists_by("Card number") && host.lists_by("CVV"));
    assert!(!host.lists_by("Note"));
    // No Card, no label.
    assert_eq!(host.list_label(&[field("CVV", "123", true)]), None);
    assert_eq!(host.list_label(&[]), None);
}

#[test]
fn a_facet_not_declaring_a_subtitle_gives_none() {
    let mut host = PluginHost::new();
    let manifest = card_with(|json| json["contributions"]["facets"][0]["subtitle"] = false.into());
    host.load(&manifest, COMPONENT).unwrap();
    assert!(!host.lists_by("Card number"));
    assert_eq!(
        host.list_label(&[field("Card number", "4111 1111 1111 1111", true)]),
        None
    );
}

/// The cost of the list's subtitles on a Vault of 5000 Entries, each with a Card and no
/// username: the worst case, as only those run `derive`. `cargo test --release -p
/// keefast-plugins -- --ignored --nocapture list_labels` prints it.
#[test]
#[ignore = "a measurement"]
fn list_labels_for_thousands_of_entries() {
    let host = PluginHost::bundled();
    let numbers: Vec<String> = (0..5000)
        .map(|n| format!("4111 1111 1111 {n:04}"))
        .collect();
    let entries: Vec<Vec<EntryField>> = numbers
        .iter()
        .map(|number| {
            vec![
                field("Card number", number, true),
                field("Expiry", "08/31", false),
                field("CVV", "123", true),
            ]
        })
        .collect();
    for _ in 0..2 {
        let start = Instant::now();
        let labelled = entries
            .iter()
            .filter(|fields| host.list_label(fields).is_some())
            .count();
        println!(
            "{labelled} subtitles in {:?} ({:?} each)",
            start.elapsed(),
            start.elapsed() / labelled as u32
        );
    }
}
