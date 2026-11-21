//! Editing an Entry as the app's editor and its quick actions do: a partial change applied to the
//! Entry as it is when the edit lands, so an unrevealed secret is kept and a Merge that came in
//! meanwhile isn't undone. An edit that changes nothing leaves the Vault saved.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{
    Attachment, CompositeKey, CustomField, EntryColor, EntryFields, EntryId, Timestamp, Vault,
};
use keefast_workspace::{
    AttachmentEdit, CustomFieldEdit, CustomFieldValue, EditEntryError, EntryEdit, TagsEdit,
    UnlockSecrets, VaultId, Workspace,
};
use secrecy::{ExposeSecret, SecretString};
use support::{FakeClipboard, FakeClock, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault};

const START: u64 = 1_800_000_000;

struct Harness {
    storage: Arc<FakeStorage>,
    clock: Arc<FakeClock>,
    workspace: Workspace,
    vault: VaultId,
}

fn harness() -> Harness {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let clock = Arc::new(FakeClock::at_unix_seconds(START));
    let mut workspace = Workspace::with_clock(storage.clone(), clock.clone());
    let vault = workspace
        .open_vault(&personal_vault(), &UnlockSecrets::password(PASSWORD))
        .expect("fixture opens");
    Harness {
        storage,
        clock,
        workspace,
        vault,
    }
}

impl Harness {
    /// A new Entry, saved, so the Vault starts with nothing unsaved.
    fn saved_entry(&mut self, fields: &EntryFields) -> EntryId {
        let root = self
            .workspace
            .vault(self.vault)
            .unwrap()
            .vault()
            .root()
            .id();
        let id = self
            .workspace
            .create_entry(self.vault, root, fields)
            .unwrap();
        self.workspace.save_vault(self.vault).unwrap();
        self.clock.advance(Duration::from_secs(60));
        id
    }

    fn unsaved(&self) -> bool {
        self.workspace
            .vault(self.vault)
            .unwrap()
            .has_unsaved_changes()
    }

    fn entry(&self, id: EntryId) -> &keefast_core::Entry {
        self.workspace.entry(self.vault, id).unwrap()
    }

    fn history(&self, id: EntryId) -> usize {
        self.entry(id).history().len()
    }

    fn modified(&self, id: EntryId) -> Option<Timestamp> {
        self.entry(id).last_modified()
    }

    fn edit(&mut self, id: EntryId, edit: &EntryEdit) -> Result<bool, EditEntryError> {
        self.workspace.edit_entry(self.vault, id, None, edit)
    }

    /// Another device edits the Entry, later than anything here, and its sync tool replaces the
    /// Vault file; then the Vault is saved here, which Merges the change in.
    fn edit_elsewhere(&mut self, id: EntryId, edit: impl FnOnce(&mut EntryFields)) {
        let key = CompositeKey::password(PASSWORD);
        let bytes = self.storage.file(&personal_vault()).unwrap();
        let mut theirs = Vault::open(&bytes, &key).unwrap();
        let mut fields = EntryFields::from(theirs.entry(id).unwrap());
        edit(&mut fields);
        let later = Timestamp::from_unix_seconds(self.workspace.now().unix_seconds() + 600);
        theirs.update_entry(id, &fields, later).unwrap();
        self.storage
            .change_elsewhere(&personal_vault(), &theirs.save().unwrap());
        self.workspace.save_vault(self.vault).unwrap();
        assert_eq!(self.entry(id).last_modified(), Some(later), "Merged in");
    }
}

fn bank() -> EntryFields {
    EntryFields {
        title: "Bank".into(),
        password: SecretString::from("made-here"),
        custom_fields: vec![field("PIN", "1111", true), field("CVV", "737", true)],
        ..EntryFields::default()
    }
}

fn field(name: &str, value: &str, protected: bool) -> CustomField {
    CustomField {
        name: name.to_owned(),
        value: SecretString::from(value),
        protected,
    }
}

fn kept(name: &str, original: usize) -> CustomFieldEdit {
    CustomFieldEdit {
        name: name.into(),
        value: CustomFieldValue::Kept(original),
        protected: true,
    }
}

