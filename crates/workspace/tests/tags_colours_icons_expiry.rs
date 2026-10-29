//! Marking Entries with tags, colours and icons, and setting when they expire, through the
//! Workspace: the sidebar's tag and colour filters list the marked Entries of every open Vault,
//! its Smart lists a Vault's Entries with TOTP, changed lately or expired, and the Entry list
//! flags expired Entries by the Workspace's clock.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{
    CompositeKey, CustomField, EditError, EntryColor, EntryFields, EntryIcon, EntryId, EntrySearch,
    EntrySort, Timestamp, Vault,
};
use keefast_workspace::{
    EditVaultError, EntryScope, ImportIconError, RECENTLY_MODIFIED, SmartList, UnlockSecrets,
    VaultId, Workspace,
};
use secrecy::SecretString;
use support::{
    FakeClipboard, FakeClock, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault, work_vault,
};

const START: u64 = 1_800_000_000;

/// A 1x1 opaque red PNG.
const RED_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

struct Harness {
    storage: Arc<FakeStorage>,
    clock: Arc<FakeClock>,
    workspace: Workspace,
    personal: VaultId,
    work: VaultId,
}

fn harness() -> Harness {
    let storage = Arc::new(
        FakeStorage::default()
            .with_file(&personal_vault(), KEEPASSXC_KDBX4)
            .with_file(&work_vault(), KEEPASSXC_KDBX4),
    );
    let clock = Arc::new(FakeClock::at_unix_seconds(START));
    let mut workspace = Workspace::with_clock(storage.clone(), clock.clone());
    let key = UnlockSecrets::password(PASSWORD);
    let personal = workspace.open_vault(&personal_vault(), &key).unwrap();
    let work = workspace.open_vault(&work_vault(), &key).unwrap();
    Harness {
        storage,
        clock,
        workspace,
        personal,
        work,
    }
}

impl Harness {
    fn entry(&self, vault: VaultId, title: &str) -> EntryId {
        self.workspace
            .visible_entries(EntryScope::Vault(vault))
            .iter()
            .find(|listed| listed.entry.title() == title)
            .unwrap_or_else(|| panic!("no Entry {title}"))
            .entry
            .id()
    }

    /// Changes an Entry's fields with `change`.
    fn mark(&mut self, vault: VaultId, title: &str, change: impl FnOnce(&mut EntryFields)) {
        let id = self.entry(vault, title);
        let mut fields = EntryFields::from(self.workspace.entry(vault, id).unwrap());
        change(&mut fields);
        self.workspace.update_entry(vault, id, &fields).unwrap();
    }

    fn listed(&self, scope: EntryScope) -> Vec<(VaultId, String)> {
        self.workspace
            .visible_entries(scope)
            .iter()
            .map(|listed| (listed.vault, listed.entry.title().to_owned()))
            .collect()
    }

    fn reopen(&self, vault: VaultId) -> Vault {
        let location = self.workspace.vault(vault).unwrap().location().clone();
        let bytes = self.storage.file(&location).unwrap();
        Vault::open(&bytes, &CompositeKey::password(PASSWORD)).unwrap()
    }
}

fn tagged(tags: &[&str]) -> impl FnOnce(&mut EntryFields) {
    let tags = tags.iter().map(|tag| tag.to_string()).collect();
    move |fields| fields.tags = tags
}

#[test]
fn the_sidebar_lists_every_tag_and_each_lists_its_entries_across_vaults() {
    let mut h = harness();
    let (personal, work) = (h.personal, h.work);
    h.mark(personal, "GitHub", tagged(&["dev", "Shared"]));
    h.mark(work, "Prod DB", tagged(&["dev"]));
    h.mark(work, "Email", tagged(&["alerts"]));

    assert_eq!(h.workspace.tags(), ["alerts", "dev", "Shared"]);
    assert_eq!(
        h.listed(EntryScope::Tag("dev".into())),
        [(personal, "GitHub".into()), (work, "Prod DB".into())]
    );
    assert_eq!(
        h.listed(EntryScope::Tag("Shared".into())),
        [(personal, "GitHub".into())]
    );
    // Tags are told apart by case, as other clients do.
    assert!(h.listed(EntryScope::Tag("shared".into())).is_empty());
}

