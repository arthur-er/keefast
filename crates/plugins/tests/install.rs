//! Installing, updating, disabling and removing Plugins in a store on disk.

mod common;

use common::*;
use keefast_plugins::{
    Capability, EntryField, FormKind, InstallError, PluginHost, PluginStore, Publisher,
    PublisherKey, bundled_by_release_key, bundled_key,
};

/// A store in a fresh directory, loaded into a fresh host.
fn open(dir: &std::path::Path) -> (PluginStore, PluginHost) {
    open_trusting(dir, None)
}

/// [`open`], trusting the keys of these seeds as keefast's release keys.
fn open_trusting(dir: &std::path::Path, release: Option<&[u8]>) -> (PluginStore, PluginHost) {
    let mut store = PluginStore::open(dir);
    if let Some(seeds) = release {
        let keys = seeds.iter().map(|seed| PublisherKey::of(&key(*seed)));
        store = store.with_release_keys(keys.collect());
    }
    let mut host = PluginHost::new();
    store.load(&mut host);
    (store, host)
}

fn install(
    store: &mut PluginStore,
    host: &mut PluginHost,
    package: &[u8],
) -> Result<(), InstallError> {
    let offer = store.inspect(host, package)?;
    store.install(host, package, &offer.digest).map(|_| ())
}

fn iban_entry() -> Vec<EntryField<'static>> {
    vec![EntryField {
        name: "IBAN",
        value: "DE89 3704 0044 0532 0130 00",
        protected: false,
    }]
}

#[test]
fn a_new_publisher_s_key_is_unverified_and_trusted_on_first_use() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    let package = iban(&key(1));

    let offer = store.inspect(&host, &package).unwrap();
    assert!(offer.new_key);
    assert_eq!(offer.installed_version, None);
    assert!(matches!(&offer.publisher, Publisher::Unverified { name, .. } if name == "Acme Ltd"));
    assert_eq!(offer.capabilities, [Capability::Copy]);
    assert_eq!(offer.new_capabilities, [Capability::Copy]);
    assert_eq!(offer.owned_fields, ["IBAN", "BIC"]);
    // Inspecting installs nothing.
    assert!(host.manifest("acme.iban").is_none());
    assert_eq!(
        std::fs::read_dir(dir.path())
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );

    store.install(&mut host, &package, &offer.digest).unwrap();
    assert!(host.manifest("acme.iban").is_some());
    assert_eq!(host.facets(&iban_entry()).len(), 1);
    let listed = store.list(&host);
    let iban = listed.iter().find(|p| p.id == "acme.iban").unwrap();
    assert!(iban.enabled && iban.running && !iban.bundled);
    assert_eq!(iban.icon.as_deref(), Some(ICON));

    // It stays installed, checked again, when keefast starts again.
    let (store, host) = open(dir.path());
    assert!(host.manifest("acme.iban").is_some());
    assert!(
        store
            .list(&host)
            .iter()
            .any(|p| p.id == "acme.iban" && p.problem.is_none())
    );
}

#[test]
fn an_update_signed_by_the_same_key_installs_and_one_by_another_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    install(&mut store, &mut host, &iban(&key(1))).unwrap();

    let same = package_of(
        &manifest(&key(1), "1.1", &["IBAN", "BIC"], &["copy"]),
        &key(1),
    );
    let offer = store.inspect(&host, &same).unwrap();
    assert!(!offer.new_key);
    assert_eq!(offer.installed_version.as_deref(), Some("1.0"));
    assert!(offer.new_capabilities.is_empty() && offer.new_owned_fields.is_empty());
    store.install(&mut host, &same, &offer.digest).unwrap();
    assert_eq!(host.manifest("acme.iban").unwrap().version, "1.1");

    let other = package_of(
        &manifest(&key(2), "2.0", &["IBAN", "BIC"], &["copy"]),
        &key(2),
    );
    assert_eq!(
        store.inspect(&host, &other).unwrap_err(),
        InstallError::KeyChanged {
            name: "IBAN".into()
        }
    );
    assert_eq!(host.manifest("acme.iban").unwrap().version, "1.1");
}

