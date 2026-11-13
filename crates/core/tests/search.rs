//! Searching and sorting Entries: a query finds Entries by any field the user remembers, small
//! typos and the starts of words included, best matches first; the Entry list sorts by title,
//! username, URL, or when Entries were created or last modified.

use keefast_core::{
    CompositeKey, CustomField, Entry, EntryFields, EntrySearch, EntrySort, GroupId, Timestamp,
    Vault,
};
use secrecy::SecretString;

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASS2_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepass2-kdbx4.kdbx");
const KEEWEB_KDBX4: &[u8] = include_bytes!("../../../fixtures/keeweb-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_000 + minutes * 60)
}

fn entry(title: &str, username: &str, url: &str) -> EntryFields {
    EntryFields {
        title: title.into(),
        username: username.into(),
        password: SecretString::from("hunter2-secret"),
        url: url.into(),
        ..EntryFields::default()
    }
}

fn field(name: &str, value: &str, protected: bool) -> CustomField {
    CustomField {
        name: name.into(),
        value: SecretString::from(value),
        protected,
    }
}

/// A Vault with a fresh Group holding just the given Entries, created a minute apart.
fn vault_with(entries: &[EntryFields]) -> (Vault, GroupId) {
    let mut vault = open(KEEPASSXC_KDBX4);
    let root = vault.root().id();
    let group = vault.create_group(root, "Search", at(0)).unwrap();
    for (minute, fields) in entries.iter().enumerate() {
        vault
            .create_entry(group, fields, at(minute as i64 + 1))
            .unwrap();
    }
    (vault, group)
}

fn found<'a>(vault: &'a Vault, group: GroupId, query: &str, sort: EntrySort) -> Vec<&'a str> {
    let entries = vault.group(group).unwrap().all_entries();
    EntrySearch::new(query)
        .rank(entries, |entry| entry, sort)
        .into_iter()
        .map(Entry::title)
        .collect()
}

fn search<'a>(vault: &'a Vault, group: GroupId, query: &str) -> Vec<&'a str> {
    found(vault, group, query, EntrySort::Title)
}

#[test]
fn an_empty_search_lists_every_entry_sorted_by_title_ignoring_case() {
    let (vault, group) = vault_with(&[
        entry("bank", "", ""),
        entry("Alpha", "", ""),
        entry("Charlie", "", ""),
    ]);

    assert_eq!(search(&vault, group, ""), ["Alpha", "bank", "Charlie"]);
    assert_eq!(search(&vault, group, "   "), ["Alpha", "bank", "Charlie"]);
}

#[test]
fn search_finds_entries_by_title_username_url_and_notes() {
    let (vault, group) = vault_with(&[
        entry("GitHub", "octocat", "https://github.com/login"),
        entry("Bank", "alice", "https://bank.example"),
        EntryFields {
            notes: "Recovery codes are in the safe".into(),
            ..entry("Router", "admin", "http://192.168.1.1")
        },
    ]);

    assert_eq!(search(&vault, group, "github"), ["GitHub"]);
    assert_eq!(search(&vault, group, "ALICE"), ["Bank"]);
    assert_eq!(search(&vault, group, "bank.example"), ["Bank"]);
    assert_eq!(search(&vault, group, "192.168"), ["Router"]);
    assert_eq!(search(&vault, group, "recovery"), ["Router"]);
    assert!(search(&vault, group, "nothing-like-it").is_empty());
}

#[test]
fn every_word_of_the_query_has_to_match_but_not_in_the_same_field() {
    let (vault, group) = vault_with(&[
        entry("Work email", "alice", "https://mail.work.example"),
        entry("Personal email", "alice", "https://mail.home.example"),
        entry("Work chat", "bob", "https://chat.work.example"),
    ]);

    assert_eq!(
        search(&vault, group, "email alice"),
        ["Personal email", "Work email"]
    );
    assert_eq!(search(&vault, group, "work email"), ["Work email"]);
    assert_eq!(search(&vault, group, "chat alice"), Vec::<&str>::new());
}

#[test]
fn search_finds_entries_by_custom_field_names_and_unprotected_values() {
    let (vault, group) = vault_with(&[
        EntryFields {
            custom_fields: vec![field("Account number", "NL91 ABNA 0417", false)],
            ..entry("Bank", "", "")
        },
        EntryFields {
            custom_fields: vec![field("Security answer", "rosebud", true)],
            ..entry("Forum", "", "")
        },
    ]);

    assert_eq!(search(&vault, group, "account"), ["Bank"]);
    assert_eq!(search(&vault, group, "abna"), ["Bank"]);
    assert_eq!(search(&vault, group, "security answer"), ["Forum"]);
}

#[test]
fn search_never_looks_into_passwords_protected_fields_or_totp_secrets() {
    let (vault, group) = vault_with(&[EntryFields {
        custom_fields: vec![
            field("Security answer", "rosebud", true),
            field("otp", "otpauth://totp/Site?secret=JBSWY3DPEHPK3PXP", false),
        ],
        ..entry("Forum", "", "")
    }]);

    assert!(search(&vault, group, "hunter2").is_empty());
    assert!(search(&vault, group, "rosebud").is_empty());
    assert!(search(&vault, group, "JBSWY3DPEHPK3PXP").is_empty());
    // The field's name is no secret.
    assert_eq!(search(&vault, group, "otp"), ["Forum"]);
}