fn retitled(title: &str) -> EntryEdit {
    EntryEdit {
        title: Some(title.into()),
        ..EntryEdit::default()
    }
}

fn shown_fields(entry: &keefast_core::Entry) -> Vec<(String, String)> {
    entry
        .custom_fields()
        .iter()
        .map(|f| (f.name.clone(), f.value.expose_secret().to_owned()))
        .collect()
}

#[test]
fn setting_the_fields_an_entry_already_has_leaves_the_vault_saved() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    let same = EntryFields::from(h.entry(id));
    h.workspace.update_entry(h.vault, id, &same).unwrap();

    assert!(!h.unsaved(), "nothing changed, so nothing is left to save");
    assert_eq!(h.history(id), 0);
}

#[test]
fn an_edit_that_changes_nothing_leaves_the_vault_saved() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    let changed = h
        .edit(
            id,
            &EntryEdit {
                title: Some("Bank".into()),
                custom_fields: Some(vec![kept("PIN", 0), kept("CVV", 1)]),
                tags: Some(TagsEdit::Replace(Vec::new())),
                expires: Some(None),
                ..EntryEdit::default()
            },
        )
        .unwrap();

    assert!(!changed);
    assert!(!h.unsaved());
    assert_eq!(h.history(id), 0);
}

#[test]
fn an_edit_that_changes_the_entry_is_one_to_save() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    assert!(h.edit(id, &retitled("Bank (edited)")).unwrap());

    assert!(h.unsaved());
    assert_eq!(h.entry(id).title(), "Bank (edited)");
    assert_eq!(h.history(id), 1);
    assert_eq!(
        h.workspace
            .vault(h.vault)
            .unwrap()
            .vault()
            .entry(id)
            .unwrap()
            .password()
            .expose_secret(),
        "made-here",
        "what the edit leaves out is kept"
    );
}

#[test]
fn tags_added_after_a_merge_keep_what_the_merge_brought() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    h.edit_elsewhere(id, |fields| {
        fields.password = SecretString::from("changed-elsewhere")
    });

    h.edit(
        id,
        &EntryEdit {
            tags: Some(TagsEdit::Add("finance".into())),
            ..EntryEdit::default()
        },
    )
    .unwrap();

    assert_eq!(h.entry(id).tags(), ["finance"]);
    assert_eq!(h.entry(id).password().expose_secret(), "changed-elsewhere");
}

#[test]
fn removing_an_attachment_after_a_merge_keeps_what_the_merge_brought() {
    let mut h = harness();
    let id = h.saved_entry(&EntryFields {
        attachments: vec![
            Attachment::new("codes.txt", b"1234".to_vec()),
            Attachment::new("key.pem", b"-----".to_vec()),
        ],
        ..bank()
    });
    h.edit_elsewhere(id, |fields| {
        fields.title = "Bank (renamed elsewhere)".into()
    });

    h.edit(
        id,
        &EntryEdit {
            attachments: vec![AttachmentEdit::Remove(0)],
            ..EntryEdit::default()
        },
    )
    .unwrap();

    let entry = h.entry(id);
    assert_eq!(entry.title(), "Bank (renamed elsewhere)");
    let names: Vec<_> = entry
        .attachments()
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(names, ["key.pem"]);
}

#[test]
fn removing_an_attachment_the_entry_has_not_got_changes_nothing() {
    let mut h = harness();
    let id = h.saved_entry(&EntryFields {
        attachments: vec![Attachment::new("codes.txt", b"1234".to_vec())],
        ..bank()
    });

    let error = h
        .edit(
            id,
            &EntryEdit {
                attachments: vec![AttachmentEdit::Remove(1)],
                ..EntryEdit::default()
            },
        )
        .unwrap_err();

    assert_eq!(error, EditEntryError::NoSuchAttachment);
    assert_eq!(h.entry(id).attachments().len(), 1);
    assert!(!h.unsaved());
}

