//! Tags, colours, icons and expiry: how an Entry is marked and when it expires. They must be
//! read as other clients write them, survive edits, saves and reopens, and read the same in an
//! independent KDBX reader.

use keefast_core::{
    CompositeKey, EditError, Entry, EntryColor, EntryFields, EntryIcon, MAX_CUSTOM_ICON_BYTES,
    STANDARD_ICON_COUNT, Timestamp, Vault, parse_tags,
};
use keepass::{Database, DatabaseKey};

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASS2_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx");
const KEEPASS2_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx3.kdbx");
const KEEWEB_KDBX4: &[u8] = include_bytes!("../../../fixtures/keeweb-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";

/// A 1x1 transparent PNG, as the fixtures use for their custom icons.
const FIXTURE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
/// A different 1x1 PNG: opaque red.
const RED_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    open(&vault.save().expect("Vault saves"))
}

fn independent_xml(vault: &Vault) -> String {
    let bytes = vault.save().unwrap();
    let xml = Database::get_xml(&mut &bytes[..], DatabaseKey::new().with_password(PASSWORD))
        .expect("keepass decrypts");
    String::from_utf8(xml).unwrap()
}

fn now() -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_000)
}

fn later() -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_600)
}

fn entry_titled<'a>(vault: &'a Vault, title: &str) -> &'a Entry {
    vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap_or_else(|| panic!("no Entry {title}"))
}

fn fixture_png() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(FIXTURE_PNG)
        .unwrap()
}

/// The text of the first `<name>` element in `xml` from where `from` is.
fn element_after<'a>(xml: &'a str, from: &str, name: &str) -> Option<&'a str> {
    let rest = &xml[xml.find(from)?..];
    let open = format!("<{name}>");
    let start = rest.find(&open)? + open.len();
    let end = rest[start..].find(&format!("</{name}>"))?;
    Some(&rest[start..start + end])
}

#[test]
fn tags_and_custom_icons_from_other_clients_are_read() {
    for (client, bytes, tags) in [
        ("KeePass2 KDBX 4", KEEPASS2_KDBX4, &["dev"][..]),
        ("KeePass2 KDBX 3.1", KEEPASS2_KDBX3, &["dev"]),
        ("KeeWeb", KEEWEB_KDBX4, &["dev", "work"]),
    ] {
        let vault = open(bytes);
        let github = entry_titled(&vault, "GitHub");
        assert_eq!(github.tags(), tags, "{client}");
        let EntryIcon::Custom(icon) = github.icon() else {
            panic!("{client}: GitHub has its custom icon");
        };
        let icon = vault.custom_icon(icon).expect("the Vault has the icon");
        assert!(icon.data().starts_with(b"\x89PNG"), "{client}");
        assert_eq!(github.color(), None, "{client}");
        assert_eq!(github.expires(), None, "{client}");
    }
    let vault = open(KEEPASSXC_KDBX4);
    assert_eq!(vault.custom_icons().len(), 1);
    assert_eq!(vault.custom_icons()[0].data(), fixture_png());
}

#[test]
fn entries_without_marks_have_the_key_icon() {
    let vault = open(KEEWEB_KDBX4);
    let email = entry_titled(&vault, "Email");
    assert!(email.tags().is_empty());
    assert_eq!(email.icon(), EntryIcon::Standard(0));
    assert_eq!(email.color(), None);
}

#[test]
fn tags_colour_icon_and_expiry_are_set_and_survive_a_save() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = entry_titled(&vault, "GitHub").id();
    let icon = vault.entry(id).unwrap().icon();
    let fields = EntryFields {
        tags: vec!["work".into(), "2fa; shared".into(), "work".into()],
        color: Some(EntryColor::Green),
        icon: EntryIcon::Standard(19),
        expires: Some(later()),
        ..EntryFields::from(entry_titled(&vault, "GitHub"))
    };
    vault.update_entry(id, &fields, now()).unwrap();

    let vault = reopen(&vault);
    let github = vault.entry(id).unwrap();
    assert_eq!(github.tags(), ["work", "2fa", "shared"]);
    assert_eq!(github.color(), Some(EntryColor::Green));
    assert_eq!(github.icon(), EntryIcon::Standard(19));
    assert_eq!(github.expires(), Some(later()));
    // The version replaced is in the history, as it was.
    let previous = github.history().last().unwrap();
    assert!(previous.tags().is_empty());
    assert_eq!(previous.color(), None);
    assert_eq!(previous.icon(), icon);
    assert_eq!(previous.expires(), None);

    let xml = independent_xml(&vault);
    let tags = xml.matches("<Tags>work;2fa;shared</Tags>").count();
    assert_eq!(tags, 1, "KeePass reads tags separated by semicolons");
    assert!(xml.contains(&format!(
        "<BackgroundColor>{}</BackgroundColor>",
        EntryColor::Green.hex()
    )));
    assert!(xml.contains("<IconID>19</IconID>"));
    assert!(xml.contains("<Expires>True</Expires>"));
}

#[test]
fn a_custom_icon_gives_way_to_a_standard_one() {
    let mut vault = open(KEEPASS2_KDBX4);
    let id = entry_titled(&vault, "GitHub").id();
    let fields = EntryFields {
        icon: EntryIcon::Standard(1),
        ..EntryFields::from(vault.entry(id).unwrap())
    };
    vault.update_entry(id, &fields, now()).unwrap();
    let vault = reopen(&vault);
    assert_eq!(vault.entry(id).unwrap().icon(), EntryIcon::Standard(1));
    // The icon stays in the Vault, where the history still uses it.
    assert_eq!(vault.custom_icons().len(), 1);
}

