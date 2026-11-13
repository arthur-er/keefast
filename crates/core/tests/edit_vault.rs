//! Editing Entries and Groups: every edit must survive a save and reopen, keep what keefast
//! doesn't model, and read the same in an independent KDBX reader.

use keefast_core::{CompositeKey, EditError, Entry, EntryFields, Group, Timestamp, Vault};
use keepass::config::DatabaseVersion;
use keepass::{Database, DatabaseKey};
use secrecy::{ExposeSecret, SecretString};

const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const KEEPASSXC_KDBX3: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx3.kdbx");
const UNKNOWN_XML_KDBX4: &[u8] =
    include_bytes!("../../../fixtures/pykeepass-unknown-xml-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";

fn key() -> CompositeKey {
    CompositeKey::password(PASSWORD)
}

fn open(bytes: &[u8]) -> Vault {
    Vault::open(bytes, &key()).expect("Vault file opens")
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

fn fields(title: &str, password: &str) -> EntryFields {
    EntryFields {
        title: title.into(),
        username: "alice".into(),
        password: SecretString::from(password),
        url: "https://example.com".into(),
        notes: "line one\nline two".into(),
        ..EntryFields::default()
    }
}

fn group_named<'a>(vault: &'a Vault, name: &str) -> &'a Group {
    fn search<'a>(group: &'a Group, name: &str) -> Option<&'a Group> {
        if group.name() == name {
            return Some(group);
        }
        group.groups().iter().find_map(|child| search(child, name))
    }
    search(vault.root(), name).unwrap_or_else(|| panic!("no Group {name}"))
}

fn entry_titled<'a>(vault: &'a Vault, title: &str) -> &'a Entry {
    vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap_or_else(|| panic!("no Entry {title}"))
}

fn titles(group: &Group) -> Vec<&str> {
    group.entries().iter().map(Entry::title).collect()
}

fn group_names(group: &Group) -> Vec<&str> {
    group.groups().iter().map(Group::name).collect()
}

#[test]
fn a_group_is_found_again_by_its_id_in_hex() {
    let vault = open(KEEPASSXC_KDBX4);
    let work = group_named(&vault, "Work").id();

    let hex = work.to_hex();

    assert_eq!(hex.len(), 32);
    assert_eq!(keefast_core::GroupId::from_hex(&hex), Some(work));
    assert_eq!(keefast_core::GroupId::from_hex(""), None);
}

#[test]
fn a_created_entry_survives_a_save_with_every_field() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let work = group_named(&vault, "Work").id();

    let id = vault
        .create_entry(work, &fields("Bank", "s3cret & <safe>"), now())
        .unwrap();

    let vault = reopen(&vault);
    let entry = vault.entry(id).expect("created Entry is there");
    assert_eq!(entry.title(), "Bank");
    assert_eq!(entry.username(), "alice");
    assert_eq!(entry.password().expose_secret(), "s3cret & <safe>");
    assert_eq!(entry.url(), "https://example.com");
    assert_eq!(entry.notes(), "line one\nline two");
    assert_eq!(entry.last_modified(), Some(now()));
    assert!(entry.history().is_empty());
    assert_eq!(titles(group_named(&vault, "Work")), ["GitHub", "Bank"]);
}

/// Whether XML 1.0 allows the character in a document at all (its `Char` production).
fn is_xml_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