#[test]
fn added_attachments_are_numbered_beside_one_with_the_same_name() {
    let mut h = harness();
    let id = h.saved_entry(&EntryFields {
        attachments: vec![Attachment::new("codes.txt", b"1234".to_vec())],
        ..bank()
    });

    h.edit(
        id,
        &EntryEdit {
            attachments: vec![AttachmentEdit::Add(Attachment::new(
                "codes.txt",
                b"5678".to_vec(),
            ))],
            ..EntryEdit::default()
        },
    )
    .unwrap();

    let names: Vec<_> = h
        .entry(id)
        .attachments()
        .iter()
        .map(|a| a.name.clone())
        .collect();
    assert_eq!(names, ["codes.txt", "codes (2).txt"]);
}

#[test]
fn an_edit_is_not_saved_over_an_entry_merged_since_the_editor_opened() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    // The editor opens.
    let seen = h.modified(id);
    h.edit_elsewhere(id, |fields| {
        fields.password = SecretString::from("changed-elsewhere")
    });

    let error = h
        .workspace
        .edit_entry(h.vault, id, seen, &retitled("Bank (edited)"))
        .unwrap_err();

    assert_eq!(error, EditEntryError::EntryChanged);
    assert_eq!(h.entry(id).title(), "Bank", "nothing was saved");
    assert_eq!(h.entry(id).password().expose_secret(), "changed-elsewhere");
}

#[test]
fn an_edit_is_not_saved_over_an_entry_edited_here_since_the_editor_opened() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    let seen = h.modified(id);
    h.edit(id, &retitled("Bank (from the detail)")).unwrap();

    let error = h
        .workspace
        .edit_entry(h.vault, id, seen, &retitled("Bank (from the editor)"))
        .unwrap_err();

    assert_eq!(error, EditEntryError::EntryChanged);
    assert_eq!(h.entry(id).title(), "Bank (from the detail)");
}

#[test]
fn keeping_ones_edits_over_an_entry_merged_meanwhile_keeps_the_merged_version_in_history() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    let seen = h.modified(id);
    h.edit_elsewhere(id, |fields| {
        fields.title = "Bank (renamed elsewhere)".into()
    });

    // The user keeps their edits: saved over the Entry as it is now.
    let now = h.modified(id);
    assert_ne!(now, seen);
    h.workspace
        .edit_entry(h.vault, id, now, &retitled("Bank (edited)"))
        .unwrap();

    let entry = h.entry(id);
    assert_eq!(entry.title(), "Bank (edited)");
    assert_eq!(
        entry.history().last().unwrap().title(),
        "Bank (renamed elsewhere)",
        "the merged version is in the history"
    );
}

#[test]
fn an_edit_of_an_entry_not_changed_meanwhile_is_saved() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    let seen = h.modified(id);
    assert!(seen.is_some());

    h.workspace
        .edit_entry(h.vault, id, seen, &retitled("Bank (edited)"))
        .unwrap();

    assert_eq!(h.entry(id).title(), "Bank (edited)");
}

#[test]
fn an_edit_without_what_the_editor_saw_is_saved_over_whatever_the_entry_is() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    h.edit_elsewhere(id, |fields| {
        fields.title = "Bank (renamed elsewhere)".into()
    });

    h.workspace
        .edit_entry(h.vault, id, None, &retitled("Bank (edited)"))
        .unwrap();

    assert_eq!(h.entry(id).title(), "Bank (edited)");
}

#[test]
fn a_password_the_editor_never_revealed_is_kept() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    h.edit(
        id,
        &EntryEdit {
            title: Some("Bank".into()),
            username: Some("alice".into()),
            password: None,
            ..EntryEdit::default()
        },
    )
    .unwrap();

    assert_eq!(h.entry(id).username(), "alice");
    assert_eq!(h.entry(id).password().expose_secret(), "made-here");
}