#[test]
fn the_colour_filter_lists_entries_marked_with_it() {
    let mut h = harness();
    let (personal, work) = (h.personal, h.work);
    h.mark(personal, "Email", |fields| {
        fields.color = Some(EntryColor::Red)
    });
    h.mark(work, "Root Entry", |fields| {
        fields.color = Some(EntryColor::Red)
    });
    h.mark(work, "GitHub", |fields| {
        fields.color = Some(EntryColor::Blue)
    });

    assert_eq!(
        h.listed(EntryScope::Color(EntryColor::Red)),
        [(personal, "Email".into()), (work, "Root Entry".into())]
    );
    assert_eq!(
        h.listed(EntryScope::Color(EntryColor::Blue)),
        [(work, "GitHub".into())]
    );
    assert!(h.listed(EntryScope::Color(EntryColor::Green)).is_empty());
}

#[test]
fn deleted_entries_drop_out_of_tags_and_colours() {
    let mut h = harness();
    let personal = h.personal;
    h.mark(personal, "GitHub", |fields| {
        fields.tags = vec!["dev".into()];
        fields.color = Some(EntryColor::Green);
    });
    let github = h.entry(personal, "GitHub");
    h.workspace.delete_entry(personal, github).unwrap();

    // The Entry is in the Recycle Bin, where it keeps its marks.
    let bin = h.workspace.vault(personal).unwrap().vault().recycle_bin();
    assert!(bin.is_some());
    assert_eq!(h.workspace.entry(personal, github).unwrap().tags(), ["dev"]);
    assert!(h.workspace.tags().is_empty());
    assert!(h.listed(EntryScope::Tag("dev".into())).is_empty());
    assert!(h.listed(EntryScope::Color(EntryColor::Green)).is_empty());
}

#[test]
fn tags_and_colours_have_no_vault_to_add_to_unless_one_is_open() {
    let h = harness();
    assert_eq!(
        h.workspace
            .group_for_new_items(EntryScope::Tag("dev".into())),
        None
    );
    assert_eq!(EntryScope::Color(EntryColor::Red).vault(), None);
    assert_eq!(EntryScope::Vault(h.work).vault(), Some(h.work));
}

#[test]
fn a_vault_waiting_to_lock_adds_no_tags_and_takes_no_new_entries() {
    let mut h = harness();
    let (personal, work) = (h.personal, h.work);
    h.mark(personal, "GitHub", tagged(&["private-project"]));
    h.mark(work, "Email", tagged(&["alerts"]));
    assert!(h.workspace.tags().contains(&"private-project"));

    // The personal Vault has unsaved changes, so it waits to lock, hidden.
    let saves = h
        .workspace
        .lock_vault(personal, &mut FakeClipboard::default());
    assert!(saves.is_empty());
    assert!(h.workspace.vault(personal).unwrap().is_locking());

    assert_eq!(h.workspace.tags(), ["alerts"]);
    assert!(
        h.listed(EntryScope::Tag("private-project".into()))
            .is_empty()
    );
    assert_eq!(
        h.workspace.group_for_new_items(EntryScope::Vault(personal)),
        None
    );
    // With the personal Vault hidden, the work Vault is the only one to add to.
    assert_eq!(
        h.workspace
            .group_for_new_items(EntryScope::AllVaults)
            .map(|(vault, _)| vault),
        Some(work)
    );
}