#[test]
fn characters_xml_does_not_allow_are_not_saved_so_other_clients_can_read_the_file() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let work = group_named(&vault, "Work").id();
    // What pasting a terminal's output into a note leaves behind, and a stray form feed.
    let pasted = EntryFields {
        title: "Bank\u{c}".into(),
        username: "ali\u{1b}ce".into(),
        url: "https://example.com/\u{0}".into(),
        notes: "\u{1b}[31mred\u{1b}[0m\nsecond\u{b} line\r\n\u{FFFE}".into(),
        ..fields("Bank", "s3cret")
    };

    let id = vault.create_entry(work, &pasted, now()).unwrap();
    let group = vault.create_group(work, "Team\u{7}", now()).unwrap();
    vault.rename_group(group, "Team\u{8} A", later()).unwrap();

    // The decrypted XML, as KeePassXC's and KeePass2's strict readers will see it.
    let xml = independent_xml(&vault);
    let bad: Vec<char> = xml.chars().filter(|&ch| !is_xml_char(ch)).collect();
    assert!(
        bad.is_empty(),
        "the XML holds characters XML 1.0 forbids: {bad:?}"
    );

    // What is left of the text is what was typed, otherwise unchanged.
    let vault = reopen(&vault);
    let entry = vault.entry(id).unwrap();
    assert_eq!(entry.title(), "Bank");
    assert_eq!(entry.username(), "alice");
    assert_eq!(entry.url(), "https://example.com/");
    assert_eq!(entry.notes(), "[31mred[0m\nsecond line\r\n");
    assert_eq!(entry.password().expose_secret(), "s3cret");
    assert!(group_names(group_named(&vault, "Work")).contains(&"Team A"));
}

#[test]
fn text_with_only_allowed_characters_is_saved_as_it_was() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let root = vault.root().id();
    let text = EntryFields {
        title: "Café ✓ 😀".into(),
        notes: "tab\there\nnewline\r\nand <markup> & \"quotes\" \u{10FFFF}".into(),
        ..fields("Bank", "s3cret")
    };

    let id = vault.create_entry(root, &text, now()).unwrap();

    let entry = reopen(&vault);
    let entry = entry.entry(id).unwrap();
    assert_eq!(entry.title(), "Café ✓ 😀");
    assert_eq!(
        entry.notes(),
        "tab\there\nnewline\r\nand <markup> & \"quotes\" \u{10FFFF}"
    );
}

#[test]
fn a_new_entry_password_is_stored_protected() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let root = vault.root().id();
    vault
        .create_entry(root, &fields("Bank", "plain-sight"), now())
        .unwrap();

    let xml = independent_xml(&vault);

    assert!(!xml.contains("plain-sight"));
    assert!(xml.contains("<Value>alice</Value>"));
}

#[test]
fn editing_an_entry_keeps_the_previous_version_in_its_history() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let email = entry_titled(&vault, "Email").id();

    vault
        .update_entry(email, &fields("Email", "new-mail-secret"), later())
        .unwrap();

    let vault = reopen(&vault);
    let entry = vault.entry(email).unwrap();
    assert_eq!(entry.password().expose_secret(), "new-mail-secret");
    assert_eq!(entry.username(), "alice");
    assert_eq!(entry.last_modified(), Some(later()));
    let [previous] = entry.history() else {
        panic!("expected one history entry, got {:?}", entry.history());
    };
    assert_eq!(previous.password().expose_secret(), "mail-secret");
    assert_eq!(previous.username(), "me@example.org");
    assert!(previous.history().is_empty());
}

#[test]
fn saving_unchanged_fields_records_no_history() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = vault
        .create_entry(vault.root().id(), &fields("Bank", "pw"), now())
        .unwrap();

    vault
        .update_entry(id, &fields("Bank", "pw"), later())
        .unwrap();

    let entry = vault.entry(id).unwrap();
    assert!(entry.history().is_empty());
    assert_eq!(entry.last_modified(), Some(now()));
}

#[test]
fn history_is_trimmed_to_the_vaults_history_max_items() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let id = vault
        .create_entry(vault.root().id(), &fields("Bank", "pw-0"), now())
        .unwrap();

    for version in 1..=12 {
        vault
            .update_entry(id, &fields("Bank", &format!("pw-{version}")), later())
            .unwrap();
    }

    let history = reopen(&vault).entry(id).unwrap().history().to_vec();
    let passwords: Vec<String> = history
        .iter()
        .map(|entry| entry.password().expose_secret().to_owned())
        .collect();
    let expected: Vec<String> = (2..=11).map(|version| format!("pw-{version}")).collect();
    assert_eq!(passwords, expected);
}

