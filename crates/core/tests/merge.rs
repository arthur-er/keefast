//! Merging two versions of a Vault edited on different devices, with KeePass's rules: newest
//! modification wins, the loser goes to history, deletions and moves are honoured, and Merging
//! again changes nothing.

use keefast_core::{
    CompositeKey, DefaultAccessPolicy, Entry, EntryFields, EntryId, Group, GroupId, MergeSummary,
    OpenError, Timestamp, Vault,
};
use keepass::{Database, DatabaseKey};
use secrecy::{ExposeSecret, SecretString};

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASS2_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

/// Two copies of the same Vault, as two devices syncing one Vault file have.
fn two_copies() -> (Vault, Vault) {
    let vault = open(KEEPASSXC_KDBX4);
    (vault.clone(), vault)
}

/// Times well after everything in the fixture, in order.
fn at(minute: i64) -> Timestamp {
    Timestamp::from_unix_seconds(1_900_000_000 + minute * 60)
}

fn entry_id(vault: &Vault, title: &str) -> EntryId {
    vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap_or_else(|| panic!("no Entry {title}"))
        .id()
}

fn group_id(vault: &Vault, name: &str) -> GroupId {
    fn search(group: &Group, name: &str) -> Option<GroupId> {
        if group.name() == name {
            return Some(group.id());
        }
        group.groups().iter().find_map(|child| search(child, name))
    }
    search(vault.root(), name).unwrap_or_else(|| panic!("no Group {name}"))
}

fn has_group(vault: &Vault, id: GroupId) -> bool {
    vault.group(id).is_some()
}

/// The Group holding the Entry.
fn group_of(vault: &Vault, entry: EntryId) -> GroupId {
    fn search(group: &Group, entry: EntryId) -> Option<GroupId> {
        if group.entries().iter().any(|e| e.id() == entry) {
            return Some(group.id());
        }
        group.groups().iter().find_map(|child| search(child, entry))
    }
    search(vault.root(), entry).expect("the Entry is in the Vault")
}

/// The Group holding the Group.
fn parent_of(vault: &Vault, child: GroupId) -> GroupId {
    fn search(group: &Group, child: GroupId) -> Option<GroupId> {
        if group.groups().iter().any(|g| g.id() == child) {
            return Some(group.id());
        }
        group.groups().iter().find_map(|g| search(g, child))
    }
    search(vault.root(), child).expect("the Group is in the Vault")
}

fn set_password(vault: &mut Vault, entry: EntryId, password: &str, now: Timestamp) {
    let mut fields = EntryFields::from(vault.entry(entry).unwrap());
    fields.password = SecretString::from(password);
    vault.update_entry(entry, &fields, now).unwrap();
}

fn password(vault: &Vault, entry: EntryId) -> String {
    vault
        .entry(entry)
        .unwrap()
        .password()
        .expose_secret()
        .to_owned()
}

fn history_passwords(vault: &Vault, entry: EntryId) -> Vec<String> {
    vault
        .entry(entry)
        .unwrap()
        .history()
        .iter()
        .map(|version| version.password().expose_secret().to_owned())
        .collect()
}

/// The passwords of an Entry's earlier versions and its current one, oldest first, as its
/// history holds them once it changes: versions saved in the same second count once, the
/// later kept.
fn versions_once(vault: &Vault, entry: EntryId) -> Vec<String> {
    let entry = vault.entry(entry).unwrap();
    let versions: Vec<&Entry> = entry.history().iter().chain([entry]).collect();
    versions
        .iter()
        .enumerate()
        .filter(|(index, version)| {
            versions[index + 1..]
                .iter()
                .all(|later| later.last_modified() != version.last_modified())
        })
        .map(|(_, version)| version.password().expose_secret().to_owned())
        .collect()
}

/// Deletes for good, emptying the Recycle Bin too if the Vault has one.
fn delete_entry_for_good(vault: &mut Vault, entry: EntryId, now: Timestamp) {
    while vault.entry(entry).is_some() {
        vault.delete_entry(entry, now).unwrap();
    }
}