#[test]
fn an_entry_is_flagged_expired_once_its_expiry_time_passes() {
    let mut h = harness();
    let personal = h.personal;
    let in_a_day = Timestamp::from_unix_seconds(START as i64 + 86_400);
    h.mark(personal, "GitHub", |fields| fields.expires = Some(in_a_day));
    let github = h.entry(personal, "GitHub");

    let expired = |h: &Harness| {
        let entry = h.workspace.entry(personal, github).unwrap();
        h.workspace.is_expired(entry)
    };
    assert!(!expired(&h));
    h.clock.advance(Duration::from_secs(86_400));
    assert!(expired(&h));

    // Saved, the expiry is in the Vault file.
    h.workspace.save_vault(personal).unwrap();
    let saved = h.reopen(personal);
    assert_eq!(saved.entry(github).unwrap().expires(), Some(in_a_day));
}

#[test]
fn a_tags_colour_is_chosen_per_vault_and_saved_with_it() {
    let mut h = harness();
    let (personal, work) = (h.personal, h.work);
    h.workspace
        .set_tag_color(personal, "dev", Some(EntryColor::Green))
        .unwrap();
    assert!(h.workspace.vault(personal).unwrap().has_unsaved_changes());
    assert!(!h.workspace.vault(work).unwrap().has_unsaved_changes());
    h.workspace.save_vault(personal).unwrap();
    assert_eq!(
        h.reopen(personal).tag_colors().get("dev"),
        Some(&EntryColor::Green)
    );
    assert!(h.reopen(work).tag_colors().is_empty());

    // Choosing it again changes nothing; undoing it is an edit.
    h.workspace
        .set_tag_color(personal, "dev", Some(EntryColor::Green))
        .unwrap();
    assert!(!h.workspace.vault(personal).unwrap().has_unsaved_changes());
    h.workspace.set_tag_color(personal, "dev", None).unwrap();
    h.workspace.save_vault(personal).unwrap();
    assert!(h.reopen(personal).tag_colors().is_empty());
}

#[test]
fn with_totp_lists_the_vaults_entries_with_totp_set_up() {
    let mut h = harness();
    let (personal, work) = (h.personal, h.work);
    let totp = |fields: &mut EntryFields| {
        fields.custom_fields.push(CustomField {
            name: "otp".into(),
            value: SecretString::from("otpauth://totp/GitHub?secret=JBSWY3DPEHPK3PXP"),
            protected: true,
        })
    };
    h.mark(personal, "GitHub", totp);
    h.mark(personal, "Email", totp);
    h.mark(work, "GitHub", totp);
    let with_totp = EntryScope::Smart(personal, SmartList::WithTotp);
    assert_eq!(
        h.listed(with_totp.clone()),
        [(personal, "Email".into()), (personal, "GitHub".into())]
    );

    // An Entry in the Recycle Bin is on no Smart list.
    let email = h.entry(personal, "Email");
    h.workspace.delete_entry(personal, email).unwrap();
    assert_eq!(h.listed(with_totp), [(personal, "GitHub".into())]);
    assert_eq!(h.workspace.smart_list(work, SmartList::WithTotp).len(), 1);
}

#[test]
fn recently_modified_lists_entries_changed_in_the_last_30_days_newest_first() {
    let mut h = harness();
    let personal = h.personal;
    let recent = EntryScope::Smart(personal, SmartList::RecentlyModified);
    // The fixture's Entries were last changed long before.
    assert!(h.listed(recent.clone()).is_empty());

    h.mark(personal, "Email", tagged(&["mail"]));
    h.clock.advance(Duration::from_secs(60));
    h.mark(personal, "GitHub", tagged(&["dev"]));
    h.clock.advance(Duration::from_secs(60));
    h.mark(h.work, "Email", tagged(&["mail"]));
    let newest_first = [(personal, "GitHub".into()), (personal, "Email".into())];
    assert_eq!(h.listed(recent.clone()), newest_first);
    // Whatever the sort asked for.
    let found: Vec<(VaultId, String)> = h
        .workspace
        .find_entries(recent.clone(), &EntrySearch::default(), EntrySort::Title)
        .iter()
        .map(|listed| (listed.vault, listed.entry.title().to_owned()))
        .collect();
    assert_eq!(found, newest_first);

    // 30 days after Email changed, only GitHub, changed a minute later, is still recent.
    h.clock
        .advance(RECENTLY_MODIFIED - Duration::from_secs(120));
    assert_eq!(h.listed(recent.clone()), newest_first);
    h.clock.advance(Duration::from_secs(1));
    assert_eq!(h.listed(recent), [(personal, "GitHub".into())]);
}