#[test]
fn colour_and_expiry_are_cleared() {
    let mut vault = open(KEEWEB_KDBX4);
    let id = entry_titled(&vault, "Email").id();
    let marked = EntryFields {
        color: Some(EntryColor::Red),
        expires: Some(later()),
        ..EntryFields::from(vault.entry(id).unwrap())
    };
    vault.update_entry(id, &marked, now()).unwrap();
    let cleared = EntryFields {
        color: None,
        expires: None,
        ..marked
    };
    vault.update_entry(id, &cleared, later()).unwrap();
    let vault = reopen(&vault);
    let email = vault.entry(id).unwrap();
    assert_eq!(email.color(), None);
    assert_eq!(email.expires(), None);
    assert_eq!(email.history().len(), 2);
}

#[test]
fn unchanged_marks_are_left_as_the_other_client_wrote_them() {
    // KeeWeb writes tags separated by a comma and a space; editing the Entry's password
    // must not rewrite them, nor record anything else.
    let mut vault = open(KEEWEB_KDBX4);
    let before = independent_xml(&vault);
    let keeweb_tags = element_after(&before, "<Tags>dev", "Tags")
        .unwrap()
        .to_owned();
    let id = entry_titled(&vault, "GitHub").id();
    let fields = EntryFields {
        title: "GitHub (work)".into(),
        ..EntryFields::from(vault.entry(id).unwrap())
    };
    vault.update_entry(id, &fields, now()).unwrap();
    let after = independent_xml(&vault);
    assert_eq!(
        after
            .matches(&format!("<Tags>{keeweb_tags}</Tags>"))
            .count(),
        before
            .matches(&format!("<Tags>{keeweb_tags}</Tags>"))
            .count()
            + 1,
        "the Entry and its history both keep KeeWeb's tags"
    );
    // Setting the same tags in another order of separators changes nothing at all.
    let same = EntryFields {
        tags: parse_tags(&keeweb_tags.replace(", ", ";")),
        ..EntryFields::from(vault.entry(id).unwrap())
    };
    let history = vault.entry(id).unwrap().history().len();
    vault.update_entry(id, &same, later()).unwrap();
    assert_eq!(vault.entry(id).unwrap().history().len(), history);
}

#[test]
fn a_new_entry_gets_its_marks() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let root = vault.root().id();
    let fields = EntryFields {
        title: "Router".into(),
        tags: vec!["home".into()],
        color: Some(EntryColor::Orange),
        icon: EntryIcon::Standard(3),
        expires: Some(later()),
        ..EntryFields::default()
    };
    let id = vault.create_entry(root, &fields, now()).unwrap();
    let vault = reopen(&vault);
    let router = vault.entry(id).unwrap();
    assert_eq!(router.tags(), ["home"]);
    assert_eq!(router.color(), Some(EntryColor::Orange));
    assert_eq!(router.icon(), EntryIcon::Standard(3));
    assert_eq!(router.expires(), Some(later()));
    assert!(router.history().is_empty());
}

#[test]
fn an_entry_expires_at_its_expiry_time() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = entry_titled(&vault, "GitHub").id();
    assert!(!vault.entry(id).unwrap().is_expired(later()));
    let fields = EntryFields {
        expires: Some(later()),
        ..EntryFields::from(vault.entry(id).unwrap())
    };
    vault.update_entry(id, &fields, now()).unwrap();
    let github = vault.entry(id).unwrap();
    assert!(!github.is_expired(now()));
    assert!(github.is_expired(later()));
}

#[test]
fn an_imported_custom_icon_is_kept_in_the_vault_and_used_by_the_entry() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let icon = vault.add_custom_icon(RED_PNG).unwrap();
    // The same image again is the same icon, and the fixture's is the one it already has.
    assert_eq!(vault.add_custom_icon(RED_PNG), Ok(icon));
    let existing = vault.custom_icons()[0].id();
    assert_eq!(vault.add_custom_icon(&fixture_png()), Ok(existing));
    assert_eq!(vault.custom_icons().len(), 2);

    let id = entry_titled(&vault, "Email").id();
    let fields = EntryFields {
        icon: EntryIcon::Custom(icon),
        ..EntryFields::from(vault.entry(id).unwrap())
    };
    vault.update_entry(id, &fields, now()).unwrap();
    let vault = reopen(&vault);
    assert_eq!(vault.entry(id).unwrap().icon(), EntryIcon::Custom(icon));
    assert_eq!(vault.custom_icon(icon).unwrap().data(), RED_PNG);
    assert_eq!(independent_xml(&vault).matches("<Icon>").count(), 2);
}

#[test]
fn only_icons_the_vault_has_can_be_used() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = entry_titled(&vault, "Email").id();
    let elsewhere = open(KEEWEB_KDBX4).custom_icons()[0].id();
    for icon in [
        EntryIcon::Standard(STANDARD_ICON_COUNT),
        EntryIcon::Custom(elsewhere),
    ] {
        let fields = EntryFields {
            icon,
            ..EntryFields::from(vault.entry(id).unwrap())
        };
        assert_eq!(
            vault.update_entry(id, &fields, now()),
            Err(EditError::IconNotFound)
        );
        let root = vault.root().id();
        assert_eq!(
            vault.create_entry(root, &fields, now()),
            Err(EditError::IconNotFound)
        );
    }
}

#[test]
fn custom_icons_must_be_small_png_images() {
    let mut vault = open(KEEPASSXC_KDBX4);
    assert_eq!(
        vault.add_custom_icon(b"GIF89a not a png"),
        Err(EditError::IconNotPng)
    );
    let mut huge = RED_PNG.to_vec();
    huge.resize(MAX_CUSTOM_ICON_BYTES + 1, 0);
    assert_eq!(vault.add_custom_icon(&huge), Err(EditError::IconTooLarge));
    assert_eq!(vault.custom_icons().len(), 1);
}