fn delete_group_for_good(vault: &mut Vault, group: GroupId, now: Timestamp) {
    while vault.group(group).is_some() {
        vault.delete_group(group, now).unwrap();
    }
}

/// Merges each copy into the other, as both devices would, and checks they agree.
fn merge_both_ways(ours: &mut Vault, theirs: &mut Vault) -> MergeSummary {
    let original_ours = ours.clone();
    let summary = ours.merge(theirs);
    theirs.merge(&original_ours);
    assert_same_content(ours, theirs);
    summary
}

/// The same Entries in the same Groups with the same fields and history.
fn assert_same_content(a: &Vault, b: &Vault) {
    fn describe(vault: &Vault) -> Vec<String> {
        let mut lines = Vec::new();
        fn walk(group: &Group, path: &str, lines: &mut Vec<String>) {
            let path = format!("{path}/{}", group.name());
            lines.push(path.clone());
            for entry in group.entries() {
                lines.push(format!("{path}: {}", describe_entry(entry)));
            }
            for child in group.groups() {
                walk(child, &path, lines);
            }
        }
        fn describe_entry(entry: &Entry) -> String {
            let history: Vec<String> = entry
                .history()
                .iter()
                .map(|version| version.password().expose_secret().to_owned())
                .collect();
            format!(
                "{} {} {:?} history {history:?}",
                entry.title(),
                entry.password().expose_secret(),
                entry.last_modified()
            )
        }
        walk(vault.root(), "", &mut lines);
        lines.sort();
        lines
    }
    assert_eq!(describe(a), describe(b));
}

#[test]
fn edits_on_both_sides_keep_the_newest_and_put_the_other_in_history() {
    let (mut ours, mut theirs) = two_copies();
    let github = entry_id(&ours, "GitHub");
    let mut expected = versions_once(&ours, github);
    set_password(&mut ours, github, "ours", at(1));
    set_password(&mut theirs, github, "theirs", at(2));

    let summary = merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(password(&ours, github), "theirs");
    expected.push("ours".into());
    assert_eq!(
        history_passwords(&ours, github),
        expected,
        "the versions both started from are in history once"
    );
    assert_eq!(summary.entries_updated, 1);
    assert!(summary.changed());
}

#[test]
fn versions_saved_in_the_same_second_count_once_as_in_keepassxc() {
    let (mut ours, mut theirs) = two_copies();
    let github = entry_id(&ours, "GitHub");
    // KeePassXC saved this Entry's current version in the same second as its last.
    let expected = versions_once(&ours, github);
    assert!(expected.len() <= history_passwords(&ours, github).len());
    set_password(&mut theirs, github, "theirs", at(1));

    ours.merge(&theirs);

    assert_eq!(history_passwords(&ours, github), expected);
}

#[test]
fn an_older_edit_elsewhere_goes_to_history_and_the_newer_one_stays() {
    let (mut ours, mut theirs) = two_copies();
    let github = entry_id(&ours, "GitHub");
    set_password(&mut theirs, github, "theirs", at(1));
    set_password(&mut ours, github, "ours", at(2));

    let summary = ours.merge(&theirs);

    assert_eq!(password(&ours, github), "ours");
    assert!(history_passwords(&ours, github).contains(&"theirs".to_owned()));
    assert_eq!(summary.entries_updated, 0);
    assert!(summary.changed(), "the history gained a version");
}

#[test]
fn histories_are_united_without_duplicates() {
    let (mut ours, mut theirs) = two_copies();
    let github = entry_id(&ours, "GitHub");
    let mut expected = versions_once(&ours, github);
    set_password(&mut ours, github, "ours-1", at(1));
    set_password(&mut ours, github, "ours-2", at(3));
    set_password(&mut theirs, github, "theirs-1", at(2));
    set_password(&mut theirs, github, "theirs-2", at(4));

    merge_both_ways(&mut ours, &mut theirs);

    expected.extend(["ours-1", "theirs-1", "ours-2"].map(String::from));
    assert_eq!(history_passwords(&ours, github), expected, "oldest first");
    assert_eq!(password(&ours, github), "theirs-2");
}