#[test]
fn custom_fields_kept_by_index_come_back_as_they_were_even_reordered() {
    let mut h = harness();
    let id = h.saved_entry(&EntryFields {
        title: "Company Visa".into(),
        custom_fields: vec![
            field("Note", "hi", false),
            field("Card number", "4111 1111 1111 1111", true),
            field("Expiry", "08/31", false),
            field("CVV", "123", true),
        ],
        ..EntryFields::default()
    });

    h.edit(
        id,
        &EntryEdit {
            custom_fields: Some(vec![
                kept("CVV", 3),
                kept(" Card number ", 1),
                CustomFieldEdit {
                    name: "Expiry".into(),
                    value: CustomFieldValue::New(SecretString::from("09/31")),
                    protected: false,
                },
                CustomFieldEdit {
                    name: "Recovery".into(),
                    value: CustomFieldValue::Empty,
                    protected: false,
                },
            ]),
            ..EntryEdit::default()
        },
    )
    .unwrap();

    let entry = h.entry(id);
    assert_eq!(
        shown_fields(entry),
        [
            ("CVV".to_owned(), "123".to_owned()),
            ("Card number".to_owned(), "4111 1111 1111 1111".to_owned()),
            ("Expiry".to_owned(), "09/31".to_owned()),
            ("Recovery".to_owned(), String::new()),
        ],
        "the Note left out is removed, names are trimmed"
    );
}

#[test]
fn a_field_kept_from_an_index_the_entry_has_not_got_is_refused() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    let error = h
        .edit(
            id,
            &EntryEdit {
                custom_fields: Some(vec![kept("PIN", 0), kept("Gone", 2)]),
                ..EntryEdit::default()
            },
        )
        .unwrap_err();

    assert_eq!(error, EditEntryError::NoSuchField);
    assert!(!h.unsaved());
}

#[test]
fn an_expiry_on_the_date_it_already_falls_on_keeps_its_time_of_day() {
    let mut h = harness();
    // 2027-01-31 14:30:15 UTC, as another client might set it.
    let afternoon = Timestamp::from_unix_seconds(1_801_405_815);
    assert_eq!(afternoon.to_date(), "2027-01-31");
    let id = h.saved_entry(&EntryFields {
        expires: Some(afternoon),
        ..bank()
    });
    let expiring = |date: &str| EntryEdit {
        expires: Some(Some(date.into())),
        ..EntryEdit::default()
    };

    assert!(!h.edit(id, &expiring("2027-01-31")).unwrap());
    assert_eq!(h.entry(id).expires(), Some(afternoon));

    h.edit(id, &expiring("2027-02-01")).unwrap();
    assert_eq!(
        h.entry(id).expires(),
        Some(Timestamp::from_unix_seconds(1_801_440_000)),
        "another date starts at midnight UTC"
    );

    h.edit(
        id,
        &EntryEdit {
            expires: Some(None),
            ..EntryEdit::default()
        },
    )
    .unwrap();
    assert_eq!(h.entry(id).expires(), None);
}

#[test]
fn an_expiry_that_is_not_a_date_is_refused() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    for bad in ["31/01/2027", "2027-02-30", "soon"] {
        let error = h
            .edit(
                id,
                &EntryEdit {
                    expires: Some(Some(bad.into())),
                    ..EntryEdit::default()
                },
            )
            .unwrap_err();
        assert_eq!(error, EditEntryError::BadExpiry, "{bad}");
    }
    assert!(!h.unsaved());
}

#[test]
fn added_tags_are_split_as_keepass_splits_them_and_join_the_entrys_own() {
    let mut h = harness();
    let id = h.saved_entry(&EntryFields {
        tags: vec!["dev".into()],
        ..bank()
    });
    let adding = |text: &str| EntryEdit {
        tags: Some(TagsEdit::Add(text.into())),
        ..EntryEdit::default()
    };

    assert!(h.edit(id, &adding("work, dev; ops ")).unwrap());
    assert_eq!(h.entry(id).tags(), ["dev", "work", "ops"]);
    assert_eq!(h.history(id), 1, "the version before keeps the old tags");
    assert_eq!(h.entry(id).password().expose_secret(), "made-here");

    // Tags it has already change nothing, not even its history.
    h.workspace.save_vault(h.vault).unwrap();
    assert!(!h.edit(id, &adding("ops,dev")).unwrap());
    assert_eq!(h.history(id), 1);
    assert!(!h.unsaved());
}