#[test]
fn an_entry_moves_between_groups() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let github = entry_titled(&vault, "GitHub").id();
    let personal = group_named(&vault, "Personal").id();

    vault.move_entry(github, personal, now()).unwrap();

    let vault = reopen(&vault);
    assert_eq!(titles(group_named(&vault, "Personal")), ["Email", "GitHub"]);
    assert!(titles(group_named(&vault, "Work")).is_empty());
}

#[test]
fn deleting_an_entry_moves_it_to_a_new_recycle_bin() {
    let mut vault = open(KEEPASSXC_KDBX4);
    assert_eq!(vault.recycle_bin(), None);
    let email = entry_titled(&vault, "Email").id();

    vault.delete_entry(email, now()).unwrap();

    let vault = reopen(&vault);
    let bin = vault.recycle_bin().expect("a Recycle Bin was created");
    let bin = vault.group(bin).unwrap();
    assert_eq!(bin.name(), "Recycle Bin");
    assert_eq!(titles(bin), ["Email"]);
    assert!(titles(group_named(&vault, "Personal")).is_empty());
    assert_eq!(
        group_names(vault.root()),
        ["Work", "Personal", "Recycle Bin"]
    );
}

#[test]
fn deleting_from_the_recycle_bin_deletes_for_good() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let email = entry_titled(&vault, "Email").id();
    assert_eq!(vault.deletes_entry_for_good(email), Ok(false));
    vault.delete_entry(email, now()).unwrap();
    assert_eq!(vault.deletes_entry_for_good(email), Ok(true));

    vault.delete_entry(email, later()).unwrap();

    let vault = reopen(&vault);
    assert!(vault.entry(email).is_none());
    let bin = vault.group(vault.recycle_bin().unwrap()).unwrap();
    assert!(bin.entries().is_empty());
}

#[test]
fn deleting_with_the_recycle_bin_disabled_deletes_for_good_and_records_it() {
    let mut vault = open(KEEPASSXC_KDBX4);
    disable_recycle_bin(&mut vault);
    let id = vault
        .create_entry(vault.root().id(), &fields("Doomed", "pw"), now())
        .unwrap();
    assert_eq!(vault.deletes_entry_for_good(id), Ok(true));

    vault.delete_entry(id, later()).unwrap();

    let vault = reopen(&vault);
    assert!(vault.entry(id).is_none());
    assert_eq!(vault.recycle_bin(), None);
    let xml = independent_xml(&vault);
    assert_eq!(xml.matches("<DeletedObject>").count(), 1, "{xml}");
}

/// Turns the Recycle Bin off the way another client would: by saving the setting in the file.
fn disable_recycle_bin(vault: &mut Vault) {
    let mut database = Database::parse(
        &vault.save().unwrap(),
        DatabaseKey::new().with_password(PASSWORD),
    )
    .unwrap();
    database.meta.recyclebin_enabled = Some(false);
    // The `keepass` crate writes KDBX 4.1 only.
    database.config.version = DatabaseVersion::KDB4(1);
    let mut bytes = Vec::new();
    database
        .save(&mut bytes, DatabaseKey::new().with_password(PASSWORD))
        .unwrap();
    *vault = open(&bytes);
}

#[test]
fn the_recycle_bin_is_turned_off_and_on_again_as_other_clients_read_it() {
    let mut vault = open(KEEPASSXC_KDBX4);
    assert!(vault.recycle_bin_enabled());
    let email = entry_titled(&vault, "Email").id();

    vault.set_recycle_bin_enabled(false, now());

    let mut vault = reopen(&vault);
    assert!(!vault.recycle_bin_enabled());
    assert_eq!(vault.deletes_entry_for_good(email), Ok(true));
    let independent = Database::parse(
        &vault.save().unwrap(),
        DatabaseKey::new().with_password(PASSWORD),
    )
    .unwrap();
    assert_eq!(independent.meta.recyclebin_enabled, Some(false));

    vault.set_recycle_bin_enabled(true, later());
    assert!(reopen(&vault).recycle_bin_enabled());
    assert_eq!(vault.deletes_entry_for_good(email), Ok(false));
}