#[test]
fn an_update_asking_for_more_says_what_is_new() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    install(&mut store, &mut host, &iban(&key(1))).unwrap();

    let more = package_of(
        &manifest(
            &key(1),
            "2.0",
            &["IBAN", "BIC", "Account holder"],
            &["copy", "readProtected"],
        ),
        &key(1),
    );
    let offer = store.inspect(&host, &more).unwrap();
    assert_eq!(offer.new_capabilities, [Capability::ReadProtected]);
    assert_eq!(offer.new_owned_fields, ["Account holder"]);
    store.install(&mut host, &more, &offer.digest).unwrap();
    let listed = store.list(&host);
    let iban = listed.iter().find(|p| p.id == "acme.iban").unwrap();
    assert_eq!(
        iban.capabilities,
        [Capability::Copy, Capability::ReadProtected]
    );
}

/// `acme.iban` asking to fill a payment form from these fields of `fields`, by index.
fn filling(version: &str, fields: &[&str], filled: &[(usize, &str)]) -> Vec<u8> {
    let mut manifest = manifest(&key(1), version, fields, &["browserFill"]);
    let filled: serde_json::Map<String, serde_json::Value> = filled
        .iter()
        .map(|(index, token)| (format!("f{index}"), (*token).into()))
        .collect();
    manifest["contributions"]["facets"][0]["fill"] =
        serde_json::json!({ "form": "payment", "fields": filled });
    package_of(&manifest, &key(1))
}

#[test]
fn an_update_filling_more_fields_asks_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    let fields = ["Gift card", "Gift name", "Gift code"];
    let first = filling("1.0", &fields, &[(0, "cc-number"), (1, "cc-name")]);
    let offer = store.inspect(&host, &first).unwrap();
    assert_eq!(offer.new_capabilities, [Capability::BrowserFill]);
    assert_eq!(offer.filled_fields, ["Gift card", "Gift name"]);
    assert_eq!(offer.new_filled_fields, offer.filled_fields);
    assert_eq!(offer.fill_forms, [FormKind::Payment]);
    store.install(&mut host, &first, &offer.digest).unwrap();
    assert_eq!(host.browser_fills().len(), 2, "the Card's and this one's");

    // The same capability, but one more field crossing to the browser: new.
    let more = filling(
        "1.1",
        &fields,
        &[(0, "cc-number"), (1, "cc-name"), (2, "cc-csc")],
    );
    let offer = store.inspect(&host, &more).unwrap();
    assert!(offer.new_capabilities.is_empty() && offer.new_owned_fields.is_empty());
    assert_eq!(offer.new_filled_fields, ["Gift code"]);

    // Put on disk in place of the installed package, it doesn't run.
    std::fs::write(dir.path().join("acme.iban.kfplugin"), &more).unwrap();
    let (store, host) = open(dir.path());
    let listed = store.list(&host);
    let iban = listed.iter().find(|p| p.id == "acme.iban").unwrap();
    assert!(!iban.running);
    assert!(iban.problem.as_deref().unwrap().contains("didn't agree to"));
    assert_eq!(iban.filled_fields, ["Gift card", "Gift name"]);
    assert_eq!(host.browser_fills().len(), 1, "only the Card's");
}

#[test]
fn a_package_changed_since_the_user_saw_it_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    let seen = store.inspect(&host, &iban(&key(1))).unwrap();
    let more = package_of(
        &manifest(&key(1), "1.0", &["IBAN", "BIC"], &["copy", "readProtected"]),
        &key(1),
    );
    assert_eq!(
        store.install(&mut host, &more, &seen.digest).unwrap_err(),
        InstallError::Changed
    );
    assert!(host.manifest("acme.iban").is_none());
}

