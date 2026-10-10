//! Installing, updating, disabling and removing Plugins in a store on disk.

mod common;

use common::*;
use keefast_plugins::{Capability, EntryField, InstallError, PluginHost, PluginStore, Publisher};

/// A store in a fresh directory, loaded into a fresh host.
fn open(dir: &std::path::Path) -> (PluginStore, PluginHost) {
    let mut store = PluginStore::open(dir);
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
    fake["id"] = "keefast.card".into();
    assert_eq!(
        store
            .inspect(&host, &package_of(&fake, &key(1)))
            .unwrap_err(),
        InstallError::Reserved
    );
    fake["id"] = "keefast.passport".into();
    assert_eq!(
        store
            .inspect(&host, &package_of(&fake, &key(1)))
            .unwrap_err(),
        InstallError::Reserved
    );
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
    assert_eq!(card.publisher, Publisher::Keefast);
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