#[test]
fn entries_and_groups_added_elsewhere_are_added() {
    let (mut ours, theirs) = two_copies();
    let mut theirs = theirs;
    let root = theirs.root().id();
    let group = theirs.create_group(root, "Shared", at(1)).unwrap();
    let entry = theirs
        .create_entry(
            group,
            &EntryFields {
                title: "Wi-Fi".into(),
                password: SecretString::from("hunter2"),
                ..Default::default()
            },
            at(1),
        )
        .unwrap();
    let ours_entry = ours
        .create_entry(
            root,
            &EntryFields {
                title: "Bank".into(),
                ..Default::default()
            },
            at(2),
        )
        .unwrap();

    let summary = merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(group_of(&ours, entry), group);
    assert_eq!(password(&ours, entry), "hunter2");
    assert!(theirs.entry(ours_entry).is_some());
    assert_eq!(summary.entries_added, 1);
    assert_eq!(summary.groups_added, 1);
}

#[test]
fn an_edit_made_after_a_delete_elsewhere_keeps_the_entry() {
    let (mut ours, mut theirs) = two_copies();
    let email = entry_id(&ours, "Email");
    let personal = group_of(&ours, email);
    delete_entry_for_good(&mut ours, email, at(1));
    set_password(&mut theirs, email, "edited", at(2));

    let summary = merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(password(&ours, email), "edited");
    assert_eq!(group_of(&ours, email), personal);
    assert_eq!(summary.entries_deleted, 0);
}

#[test]
fn a_delete_made_after_an_edit_elsewhere_deletes_the_entry() {
    let (mut ours, mut theirs) = two_copies();
    let email = entry_id(&ours, "Email");
    set_password(&mut theirs, email, "edited", at(1));
    delete_entry_for_good(&mut ours, email, at(2));

    let theirs_summary = theirs.merge(&ours);
    merge_both_ways(&mut ours, &mut theirs);

    assert!(ours.entry(email).is_none());
    assert!(theirs.entry(email).is_none());
    assert_eq!(theirs_summary.entries_deleted, 1);
}

#[test]
fn a_delete_elsewhere_of_an_unchanged_entry_is_merged_quietly_into_a_vault_without_it() {
    let (mut ours, mut theirs) = two_copies();
    let email = entry_id(&ours, "Email");
    delete_entry_for_good(&mut ours, email, at(1));
    delete_entry_for_good(&mut theirs, email, at(2));

    let summary = ours.merge(&theirs);

    assert!(ours.entry(email).is_none());
    assert_eq!(summary.entries_added, 0);
    assert_eq!(summary.entries_deleted, 0);
}

#[test]
fn moving_to_the_recycle_bin_elsewhere_moves_the_entry_there() {
    let (mut ours, mut theirs) = two_copies();
    let email = entry_id(&ours, "Email");
    theirs.delete_entry(email, at(1)).unwrap();
    let bin = theirs
        .recycle_bin()
        .expect("the fixture has the Recycle Bin enabled");

    let summary = merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(group_of(&ours, email), bin);
    assert_eq!(ours.recycle_bin(), Some(bin));
    assert_eq!(summary.entries_moved, 1);
}

#[test]
fn moves_on_both_sides_keep_the_latest_move() {
    let (mut ours, mut theirs) = two_copies();
    let email = entry_id(&ours, "Email");
    let work = group_id(&ours, "Work");
    let servers = group_id(&ours, "Servers");
    ours.move_entry(email, work, at(1)).unwrap();
    theirs.move_entry(email, servers, at(2)).unwrap();

    merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(group_of(&ours, email), servers);
}

#[test]
fn group_moves_on_both_sides_keep_the_latest_move() {
    let (mut ours, mut theirs) = two_copies();
    let servers = group_id(&ours, "Servers");
    let personal = group_id(&ours, "Personal");
    let root = ours.root().id();
    theirs.move_group(servers, root, at(1)).unwrap();
    ours.move_group(servers, personal, at(2)).unwrap();

    let summary = merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(parent_of(&ours, servers), personal);
    assert_eq!(summary.groups_moved, 0, "ours moved it last");
}