#[test]
fn a_plugin_owning_a_field_another_owns_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());

    let cards = package_of(&manifest(&key(1), "1.0", &["Card number"], &[]), &key(1));
    assert_eq!(
        store.inspect(&host, &cards).unwrap_err(),
        InstallError::Clash {
            name: "IBAN".into(),
            field: "Card number".into(),
            other: "Card".into()
        }
    );

    // Against an installed one too, even disabled.
    install(&mut store, &mut host, &iban(&key(1))).unwrap();
    store.set_enabled(&mut host, "acme.iban", false).unwrap();
    let mut rival = manifest(&key(2), "1.0", &["BIC"], &[]);
    rival["id"] = "other.bic".into();
    rival["name"] = "BIC codes".into();
    let rival = package_of(&rival, &key(2));
    let error = store.inspect(&host, &rival).unwrap_err();
    assert_eq!(
        error,
        InstallError::Clash {
            name: "BIC codes".into(),
            field: "BIC".into(),
            other: "IBAN".into()
        }
    );
    assert!(error.to_string().contains("\"BIC\""));
}

#[test]
fn only_keefast_names_plugins_keefast() {
    let dir = tempfile::tempdir().unwrap();
    let (store, host) = open(dir.path());
    let mut fake = manifest(&key(1), "9.9", &["Passport"], &[]);
    fake["id"] = "keefast.passport".into();
    assert_eq!(
        store
            .inspect(&host, &package_of(&fake, &key(1)))
            .unwrap_err(),
        InstallError::Reserved
    );
    // Not even with the key the bundled Plugins were signed with in this build: it is trusted for
    // them alone.
    fake["id"] = "keefast.card".into();
    assert_eq!(
        store
            .inspect(&host, &package_of(&fake, &key(1)))
            .unwrap_err(),
        InstallError::BundledId("Card".into())
    );
}

#[test]
fn keefast_s_release_key_signs_keefast_s_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open_trusting(dir.path(), Some(&[9]));
    let mut passport = manifest(&key(9), "1.0", &["Passport number"], &[]);
    passport["id"] = "keefast.passport".into();
    let package = package_of(&passport, &key(9));

    let offer = store.inspect(&host, &package).unwrap();
    assert!(matches!(
        offer.publisher,
        Publisher::Keefast { release: true, .. }
    ));
    assert!(!offer.new_key);
    install(&mut store, &mut host, &package).unwrap();
    let listed = store.list(&host);
    let installed = listed.iter().find(|p| p.id == "keefast.passport").unwrap();
    assert!(installed.running && !installed.bundled);
    assert!(matches!(
        installed.publisher,
        Publisher::Keefast { release: true, .. }
    ));

    // A store that doesn't trust that key, as after keefast dropped it, doesn't load it.
    let (store, host) = open(dir.path());
    assert!(host.manifest("keefast.passport").is_none());
    let problem = store
        .list(&host)
        .into_iter()
        .find(|p| p.id == "keefast.passport")
        .unwrap()
        .problem;
    assert!(problem.unwrap().contains("keefast's key"));
}

#[test]
fn keefast_s_plugins_update_across_a_rotation_of_its_release_key() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open_trusting(dir.path(), Some(&[9]));
    let mut passport = manifest(&key(9), "1.0", &["Passport number"], &[]);
    passport["id"] = "keefast.passport".into();
    install(&mut store, &mut host, &package_of(&passport, &key(9))).unwrap();

    // The new key first, the old one still trusted.
    let (mut store, mut host) = open_trusting(dir.path(), Some(&[10, 9]));
    assert!(host.manifest("keefast.passport").is_some());
    let mut update = manifest(&key(10), "2.0", &["Passport number"], &[]);
    update["id"] = "keefast.passport".into();
    install(&mut store, &mut host, &package_of(&update, &key(10))).unwrap();

    // Another publisher's key still can't take over a third-party Plugin, release keys or not.
    install(&mut store, &mut host, &iban(&key(1))).unwrap();
    assert!(matches!(
        store.inspect(&host, &iban(&key(10))).unwrap_err(),
        InstallError::KeyChanged { .. }
    ));
}