#[test]
fn the_vault_is_renamed_as_other_clients_read_it() {
    let mut vault = open(KEEPASSXC_KDBX4);

    vault.rename("  Home  ", now());

    let vault = reopen(&vault);
    assert_eq!(vault.name(), "Home");
    let independent = Database::parse(
        &vault.save().unwrap(),
        DatabaseKey::new().with_password(PASSWORD),
    )
    .unwrap();
    assert_eq!(independent.meta.database_name.as_deref(), Some("Home"));
}

#[test]
fn groups_are_created_renamed_and_nested() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let work = group_named(&vault, "Work").id();

    let clients = vault.create_group(work, "Clients", now()).unwrap();
    let acme = vault.create_group(clients, "Acme", now()).unwrap();
    vault.rename_group(clients, "Customers", later()).unwrap();

    let vault = reopen(&vault);
    assert_eq!(
        group_names(group_named(&vault, "Work")),
        ["Servers", "Customers"]
    );
    let customers = vault.group(clients).unwrap();
    assert_eq!(customers.last_modified(), Some(later()));
    assert_eq!(group_names(customers), ["Acme"]);
    assert_eq!(vault.group(acme).unwrap().name(), "Acme");
}

#[test]
fn a_group_moves_with_everything_in_it() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let servers = group_named(&vault, "Servers").id();
    let personal = group_named(&vault, "Personal").id();

    vault.move_group(servers, personal, now()).unwrap();

    let vault = reopen(&vault);
    assert_eq!(group_names(group_named(&vault, "Personal")), ["Servers"]);
    assert_eq!(titles(group_named(&vault, "Servers")), ["Prod DB"]);
    assert!(group_names(group_named(&vault, "Work")).is_empty());
}

#[test]
fn a_group_cannot_move_into_itself_or_its_subgroups() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let work = group_named(&vault, "Work").id();
    let servers = group_named(&vault, "Servers").id();
    let root = vault.root().id();
    let before = reopen(&vault);

    assert_eq!(
        vault.move_group(work, servers, now()),
        Err(EditError::IntoItself)
    );
    assert_eq!(
        vault.move_group(work, work, now()),
        Err(EditError::IntoItself)
    );
    assert_eq!(
        vault.move_group(root, work, now()),
        Err(EditError::RootGroup)
    );
    assert_eq!(vault.delete_group(root, now()), Err(EditError::RootGroup));
    assert!(vault == before);
}

#[test]
fn deleting_a_group_moves_it_to_the_recycle_bin_and_deleting_it_there_removes_it() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let work = group_named(&vault, "Work").id();
    let prod_db = entry_titled(&vault, "Prod DB").id();

    vault.delete_group(work, now()).unwrap();
    let bin = vault.recycle_bin().unwrap();
    assert_eq!(group_names(vault.group(bin).unwrap()), ["Work"]);
    assert!(vault.entry(prod_db).is_some());

    vault.delete_group(work, later()).unwrap();

    let vault = reopen(&vault);
    assert!(vault.group(work).is_none());
    assert!(vault.entry(prod_db).is_none());
    assert_eq!(group_names(vault.root()), ["Personal", "Recycle Bin"]);
}

#[test]
fn deleting_the_recycle_bin_deletes_it_for_good() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let email = entry_titled(&vault, "Email").id();
    vault.delete_entry(email, now()).unwrap();
    let bin = vault.recycle_bin().unwrap();
    assert_eq!(vault.deletes_group_for_good(bin), Ok(true));

    vault.delete_group(bin, later()).unwrap();

    assert!(vault.group(bin).is_none());
    assert!(vault.entry(email).is_none());
    assert_eq!(vault.recycle_bin(), None);
}

#[test]
fn an_existing_recycle_bin_is_reused() {
    let mut vault = open(KEEPASSXC_KDBX4);
    vault
        .delete_entry(entry_titled(&vault, "Email").id(), now())
        .unwrap();
    let mut vault = reopen(&vault);
    let bin = vault.recycle_bin().unwrap();

    vault
        .delete_entry(entry_titled(&vault, "GitHub").id(), later())
        .unwrap();

    assert_eq!(vault.recycle_bin(), Some(bin));
    assert_eq!(titles(vault.group(bin).unwrap()), ["Email", "GitHub"]);
}

