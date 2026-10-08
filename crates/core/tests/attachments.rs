//! Attachments: files kept with an Entry. KDBX 4 stores their contents once in the binary pool,
//! and Entries and their history refer to them there, so one file can belong to several Entries
//! and history versions. Edits must keep every reference pointing at the right contents.

use keefast_core::{
    Attachment, AttachmentPreview, CompositeKey, EditError, Entry, EntryFields, EntryId,
    ImageFormat, Timestamp, Vault,
};
use keepass::{Database, DatabaseKey};

const KEEPASS2_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx");
const KEEPASS2_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx3.kdbx");
const PASSWORD: &str = "fixture-password";
const CODES: &[u8] = b"recovery-code-1\nrecovery-code-2\n";
const PROTECTED_BIN: &[u8] = &[0, 1, 2, 3, 254, 255];
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    open(&vault.save().expect("Vault saves"))
}

fn independent(vault: &Vault) -> Database {
    let bytes = vault.save().unwrap();
    Database::parse(&bytes, DatabaseKey::new().with_password(PASSWORD)).expect("keepass reads")
}

fn now() -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_000)
}

fn entry_titled<'a>(vault: &'a Vault, title: &str) -> &'a Entry {
    vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap_or_else(|| panic!("no Entry titled {title}"))
}

fn id_of(vault: &Vault, title: &str) -> EntryId {
    entry_titled(vault, title).id()
}

/// Each attachment's name and contents.
fn files(entry: &Entry) -> Vec<(String, Vec<u8>)> {
    entry
        .attachments()
        .iter()
        .map(|attachment| (attachment.name.clone(), attachment.data().to_vec()))
        .collect()
}

fn file(name: &str, data: &[u8]) -> (String, Vec<u8>) {
    (name.to_owned(), data.to_vec())
}

/// Sets an Entry's attachments, keeping its other fields.
fn set_attachments(vault: &mut Vault, title: &str, attachments: Vec<Attachment>) {
    let id = id_of(vault, title);
    let mut fields = EntryFields::from(vault.entry(id).unwrap());
    fields.attachments = attachments;
    vault.update_entry(id, &fields, now()).expect("edit");
}

/// An independent reader's view of an Entry's attachments, by name.
fn independently(database: &Database, title: &str, name: &str) -> Option<Vec<u8>> {
    let entry = database
        .iter_all_entries()
        .find(|entry| entry.get_title() == Some(title))?;
    Some(entry.attachment_by_name(name)?.data.get().to_vec())
}

#[test]
fn an_entry_lists_its_attachments_from_the_binary_pool() {
    let vault = open(KEEPASS2_KDBX4);

    assert_eq!(
        files(entry_titled(&vault, "Email")),
        [
            file("protected.bin", PROTECTED_BIN),
            file("recovery-codes.txt", CODES)
        ]
    );
    assert_eq!(
        files(entry_titled(&vault, "GitHub")),
        [file("recovery-codes.txt", CODES)]
    );
    let attachment = &entry_titled(&vault, "Email").attachments()[0];
    assert_eq!(attachment.size(), 6);
}

#[test]
fn kdbx3_attachments_are_listed_too() {
    let vault = open(KEEPASS2_KDBX3);

    assert_eq!(
        files(entry_titled(&vault, "Email")),
        [
            file("protected.bin", PROTECTED_BIN),
            file("recovery-codes.txt", CODES)
        ]
    );
}

#[test]
fn an_added_attachment_survives_a_save_and_reads_in_keepass() {
    let mut vault = open(KEEPASS2_KDBX4);
    let mut attachments = entry_titled(&vault, "GitHub").attachments().to_vec();
    attachments.push(Attachment::new("key.png", PNG.to_vec()));
    set_attachments(&mut vault, "GitHub", attachments);

    let reopened = reopen(&vault);
    assert_eq!(
        files(entry_titled(&reopened, "GitHub")),
        [file("recovery-codes.txt", CODES), file("key.png", PNG)]
    );
    let database = independent(&vault);
    assert_eq!(
        independently(&database, "GitHub", "key.png").as_deref(),
        Some(PNG)
    );
    assert_eq!(
        independently(&database, "GitHub", "recovery-codes.txt").as_deref(),
        Some(CODES)
    );
}

#[test]
fn a_new_entry_can_have_attachments() {
    let mut vault = open(KEEPASS2_KDBX4);
    let group = vault.root().id();
    let fields = EntryFields {
        title: "Keys".into(),
        attachments: vec![Attachment::new("notes.txt", b"hello".to_vec())],
        ..EntryFields::default()
    };
    let id = vault.create_entry(group, &fields, now()).unwrap();

    let reopened = reopen(&vault);
    assert_eq!(
        files(reopened.entry(id).unwrap()),
        [file("notes.txt", b"hello")]
    );
}

#[test]
fn the_previous_attachments_stay_in_history() {
    let mut vault = open(KEEPASS2_KDBX4);
    set_attachments(
        &mut vault,
        "Email",
        vec![Attachment::new("new.txt", b"new".to_vec())],
    );

    let reopened = reopen(&vault);
    let email = entry_titled(&reopened, "Email");
    assert_eq!(files(email), [file("new.txt", b"new")]);
    let previous = email.history().last().expect("the replaced version");
    assert_eq!(
        files(previous),
        [
            file("protected.bin", PROTECTED_BIN),
            file("recovery-codes.txt", CODES)
        ]
    );
}