#[test]
fn a_disabled_plugin_finds_nothing_until_enabled_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    install(&mut store, &mut host, &iban(&key(1))).unwrap();

    store.set_enabled(&mut host, "acme.iban", false).unwrap();
    assert!(host.facets(&iban_entry()).is_empty());
    let (mut store, mut host) = open(dir.path());
    assert!(host.facets(&iban_entry()).is_empty());
    assert!(
        store
            .list(&host)
            .iter()
            .any(|p| p.id == "acme.iban" && !p.enabled)
    );

    store.set_enabled(&mut host, "acme.iban", true).unwrap();
    assert_eq!(host.facets(&iban_entry()).len(), 1);
}

#[test]
fn removing_a_plugin_forgets_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    install(&mut store, &mut host, &iban(&key(1))).unwrap();

    store.remove(&mut host, "acme.iban").unwrap();
    assert!(host.manifest("acme.iban").is_none());
    assert!(!dir.path().join("acme.iban.kfplugin").exists());
    assert!(!store.list(&host).iter().any(|p| p.id == "acme.iban"));

    let rekeyed = package_of(&manifest(&key(2), "2.0", &["IBAN"], &[]), &key(2));
    let offer = store.inspect(&host, &rekeyed).unwrap();
    assert!(offer.new_key);
}

#[test]
fn a_package_tampered_with_on_disk_doesnt_load() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    install(&mut store, &mut host, &iban(&key(1))).unwrap();

    let path = dir.path().join("acme.iban.kfplugin");
    let mut entries = unpack(&std::fs::read(&path).unwrap());
    entries
        .iter_mut()
        .find(|(name, _)| name == "plugin.wasm")
        .unwrap()
        .1
        .push(0);
    std::fs::write(&path, repack(&entries)).unwrap();

    let (store, host) = open(dir.path());
    assert!(host.manifest("acme.iban").is_none());
    let listed = store.list(&host);
    let iban = listed.iter().find(|p| p.id == "acme.iban").unwrap();
    assert!(!iban.running);
    assert!(iban.problem.as_deref().unwrap().contains("signature"));
}

#[test]
fn a_package_replaced_on_disk_by_another_key_s_doesnt_load() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    install(&mut store, &mut host, &iban(&key(1))).unwrap();
    std::fs::write(dir.path().join("acme.iban.kfplugin"), iban(&key(2))).unwrap();

    let (store, host) = open(dir.path());
    assert!(host.manifest("acme.iban").is_none());
    let problem = store
        .list(&host)
        .into_iter()
        .find(|p| p.id == "acme.iban")
        .unwrap()
        .problem;
    assert!(problem.unwrap().contains("another key"));
}

#[test]
fn keefast_s_card_can_be_disabled_not_removed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, mut host) = open(dir.path());
    let card = store.list(&host).into_iter().next().unwrap();
    assert_eq!(card.id, "keefast.card");
    assert_eq!(
        card.publisher,
        Publisher::Keefast {
            fingerprint: bundled_key().fingerprint(),
            release: bundled_by_release_key(),
        }
    );
    assert!(card.bundled && card.running);

    assert_eq!(
        store.remove(&mut host, "keefast.card").unwrap_err(),
        InstallError::Bundled("Card".into())
    );
    store.set_enabled(&mut host, "keefast.card", false).unwrap();
    assert!(host.manifest("keefast.card").is_none());
    let (mut store, mut host) = open(dir.path());
    assert!(host.manifest("keefast.card").is_none());
    store.set_enabled(&mut host, "keefast.card", true).unwrap();
    assert!(host.manifest("keefast.card").is_some());
}

