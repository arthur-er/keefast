//! Creating a new, empty Vault.

use crate::edit::{FOLDER_ICON, NO_UUID, new_group, new_uuid, push, text_element};
use crate::encryption::new_settings;
use crate::kdbx::VaultFile;
use crate::vault::encode_uuid;
use crate::xml::Element;
use crate::{CompositeKey, Encryption, EncryptionError, Timestamp, Vault};

/// What keefast writes as the program that made a Vault file.
const GENERATOR: &str = "keefast";

impl Vault {
    /// A new, empty Vault, with a top Group named `name` and the Recycle Bin on, to be saved as
    /// KDBX 4 with `key`, the cipher and the key derivation given.
    pub fn create(
        name: &str,
        key: &CompositeKey,
        encryption: &Encryption,
        now: Timestamp,
    ) -> Result<Vault, EncryptionError> {
        encryption.check()?;
        let file = VaultFile {
            settings: new_settings(encryption),
            binaries: Vec::new(),
            document: document(name, now),
        };
        Ok(Vault::from_file(file, key.hash()).expect("a new Vault has a root Group"))
    }
}

/// The XML of a new Vault, with the `Meta` settings KeePassXC writes for one.
fn document(name: &str, now: Timestamp) -> Element {
    let changed = now.encode();
    let no_group = encode_uuid(NO_UUID);
    let mut meta = Element::new("Meta");
    for (setting, value) in [
        ("Generator", GENERATOR),
        ("DatabaseName", name),
        ("DatabaseNameChanged", &changed),
        ("DatabaseDescription", ""),
        ("DatabaseDescriptionChanged", &changed),
        ("DefaultUserName", ""),
        ("DefaultUserNameChanged", &changed),
        ("MaintenanceHistoryDays", "365"),
        ("Color", ""),
        ("MasterKeyChanged", &changed),
        ("MasterKeyChangeRec", "-1"),
        ("MasterKeyChangeForce", "-1"),
    ] {
        push(&mut meta, text_element(setting, value));
    }
    let mut protection = Element::new("MemoryProtection");
    for (setting, value) in [
        ("ProtectTitle", "False"),
        ("ProtectUserName", "False"),
        ("ProtectPassword", "True"),
        ("ProtectURL", "False"),
        ("ProtectNotes", "False"),
    ] {
        push(&mut protection, text_element(setting, value));
    }
    push(&mut meta, protection);
    for (setting, value) in [
        ("RecycleBinEnabled", "True"),
        ("RecycleBinUUID", &no_group),
        ("RecycleBinChanged", &changed),
        ("EntryTemplatesGroup", &no_group),
        ("EntryTemplatesGroupChanged", &changed),
        ("LastSelectedGroup", &no_group),
        ("LastTopVisibleGroup", &no_group),
        ("HistoryMaxItems", "10"),
        ("HistoryMaxSize", "6291456"),
        ("SettingsChanged", &changed),
    ] {
        push(&mut meta, text_element(setting, value));
    }
    push(&mut meta, Element::new("CustomData"));

    let mut root = Element::new("Root");
    push(&mut root, new_group(new_uuid(), name, FOLDER_ICON, now));
    push(&mut root, Element::new("DeletedObjects"));

    let mut document = Element::new("KeePassFile");
    push(&mut document, meta);
    push(&mut document, root);
    document
}