#[test]
fn a_move_and_an_edit_elsewhere_both_survive() {
    let (mut ours, mut theirs) = two_copies();
    let email = entry_id(&ours, "Email");
    let work = group_id(&ours, "Work");
    ours.move_entry(email, work, at(1)).unwrap();
    set_password(&mut theirs, email, "edited", at(2));

    merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(group_of(&ours, email), work);
    assert_eq!(password(&ours, email), "edited");
}

#[test]
fn a_group_deleted_elsewhere_stays_while_it_holds_an_entry_edited_after() {
    let (mut ours, mut theirs) = two_copies();
    let personal = group_id(&ours, "Personal");
    let email = entry_id(&ours, "Email");
    delete_group_for_good(&mut ours, personal, at(1));
    set_password(&mut theirs, email, "edited", at(2));

    let summary = merge_both_ways(&mut ours, &mut theirs);

    assert!(has_group(&ours, personal));
    assert_eq!(group_of(&ours, email), personal);
    assert_eq!(password(&ours, email), "edited");
    assert_eq!(summary.groups_deleted, 0);
}

#[test]
fn a_group_deleted_elsewhere_is_deleted_with_what_it_holds() {
    let (mut ours, mut theirs) = two_copies();
    let work = group_id(&ours, "Work");
    let servers = group_id(&ours, "Servers");
    let github = entry_id(&ours, "GitHub");
    delete_group_for_good(&mut theirs, work, at(1));

    let summary = ours.merge(&theirs);

    assert!(!has_group(&ours, work));
    assert!(!has_group(&ours, servers));
    assert!(ours.entry(github).is_none());
    assert_eq!(summary.groups_deleted, 2);
    assert_eq!(summary.entries_deleted, 2);
}

#[test]
fn a_renamed_group_takes_the_newest_name() {
    let (mut ours, mut theirs) = two_copies();
    let work = group_id(&ours, "Work");
    ours.rename_group(work, "Job", at(1)).unwrap();
    theirs.rename_group(work, "Office", at(2)).unwrap();

    let summary = merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(ours.group(work).unwrap().name(), "Office");
    assert_eq!(summary.groups_updated, 1);
}

#[test]
fn merging_again_changes_nothing() {
    let (mut ours, mut theirs) = two_copies();
    let github = entry_id(&ours, "GitHub");
    let email = entry_id(&ours, "Email");
    let work = group_id(&ours, "Work");
    set_password(&mut ours, github, "ours", at(1));
    set_password(&mut theirs, github, "theirs", at(2));
    theirs.move_entry(email, work, at(3)).unwrap();
    let servers = group_id(&ours, "Servers");
    delete_group_for_good(&mut ours, servers, at(4));

    merge_both_ways(&mut ours, &mut theirs);
    let merged = ours.clone();
    let again = ours.merge(&theirs);

    assert!(!again.changed(), "{again:?}");
    assert_eq!(again, MergeSummary::default());
    assert!(ours == merged);
    assert!(!theirs.merge(&merged).changed());
}

#[test]
fn merging_an_unchanged_copy_changes_nothing() {
    let (mut ours, theirs) = two_copies();
    let before = ours.clone();

    let summary = ours.merge(&theirs);

    assert!(!summary.changed());
    assert!(ours == before);
}

#[test]
fn browser_pairings_made_on_either_side_are_kept() {
    let (mut ours, mut theirs) = two_copies();
    ours.add_pairing("Laptop", "laptop-key", at(1)).unwrap();
    theirs.add_pairing("Desktop", "desktop-key", at(2)).unwrap();

    merge_both_ways(&mut ours, &mut theirs);

    let names = |vault: &Vault| {
        let mut names: Vec<String> = vault.pairings().into_iter().map(|p| p.name).collect();
        names.sort();
        names
    };
    assert!(names(&ours).contains(&"Laptop".to_owned()));
    assert!(names(&ours).contains(&"Desktop".to_owned()));
    assert_eq!(names(&ours), names(&theirs));
}