#[test]
fn without_a_data_directory_only_keefast_s_plugins_run() {
    let mut store = PluginStore::bundled_only();
    let mut host = PluginHost::new();
    store.load(&mut host);
    assert!(host.manifest("keefast.card").is_some());
    let package = iban(&key(1));
    let offer = store.inspect(&host, &package).unwrap();
    assert_eq!(
        store
            .install(&mut host, &package, &offer.digest)
            .unwrap_err(),
        InstallError::NoStore
    );
}

#[test]
fn a_damaged_state_file_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plugins.json"), "{ not json").unwrap();
    let (mut store, mut host) = open(dir.path());
    assert!(store.error().is_some());
    let package = iban(&key(1));
    let offer = store.inspect(&host, &package).unwrap();
    assert!(matches!(
        store
            .install(&mut host, &package, &offer.digest)
            .unwrap_err(),
        InstallError::Io(_)
    ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("plugins.json")).unwrap(),
        "{ not json"
    );
}

/// An installed IBAN Plugin whose copy command asks for `shortcut`.
fn iban_with_shortcut(shortcut: &str) -> Vec<u8> {
    let key = key(1);
    let mut manifest = manifest(&key, "1.0", &["IBAN", "BIC"], &["copy"]);
    manifest["contributions"]["facets"][0]["commands"] = serde_json::json!([
        { "id": "copy-iban", "title": "Copy IBAN", "copy": "f0", "shortcut": shortcut }
    ]);
    package_of(&manifest, &key)
}

fn shortcut_of(host: &PluginHost, field: &'static str) -> Option<String> {
    let entry = [EntryField {
        name: field,
        value: "1",
        protected: false,
    }];
    let found = host.facets(&entry);
    found[0].commands[0].shortcut.map(str::to_owned)
}

#[test]
fn an_installed_plugin_cant_take_a_shortcut_the_card_has_until_the_card_is_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PluginStore::open(dir.path());
    let mut host = PluginHost::new();
    host.reserve_shortcuts(&["Ctrl K"]);
    store.load(&mut host);

    // keefast's own keys hold for an installed Plugin as for a bundled one.
    install(&mut store, &mut host, &iban_with_shortcut("Ctrl K")).unwrap();
    assert_eq!(shortcut_of(&host, "IBAN"), None);
    assert_eq!(host.refused_shortcuts().len(), 1);

    // The Card's keys are taken: the Plugin installs, its command without keys.
    install(&mut store, &mut host, &iban_with_shortcut("Ctrl Shift C")).unwrap();
    assert_eq!(shortcut_of(&host, "IBAN"), None);
    let refused = &host.refused_shortcuts()[0];
    assert_eq!(refused.plugin, "acme.iban");
    assert_eq!(
        refused.reason.to_string(),
        "Ctrl Shift C is taken by the Card Plugin"
    );
    assert_eq!(
        shortcut_of(&host, "Card number").as_deref(),
        Some("Ctrl Shift C")
    );

    // Disabling the Card frees them.
    store.set_enabled(&mut host, "keefast.card", false).unwrap();
    assert_eq!(shortcut_of(&host, "IBAN").as_deref(), Some("Ctrl Shift C"));
    assert!(host.refused_shortcuts().is_empty());

    // Enabled again, the Card loads after the IBAN Plugin, which keeps them.
    store.set_enabled(&mut host, "keefast.card", true).unwrap();
    assert_eq!(shortcut_of(&host, "Card number"), None);
    assert_eq!(host.refused_shortcuts()[0].plugin, "keefast.card");

    // Removing the IBAN Plugin gives them back to the Card.
    store.remove(&mut host, "acme.iban").unwrap();
    assert_eq!(
        shortcut_of(&host, "Card number").as_deref(),
        Some("Ctrl Shift C")
    );
    assert!(host.refused_shortcuts().is_empty());
}