#[test]
fn search_finds_entries_by_tag() {
    for (bytes, tags) in [
        (KEEWEB_KDBX4, &["dev", "work"][..]),
        (KEEPASS2_KDBX4, &["dev"][..]),
    ] {
        let vault = open(bytes);
        let github = vault
            .root()
            .all_entries()
            .find(|entry| entry.title() == "GitHub")
            .unwrap();
        assert_eq!(github.tags(), tags);

        let titles: Vec<&str> = EntrySearch::new("dev")
            .rank(vault.root().all_entries(), |entry| entry, EntrySort::Title)
            .into_iter()
            .map(Entry::title)
            .collect();
        assert_eq!(titles, ["GitHub"]);
    }
}

#[test]
fn small_typos_still_find_the_entry() {
    let (vault, group) = vault_with(&[
        entry("GitHub", "octocat", "https://github.com/login"),
        entry("Bank", "alice", "https://bank.example"),
        entry("Dropbox", "alice@example.org", "https://www.dropbox.com"),
    ]);

    // A swapped pair, a missing letter, a wrong letter, an extra letter.
    assert_eq!(search(&vault, group, "gihtub"), ["GitHub"]);
    assert_eq!(search(&vault, group, "gthub"), ["GitHub"]);
    assert_eq!(search(&vault, group, "dropbix"), ["Dropbox"]);
    assert_eq!(search(&vault, group, "octoccat"), ["GitHub"]);
    // Half-typed, with a typo.
    assert_eq!(search(&vault, group, "dorpb"), ["Dropbox"]);
}

#[test]
fn the_starts_of_words_find_the_entry() {
    let (vault, group) = vault_with(&[
        entry("GitHub", "octocat", "https://github.com/login"),
        entry("Work email", "alice", "https://mail.work.example"),
        entry("Bank", "billing@rezende.dev", ""),
    ]);

    // camelCase humps start words too.
    assert_eq!(search(&vault, group, "gh"), ["GitHub"]);
    assert_eq!(search(&vault, group, "wem"), ["Work email"]);
    // Letters scattered inside words don't.
    assert!(search(&vault, group, "bdv").is_empty());
}

#[test]
fn short_words_and_big_differences_are_not_typos() {
    let (vault, group) = vault_with(&[entry("Bank", "", ""), entry("GitHub", "", "")]);

    // Too short to guess at: "bak" could be anything.
    assert!(search(&vault, group, "bak").is_empty());
    assert!(search(&vault, group, "gitlab").is_empty());
}

#[test]
fn better_matches_come_first() {
    let (vault, group) = vault_with(&[
        // Only in the notes.
        EntryFields {
            notes: "Same password as mail".into(),
            ..entry("Bank", "", "")
        },
        // In the middle of the title.
        entry("Gmail", "", ""),
        // A typo away from the title: a guess, after every real match.
        entry("Maul", "", ""),
        // A word of the title starts with it.
        entry("Work mail", "", ""),
        // The title starts with it.
        entry("Mailbox", "", ""),
        // The whole title.
        entry("Mail", "", ""),
        // The username starts with it.
        entry("Server", "mailadmin", ""),
    ]);

    assert_eq!(
        search(&vault, group, "mail"),
        [
            "Mail",
            "Mailbox",
            "Work mail",
            "Gmail",
            "Server",
            "Bank",
            "Maul"
        ]
    );
}

#[test]
fn equally_good_matches_follow_the_sort() {
    let (vault, group) = vault_with(&[
        entry("Mail B", "", ""),
        entry("Mail A", "", ""),
        entry("Mail C", "", ""),
    ]);

    assert_eq!(
        search(&vault, group, "mail"),
        ["Mail A", "Mail B", "Mail C"]
    );
    assert_eq!(
        found(&vault, group, "mail", EntrySort::Created),
        ["Mail C", "Mail A", "Mail B"]
    );
}

#[test]
fn entries_sort_by_username_or_url_with_empty_ones_last() {
    let (vault, group) = vault_with(&[
        entry("One", "carol", "https://b.example"),
        entry("Two", "", "https://a.example"),
        entry("Three", "Alice", ""),
        entry("Four", "bob", "https://C.example"),
    ]);

    assert_eq!(
        found(&vault, group, "", EntrySort::Username),
        ["Three", "Four", "One", "Two"]
    );
    assert_eq!(
        found(&vault, group, "", EntrySort::Url),
        ["Two", "One", "Four", "Three"]
    );
}

#[test]
fn entries_sort_by_when_they_were_created_or_last_modified_newest_first() {
    let (mut vault, group) = vault_with(&[
        entry("Oldest", "", ""),
        entry("Middle", "", ""),
        entry("Newest", "", ""),
    ]);
    let oldest = vault
        .group(group)
        .unwrap()
        .entries()
        .iter()
        .find(|entry| entry.title() == "Oldest")
        .unwrap()
        .id();
    vault
        .update_entry(oldest, &entry("Oldest", "edited", ""), at(60))
        .unwrap();

    assert_eq!(
        found(&vault, group, "", EntrySort::Created),
        ["Newest", "Middle", "Oldest"]
    );
    assert_eq!(
        found(&vault, group, "", EntrySort::Modified),
        ["Oldest", "Newest", "Middle"]
    );
    let edited = vault.entry(oldest).unwrap();
    assert_eq!(edited.created(), Some(at(1)));
    assert_eq!(edited.last_modified(), Some(at(60)));
}

#[test]
fn entries_from_kdbx_3_files_have_their_creation_time() {
    let vault = open(include_bytes!("../../../fixtures/keepass2-kdbx3.kdbx"));
    assert!(
        vault
            .root()
            .all_entries()
            .all(|entry| entry.created().is_some())
    );
}