#[test]
fn replaced_tags_are_split_as_keepass_splits_them() {
    let mut h = harness();
    let id = h.saved_entry(&EntryFields {
        tags: vec!["dev".into()],
        ..bank()
    });

    h.edit(
        id,
        &EntryEdit {
            tags: Some(TagsEdit::Replace(vec!["work; ops".into(), "home".into()])),
            ..EntryEdit::default()
        },
    )
    .unwrap();

    assert_eq!(h.entry(id).tags(), ["work", "ops", "home"]);
}

#[test]
fn colour_and_icon_are_set_or_cleared() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    h.edit(
        id,
        &EntryEdit {
            color: Some(Some(EntryColor::Red)),
            icon: Some(keefast_core::EntryIcon::Standard(12)),
            ..EntryEdit::default()
        },
    )
    .unwrap();
    assert_eq!(h.entry(id).color(), Some(EntryColor::Red));
    assert_eq!(h.entry(id).icon(), keefast_core::EntryIcon::Standard(12));

    h.edit(
        id,
        &EntryEdit {
            color: Some(None),
            ..EntryEdit::default()
        },
    )
    .unwrap();
    assert_eq!(h.entry(id).color(), None);
    assert_eq!(h.entry(id).icon(), keefast_core::EntryIcon::Standard(12));
}

#[test]
fn a_new_entrys_fields_come_from_the_edit_alone() {
    let fields = EntryEdit {
        title: Some("New".into()),
        tags: Some(TagsEdit::Replace(vec!["a,b".into()])),
        custom_fields: Some(vec![CustomFieldEdit {
            name: "PIN".into(),
            value: CustomFieldValue::New(SecretString::from("1234")),
            protected: true,
        }]),
        expires: Some(Some("2027-01-31".into())),
        ..EntryEdit::default()
    }
    .apply(None)
    .unwrap();

    assert_eq!(fields.title, "New");
    assert_eq!(fields.password.expose_secret(), "", "no password to keep");
    assert_eq!(fields.tags, ["a", "b"]);
    assert_eq!(fields.custom_fields[0].value.expose_secret(), "1234");
    assert_eq!(
        fields.expires,
        Some(Timestamp::from_unix_seconds(1_801_353_600))
    );

    let kept_from_nothing = EntryEdit {
        custom_fields: Some(vec![kept("PIN", 0)]),
        ..EntryEdit::default()
    };
    assert!(kept_from_nothing.apply(None).is_err());
}

#[test]
fn an_entry_that_is_not_there_is_not_found() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    h.workspace.delete_entry(h.vault, id).unwrap();
    let gone = h
        .workspace
        .vault(h.vault)
        .unwrap()
        .vault()
        .root()
        .all_entries()
        .find(|entry| entry.title() == "Bank");
    // Deleting moves it to the Recycle Bin when the Vault has one; delete that too.
    if let Some(entry) = gone {
        let id = entry.id();
        h.workspace.delete_entry(h.vault, id).unwrap();
    }

    let error = h.edit(id, &retitled("Ghost")).unwrap_err();
    assert_eq!(error, EditEntryError::NotFound);
}

#[test]
fn a_vault_waiting_to_lock_refuses_an_edit() {
    let mut h = harness();
    let id = h.saved_entry(&bank());
    // An unsaved change makes the lock wait for a save, which is left to run.
    h.edit(id, &retitled("Bank (edited)")).unwrap();
    let _saves = h
        .workspace
        .lock_vault(h.vault, &mut FakeClipboard::default());
    assert!(h.workspace.vault(h.vault).unwrap().is_locking());

    let error = h
        .workspace
        .edit_entry(h.vault, id, None, &retitled("Bank (while locking)"))
        .unwrap_err();

    assert_eq!(error, EditEntryError::VaultNotOpen);
    let title = h
        .workspace
        .vault(h.vault)
        .unwrap()
        .vault()
        .entry(id)
        .unwrap()
        .title();
    assert_eq!(title, "Bank (edited)");
}