#[test]
fn attachments_and_custom_icons_come_along_and_the_merge_reads_in_keepass() {
    // Two unrelated Vaults Merge like versions whose Entries were all added on one side.
    let mut ours = open(KEEPASS2_KDBX4);
    let theirs = open(KEEPASSXC_KDBX4);
    let github = entry_id(&theirs, "GitHub");

    let summary = ours.merge(&theirs);
    let bytes = ours.save().unwrap();

    assert!(summary.entries_added >= 4, "{summary:?}");
    let reopened = open(&bytes);
    assert_eq!(password(&reopened, github), password(&theirs, github));
    let database = Database::open(&mut &bytes[..], DatabaseKey::new().with_password(PASSWORD))
        .expect("keepass reads the merged Vault file");
    let codes = database
        .iter_all_entries()
        .filter_map(|entry| {
            let attachment = entry.attachment_by_name("recovery-codes.txt")?;
            Some(attachment.data.get().to_vec())
        })
        .collect::<Vec<_>>();
    assert!(
        codes.contains(&b"recovery-code-1\nrecovery-code-2\n".to_vec()),
        "the attachment came along"
    );
    assert!(database.iter_all_custom_icons().count() >= 2);
}

#[test]
fn a_version_saved_elsewhere_opens_with_the_vaults_key() {
    let (ours, mut theirs) = two_copies();
    let github = entry_id(&ours, "GitHub");
    set_password(&mut theirs, github, "theirs", at(1));

    let reopened = ours.open_version(&theirs.save().unwrap()).unwrap();

    assert_eq!(password(&reopened, github), "theirs");
}

#[test]
fn a_damaged_version_is_refused() {
    let ours = open(KEEPASSXC_KDBX4);

    assert!(matches!(
        ours.open_version(b"not a vault file"),
        Err(OpenError::Corrupt(_) | OpenError::Unsupported(_))
    ));
}

#[test]
fn the_browser_group_chosen_last_wins_on_both_sides() {
    let (mut ours, mut theirs) = two_copies();
    let work = group_id(&ours, "Work");
    let personal = group_id(&ours, "Personal");
    ours.set_browser_group(work, at(1)).unwrap();
    theirs.set_browser_group(personal, at(2)).unwrap();

    merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(ours.browser_group(), Some(personal));
    assert_eq!(theirs.browser_group(), Some(personal));
}

#[test]
fn the_default_access_policy_set_last_wins_on_both_sides() {
    let (mut ours, mut theirs) = two_copies();
    ours.set_default_access_policy(DefaultAccessPolicy::AlwaysDeny, at(2));
    theirs.set_default_access_policy(DefaultAccessPolicy::AlwaysAllow, at(1));

    merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(
        ours.default_access_policy(),
        DefaultAccessPolicy::AlwaysDeny
    );
    assert_eq!(
        theirs.default_access_policy(),
        DefaultAccessPolicy::AlwaysDeny
    );
}

#[test]
fn a_pairing_revoked_on_one_side_stays_revoked_on_both() {
    let (mut ours, mut theirs) = two_copies();
    ours.add_pairing("Home", "aG9tZQ==", at(1)).unwrap();
    theirs.merge(&ours);
    // KeePassXC's Pairing, written without a time, and one keefast made.
    ours.revoke_pairing("fixture-browser", at(2)).unwrap();
    ours.revoke_pairing("Home", at(2)).unwrap();

    merge_both_ways(&mut ours, &mut theirs);

    assert_eq!(ours.pairings(), []);
    assert_eq!(theirs.pairings(), []);
}

#[test]
fn a_browser_paired_again_after_its_revocation_stays_paired() {
    let (mut ours, mut theirs) = two_copies();
    ours.add_pairing("Home", "aG9tZQ==", at(1)).unwrap();
    ours.revoke_pairing("Home", at(2)).unwrap();
    theirs.merge(&ours);
    theirs.add_pairing("Home", "aG9tZQ==", at(3)).unwrap();

    merge_both_ways(&mut ours, &mut theirs);

    assert!(ours.is_paired_with("Home", "aG9tZQ=="));
    assert!(theirs.is_paired_with("Home", "aG9tZQ=="));
}