#[test]
fn removing_a_shared_attachment_from_one_entry_keeps_it_for_the_other() {
    let mut vault = open(KEEPASS2_KDBX4);
    set_attachments(&mut vault, "GitHub", Vec::new());

    let reopened = reopen(&vault);
    assert!(entry_titled(&reopened, "GitHub").attachments().is_empty());
    assert_eq!(
        files(entry_titled(&reopened, "Email")),
        [
            file("protected.bin", PROTECTED_BIN),
            file("recovery-codes.txt", CODES)
        ]
    );
    let database = independent(&vault);
    assert_eq!(
        independently(&database, "Email", "recovery-codes.txt").as_deref(),
        Some(CODES)
    );
    assert_eq!(
        independently(&database, "Email", "protected.bin").as_deref(),
        Some(PROTECTED_BIN)
    );
}

#[test]
fn attachments_still_match_after_one_is_deleted_for_good() {
    // Deleting the Entry with the only reference to protected.bin frees its place in the pool;
    // what the other Entries refer to must still be what they had.
    let mut vault = open(KEEPASS2_KDBX4);
    let codes = Attachment::new("codes-again.txt", CODES.to_vec());
    let extra = Attachment::new("extra.txt", b"extra".to_vec());
    set_attachments(&mut vault, "GitHub", vec![codes, extra]);
    let email = id_of(&vault, "Email");
    vault.delete_entry(email, now()).unwrap();
    let in_bin = vault.entry(email).is_some();
    if in_bin {
        vault.delete_entry(email, now()).unwrap();
    }
    assert!(vault.entry(email).is_none());

    let reopened = reopen(&vault);
    assert_eq!(
        files(entry_titled(&reopened, "GitHub")),
        [file("codes-again.txt", CODES), file("extra.txt", b"extra")]
    );
    let database = independent(&vault);
    assert_eq!(
        independently(&database, "GitHub", "extra.txt").as_deref(),
        Some(&b"extra"[..])
    );
}

#[test]
fn renaming_an_attachment_keeps_its_contents() {
    let mut vault = open(KEEPASS2_KDBX4);
    let mut attachments = entry_titled(&vault, "Email").attachments().to_vec();
    attachments[0].name = "renamed.bin".into();
    set_attachments(&mut vault, "Email", attachments);

    assert_eq!(
        files(entry_titled(&reopen(&vault), "Email")),
        [
            file("renamed.bin", PROTECTED_BIN),
            file("recovery-codes.txt", CODES)
        ]
    );
}

#[test]
fn setting_the_same_attachments_changes_nothing() {
    let mut vault = open(KEEPASS2_KDBX4);
    let before = vault.clone();
    let attachments = entry_titled(&vault, "Email").attachments().to_vec();
    set_attachments(&mut vault, "Email", attachments);

    assert_eq!(vault, before);
}

#[test]
fn attachment_names_must_be_present_and_unique() {
    let mut vault = open(KEEPASS2_KDBX4);
    let id = id_of(&vault, "Email");
    let before = vault.clone();
    let mut fields = EntryFields::from(vault.entry(id).unwrap());

    fields.attachments = vec![Attachment::new(" ", b"x".to_vec())];
    assert_eq!(
        vault.update_entry(id, &fields, now()),
        Err(EditError::AttachmentNameEmpty)
    );
    fields.attachments = vec![
        Attachment::new("a.txt", b"x".to_vec()),
        Attachment::new("a.txt", b"y".to_vec()),
    ];
    assert_eq!(
        vault.update_entry(id, &fields, now()),
        Err(EditError::AttachmentNameTaken)
    );
    assert_eq!(vault, before);
}

#[test]
fn adding_an_attachment_with_a_taken_name_numbers_it() {
    let mut attachments = Vec::new();
    let mut add = |name: &str| Attachment::new(name, b"x".to_vec()).add_to(&mut attachments);
    let first = add("codes.txt");
    let second = add("codes.txt");
    let third = add("codes.txt");
    let plain = add("README");
    let plain_again = add("README");

    assert_eq!([first, second, third, plain, plain_again], [0, 1, 2, 3, 4]);
    let names: Vec<&str> = attachments.iter().map(|a| &*a.name).collect();
    assert_eq!(
        names,
        [
            "codes.txt",
            "codes (2).txt",
            "codes (3).txt",
            "README",
            "README (2)"
        ]
    );
}

#[test]
fn text_and_images_can_be_previewed() {
    let text = Attachment::new("codes.txt", CODES.to_vec());
    assert_eq!(
        text.preview(),
        AttachmentPreview::Text("recovery-code-1\nrecovery-code-2\n")
    );
    let png = Attachment::new("key", PNG.to_vec());
    assert_eq!(png.preview(), AttachmentPreview::Image(ImageFormat::Png));
    let jpeg = Attachment::new("photo.jpg", b"\xff\xd8\xff\xe0rest".to_vec());
    assert_eq!(jpeg.preview(), AttachmentPreview::Image(ImageFormat::Jpeg));
    let svg = Attachment::new(
        "logo.SVG",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
    );
    assert_eq!(svg.preview(), AttachmentPreview::Image(ImageFormat::Svg));
    let binary = Attachment::new("protected.bin", PROTECTED_BIN.to_vec());
    assert_eq!(binary.preview(), AttachmentPreview::None);
    let nul = Attachment::new("nul.txt", b"a\0b".to_vec());
    assert_eq!(nul.preview(), AttachmentPreview::None);
}

#[test]
fn attachment_contents_are_not_printed() {
    let attachment = Attachment::new("secret.txt", b"hunter2".to_vec());
    assert!(!format!("{attachment:?}").contains("hunter2"));
}
