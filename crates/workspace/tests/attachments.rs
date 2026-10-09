//! Attachments between disk and Vault: a file read from disk becomes an Entry's attachment, and
//! an attachment is saved back to disk as the same bytes.

mod support;

use std::fs;
use std::sync::Arc;

use keefast_core::{CompositeKey, EntryFields, Vault};
use keefast_workspace::{UnlockSecrets, Workspace, read_attachment, write_attachment};
use support::{FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault};

/// Bytes that are no valid UTF-8, so nothing on the way may treat them as text.
const DATA: &[u8] = &[0x89, b'P', b'N', b'G', 0, 0xff, 0xfe, b'\n', 7];

#[test]
fn a_file_is_read_as_an_attachment_named_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery key.bin");
    fs::write(&path, DATA).unwrap();

    let attachment = read_attachment(&path).expect("file reads");

    assert_eq!(attachment.name, "recovery key.bin");
    assert_eq!(attachment.data(), DATA);
}

#[test]
fn a_missing_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let error = read_attachment(&dir.path().join("missing.txt")).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn a_directory_is_not_a_file_to_attach() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_attachment(dir.path()).is_err());
}

#[test]
fn an_attachment_is_saved_to_disk_replacing_what_was_there() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.bin");
    fs::write(&source, DATA).unwrap();
    let attachment = read_attachment(&source).unwrap();
    let target = dir.path().join("saved.bin");
    fs::write(&target, b"older and longer contents").unwrap();

    write_attachment(&attachment, &target).expect("attachment saves");

    assert_eq!(fs::read(&target).unwrap(), DATA);
}

#[test]
fn saving_into_a_missing_directory_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let attachment = keefast_core::Attachment::new("a.txt", b"a".to_vec());
    assert!(write_attachment(&attachment, &dir.path().join("no/such/dir/a.txt")).is_err());
}

#[test]
fn a_file_attached_through_the_workspace_comes_back_out_the_same_after_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("codes.txt");
    fs::write(&source, DATA).unwrap();
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let mut workspace = Workspace::new(storage.clone());
    let vault = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .unwrap();
    let root = workspace.vault(vault).unwrap().vault().root().id();

    let mut fields = EntryFields {
        title: "Recovery".into(),
        ..EntryFields::default()
    };
    read_attachment(&source)
        .unwrap()
        .add_to(&mut fields.attachments);
    let entry = workspace.create_entry(vault, root, &fields).unwrap();
    workspace.save_vault(vault).unwrap();

    let bytes = storage.file(&personal_vault()).unwrap();
    let reopened = Vault::open(&bytes, &CompositeKey::password(PASSWORD)).unwrap();
    let attachment = &reopened.entry(entry).unwrap().attachments()[0];
    let target = dir.path().join("out.txt");
    write_attachment(attachment, &target).unwrap();
    assert_eq!(attachment.name, "codes.txt");
    assert_eq!(fs::read(&target).unwrap(), DATA);
}