#[test]
fn expired_lists_the_vaults_entries_whose_expiry_time_has_come() {
    let mut h = harness();
    let (personal, work) = (h.personal, h.work);
    let in_a_day = Timestamp::from_unix_seconds(START as i64 + 86_400);
    h.mark(personal, "GitHub", |fields| fields.expires = Some(in_a_day));
    h.mark(work, "GitHub", |fields| fields.expires = Some(in_a_day));
    let expired = EntryScope::Smart(personal, SmartList::Expired);
    assert!(h.listed(expired.clone()).is_empty());

    h.clock.advance(Duration::from_secs(86_400));
    assert_eq!(h.listed(expired), [(personal, "GitHub".into())]);
}

#[test]
fn a_smart_list_is_in_its_vault_and_takes_new_entries_at_its_top() {
    let h = harness();
    let scope = EntryScope::Smart(h.work, SmartList::Expired);
    assert_eq!(scope.vault(), Some(h.work));
    let top = h.workspace.vault(h.work).unwrap().vault().root().id();
    assert_eq!(h.workspace.group_for_new_items(scope), Some((h.work, top)));
}

#[test]
fn an_imported_icon_is_added_once_and_saved_with_the_entry_using_it() {
    let mut h = harness();
    let personal = h.personal;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    std::fs::write(&path, RED_PNG).unwrap();

    let icon = h.workspace.import_custom_icon(personal, &path).unwrap();
    assert!(h.workspace.vault(personal).unwrap().has_unsaved_changes());
    h.workspace.save_vault(personal).unwrap();
    // The same image again is the icon the Vault has, and changes nothing.
    assert_eq!(
        h.workspace.import_custom_icon(personal, &path).unwrap(),
        icon
    );
    assert!(!h.workspace.vault(personal).unwrap().has_unsaved_changes());

    h.mark(personal, "Email", |fields| {
        fields.icon = EntryIcon::Custom(icon)
    });
    h.workspace.save_vault(personal).unwrap();
    let saved = h.reopen(personal);
    let email = h.entry(personal, "Email");
    assert_eq!(saved.entry(email).unwrap().icon(), EntryIcon::Custom(icon));
    assert_eq!(saved.custom_icon(icon).unwrap().data(), RED_PNG);
}

#[test]
fn an_icon_that_is_not_a_png_image_is_refused() {
    let mut h = harness();
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("notes.txt");
    std::fs::write(&text, "not an image").unwrap();
    let missing = dir.path().join("missing.png");

    assert!(matches!(
        h.workspace.import_custom_icon(h.personal, &text),
        Err(ImportIconError::Edit(EditVaultError::Edit(
            EditError::IconNotPng
        )))
    ));
    assert!(matches!(
        h.workspace.import_custom_icon(h.personal, &missing),
        Err(ImportIconError::Unreadable(_))
    ));
    let huge = dir.path().join("huge.png");
    let mut png = RED_PNG.to_vec();
    png.resize(keefast_core::MAX_CUSTOM_ICON_BYTES * 2, 0);
    std::fs::write(&huge, png).unwrap();
    assert!(matches!(
        h.workspace.import_custom_icon(h.personal, &huge),
        Err(ImportIconError::Edit(EditVaultError::Edit(
            EditError::IconTooLarge
        )))
    ));
    assert!(!h.workspace.vault(h.personal).unwrap().has_unsaved_changes());
}