#[test]
fn edits_on_missing_ids_change_nothing() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let other = open(KEEPASSXC_KDBX4);
    let mut scratch = open(KEEPASSXC_KDBX4);
    let ghost_entry = scratch
        .create_entry(scratch.root().id(), &fields("Ghost", "pw"), now())
        .unwrap();
    let ghost_group = scratch
        .create_group(scratch.root().id(), "Ghost", now())
        .unwrap();
    let root = vault.root().id();

    assert_eq!(
        vault.update_entry(ghost_entry, &fields("x", "y"), now()),
        Err(EditError::EntryNotFound)
    );
    assert_eq!(
        vault.move_entry(ghost_entry, root, now()),
        Err(EditError::EntryNotFound)
    );
    assert_eq!(
        vault.create_entry(ghost_group, &fields("x", "y"), now()),
        Err(EditError::GroupNotFound)
    );
    assert_eq!(
        vault.rename_group(ghost_group, "x", now()),
        Err(EditError::GroupNotFound)
    );
    let email = entry_titled(&vault, "Email").id();
    assert_eq!(
        vault.move_entry(email, ghost_group, now()),
        Err(EditError::GroupNotFound)
    );
    assert!(vault == other);
}

#[test]
fn edits_keep_everything_keefast_does_not_model() {
    let mut vault = open(UNKNOWN_XML_KDBX4);
    let entry = vault.root().all_entries().next().unwrap().id();
    let root = vault.root().id();

    vault
        .update_entry(entry, &fields("Edited", "pw"), now())
        .unwrap();
    let group = vault.create_group(root, "New", now()).unwrap();
    vault.move_entry(entry, group, now()).unwrap();

    let xml = independent_xml(&vault);
    for expected in [
        r#"<KeefastFutureMeta Origin="pykeepass">"#,
        r#"FutureGroupAttribute="group-attribute""#,
        "<FutureGroupSetting>group-payload</FutureGroupSetting>",
        r#"FutureEntryAttribute="entry-attribute""#,
        r#"<FutureEntryField Kind="opaque">entry-payload</FutureEntryField>"#,
        "<FutureRootElement>root-payload</FutureRootElement>",
        "<FutureHistoryField>history-payload</FutureHistoryField>",
    ] {
        assert!(xml.contains(expected), "lost {expected}");
    }
    assert_eq!(reopen(&vault).entry(entry).unwrap().title(), "Edited");
}

#[test]
fn a_kdbx3_1_vault_file_can_be_edited_and_saved_as_kdbx4() {
    let mut vault = open(KEEPASSXC_KDBX3);
    let github = entry_titled(&vault, "GitHub").id();

    vault
        .update_entry(github, &fields("GitHub", "rotated"), now())
        .unwrap();

    let vault = reopen(&vault);
    let entry = vault.entry(github).unwrap();
    assert_eq!(entry.password().expose_secret(), "rotated");
    assert!(!entry.history().is_empty());
}

#[test]
fn an_independent_reader_sees_the_edits() {
    let mut vault = open(KEEPASSXC_KDBX4);
    let work = group_named(&vault, "Work").id();
    let id = vault
        .create_entry(work, &fields("Bank", "bank-pw"), now())
        .unwrap();
    vault
        .update_entry(id, &fields("Bank", "bank-pw-2"), later())
        .unwrap();
    let archive = vault
        .create_group(vault.root().id(), "Archive", now())
        .unwrap();
    vault.move_entry(id, archive, later()).unwrap();

    let bytes = vault.save().unwrap();
    let database = Database::parse(&bytes, DatabaseKey::new().with_password(PASSWORD)).unwrap();

    let bank = database
        .iter_all_entries()
        .find(|entry| entry.get_title() == Some("Bank"))
        .expect("Bank is readable");
    assert_eq!(bank.get_password(), Some("bank-pw-2"));
    assert_eq!(bank.get_username(), Some("alice"));
    let history = bank.history.as_ref().unwrap().get_entries();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].get_password(), Some("bank-pw"));
}
