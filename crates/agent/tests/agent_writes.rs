//! Agents creating secrets through `AgentSession` (spec Seam 1, ADR 0012 "Agents create
//! secrets"): `create_entry`, which can have keefast generate a password it never returns, and
//! `store`, which sets a field from a value the Agent piped in. Only pairings with the create
//! right write, only in their own subgroup, never deleting, and every change keeps the Entry's
//! history and goes to the audit log without its value.

mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, MutexGuard};

use keefast_agent::{
    AgentError, AgentRequest, AgentSession, AgentSettingsExt, AuditOutcome, CreateEntry,
    StoreSecret, WrittenEntry,
};
use keefast_core::{
    CharacterRecipe, CustomField, EntryFields, EntryId, GroupId, PassphraseRecipe, PasswordRecipe,
};
use keefast_workspace::{Clipboard, UnsavedLock, VaultId, Workspace};
use secrecy::{ExposeSecret, SecretString};
use support::{FakeClock, KEEPASSXC_KDBX4, MemoryAudit, SharedVault, VaultFile};
use zeroize::Zeroizing;

/// A Vault with an Agent group, `Agents`, whose subgroups are `claude`, `shared` and `codex`.
/// The Agent pairing `claude` may create Entries and reads `shared`; `codex` may not create.
struct Fixture {
    shared: Arc<SharedVault>,
    vault: VaultId,
    claude: AgentSession,
    codex: AgentSession,
    audit: Arc<MemoryAudit>,
    shared_group: GroupId,
    _file: VaultFile,
}

impl Fixture {
    fn new() -> Self {
        let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
        let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
        let root = workspace.readable_vault(vault).unwrap().vault().root().id();
        let agents = workspace.create_group(vault, root, "Agents").unwrap();
        let shared_group = workspace.create_group(vault, agents, "shared").unwrap();
        workspace.set_agent_group(vault, agents).unwrap();
        let claude = workspace
            .create_agent_pairing(vault, "claude", vec![shared_group], true)
            .unwrap();
        let codex = workspace
            .create_agent_pairing(vault, "codex", vec![], false)
            .unwrap();
        let audit = Arc::new(MemoryAudit::default());
        Self {
            claude: AgentSession::new(claude.pairing.id().clone(), audit.clone()),
            codex: AgentSession::new(codex.pairing.id().clone(), audit.clone()),
            audit,
            shared_group,
            shared: SharedVault::new(workspace, vault),
            vault,
            _file: file,
        }
    }

    fn workspace(&self) -> MutexGuard<'_, Workspace> {
        self.shared.workspace()
    }

    fn own_group(&self) -> GroupId {
        let settings = self.workspace().agent_settings(self.vault).unwrap();
        settings.pairing(self.claude.pairing()).unwrap().group()
    }

    fn add(&self, group: GroupId, title: &str, password: &str) -> EntryId {
        let fields = EntryFields {
            title: title.to_owned(),
            password: SecretString::from(password.to_owned()),
            ..EntryFields::default()
        };
        self.workspace()
            .create_entry(self.vault, group, &fields)
            .unwrap()
    }

    fn create(
        &self,
        session: &AgentSession,
        request: CreateEntry,
    ) -> Result<WrittenEntry, AgentError> {
        session
            .handle(&*self.shared, AgentRequest::CreateEntry(request))
            .map(|reply| reply.into_written().unwrap())
    }

    fn store(
        &self,
        session: &AgentSession,
        reference: &str,
        value: &str,
    ) -> Result<WrittenEntry, AgentError> {
        let request = StoreSecret::new(reference.to_owned(), Zeroizing::new(value.to_owned()));
        session
            .handle(&*self.shared, AgentRequest::Store(request))
            .map(|reply| reply.into_written().unwrap())
    }

    /// The Entry `written` names, as the Vault has it now.
    fn entry(&self, uuid: &str) -> keefast_core::Entry {
        let id = EntryId::from_hex(uuid).unwrap();
        self.workspace().entry(self.vault, id).unwrap().clone()
    }

    fn entries_in(&self, group: GroupId) -> Vec<String> {
        let workspace = self.workspace();
        let group = workspace.group(self.vault, group).unwrap();
        group
            .entries()
            .iter()
            .map(|entry| entry.title().to_owned())
            .collect()
    }

    fn custom(entry: &keefast_core::Entry, name: &str) -> Option<String> {
        entry
            .custom_fields()
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.value.expose_secret().to_owned())
    }
}

fn new_entry(title: &str) -> CreateEntry {
    CreateEntry {
        title: title.to_owned(),
        ..CreateEntry::default()
    }
}

#[test]
fn create_entry_makes_an_entry_in_the_pairings_own_subgroup_with_what_the_agent_gave() {
    let fixture = Fixture::new();

    let written = fixture
        .create(
            &fixture.claude,
            CreateEntry {
                title: "Fly.io".to_owned(),
                username: "deploy-bot".to_owned(),
                url: "https://fly.io".to_owned(),
                tags: vec!["deploy".to_owned()],
                fields: BTreeMap::from([("Region".to_owned(), "fra".to_owned())]),
                password: None,
            },
        )
        .unwrap();

    assert_eq!(written.reference.path, "claude/Fly.io");
    assert!(written.created);
    assert_eq!(fixture.entries_in(fixture.own_group()), ["Fly.io"]);
    let entry = fixture.entry(&written.reference.uuid);
    assert_eq!(entry.username(), "deploy-bot");
    assert_eq!(entry.url(), "https://fly.io");
    assert_eq!(entry.tags(), ["deploy"]);
    assert_eq!(Fixture::custom(&entry, "Region").as_deref(), Some("fra"));
    assert_eq!(entry.password().expose_secret(), "");
}

#[test]
fn a_generated_password_is_saved_but_never_returned_or_audited() {
    let fixture = Fixture::new();

    let written = fixture
        .create(
            &fixture.claude,
            CreateEntry {
                password: Some(PasswordRecipe::Characters(CharacterRecipe {
                    length: 32,
                    symbols: false,
                    ..CharacterRecipe::default()
                })),
                ..new_entry("Database")
            },
        )
        .unwrap();

    let entry = fixture.entry(&written.reference.uuid);
    let password = entry.password().expose_secret().to_owned();
    assert_eq!(password.chars().count(), 32);
    assert!(password.chars().all(char::is_alphanumeric), "{password}");
    assert!(!format!("{written:?}").contains(&password));
    for record in fixture.audit.records() {
        assert!(!serde_json::to_string(&record).unwrap().contains(&password));
    }
}

#[test]
fn a_passphrase_can_be_generated_too() {
    let fixture = Fixture::new();

    let written = fixture
        .create(
            &fixture.claude,
            CreateEntry {
                password: Some(PasswordRecipe::Passphrase(PassphraseRecipe {
                    words: 5,
                    separator: ".".to_owned(),
                })),
                ..new_entry("Wi-Fi")
            },
        )
        .unwrap();

    let entry = fixture.entry(&written.reference.uuid);
    assert_eq!(entry.password().expose_secret().split('.').count(), 5);
}

#[test]
fn a_recipe_that_cant_make_a_password_is_refused_and_nothing_is_created() {
    let fixture = Fixture::new();

    let refused = fixture.create(
        &fixture.claude,
        CreateEntry {
            password: Some(PasswordRecipe::Characters(CharacterRecipe {
                length: 1000,
                ..CharacterRecipe::default()
            })),
            ..new_entry("Too long")
        },
    );

    assert!(
        matches!(refused, Err(AgentError::BadEntry(_))),
        "{refused:?}"
    );
    assert!(fixture.entries_in(fixture.own_group()).is_empty());
}

#[test]
fn a_pairing_without_the_create_right_creates_and_stores_nothing() {
    let fixture = Fixture::new();

    assert_eq!(
        fixture.create(&fixture.codex, new_entry("token")),
        Err(AgentError::NoCreateRight)
    );
    assert_eq!(
        fixture.store(&fixture.codex, "codex/token#password", "minted"),
        Err(AgentError::NoCreateRight)
    );
    let settings = fixture.workspace().agent_settings(fixture.vault).unwrap();
    let codex_group = settings.pairing(fixture.codex.pairing()).unwrap().group();
    assert!(fixture.entries_in(codex_group).is_empty());
}

#[test]
fn create_entry_wont_make_a_second_entry_with_the_same_path() {
    let fixture = Fixture::new();
    fixture.create(&fixture.claude, new_entry("token")).unwrap();

    let again = fixture.create(&fixture.claude, new_entry("token"));

    assert_eq!(
        again,
        Err(AgentError::EntryExists("claude/token".to_owned()))
    );
    assert_eq!(fixture.entries_in(fixture.own_group()), ["token"]);
}

#[test]
fn a_title_that_isnt_one_path_step_is_refused() {
    let fixture = Fixture::new();

    for title in ["", "  ", "nested/token"] {
        let refused = fixture.create(&fixture.claude, new_entry(title));
        assert!(
            matches!(refused, Err(AgentError::BadEntry(_))),
            "{title}: {refused:?}"
        );
    }
    assert!(fixture.entries_in(fixture.own_group()).is_empty());
}

#[test]
fn store_creates_the_entry_its_reference_names_in_the_pairings_own_subgroup() {
    let fixture = Fixture::new();

    let written = fixture
        .store(&fixture.claude, "claude/new#token", "minted-token-value")
        .unwrap();

    assert!(written.created);
    assert_eq!(written.reference.path, "claude/new");
    let entry = fixture.entry(&written.reference.uuid);
    assert_eq!(entry.title(), "new");
    assert_eq!(
        Fixture::custom(&entry, "token").as_deref(),
        Some("minted-token-value")
    );
    let field = entry
        .custom_fields()
        .iter()
        .find(|field| field.name == "token")
        .unwrap();
    assert!(field.protected, "a stored value is kept protected");
}

#[test]
fn store_sets_a_standard_field_on_an_existing_entry_and_keeps_the_old_version_in_history() {
    let fixture = Fixture::new();
    let id = fixture.add(fixture.own_group(), "GitHub", "old-password");

    let written = fixture
        .store(&fixture.claude, "claude/GitHub#password", "new-password")
        .unwrap();

    assert!(!written.created);
    assert_eq!(written.reference.uuid, id.to_hex());
    let entry = fixture.entry(&written.reference.uuid);
    assert_eq!(entry.password().expose_secret(), "new-password");
    let history: Vec<String> = entry
        .history()
        .iter()
        .map(|old| old.password().expose_secret().to_owned())
        .collect();
    assert_eq!(history, ["old-password"]);
}

#[test]
fn store_replaces_a_custom_field_by_name_and_by_uuid() {
    let fixture = Fixture::new();
    let written = fixture
        .store(&fixture.claude, "claude/npm#token", "first")
        .unwrap();

    fixture
        .store(
            &fixture.claude,
            &format!("{}#token", written.reference.uuid),
            "second",
        )
        .unwrap();

    let entry = fixture.entry(&written.reference.uuid);
    assert_eq!(Fixture::custom(&entry, "token").as_deref(), Some("second"));
    assert_eq!(entry.custom_fields().len(), 1);
    assert_eq!(entry.history().len(), 1);
}

#[test]
fn writing_an_entry_outside_the_pairings_own_subgroup_is_refused_even_one_it_reads() {
    let fixture = Fixture::new();
    let shared = fixture.add(fixture.shared_group, "shared token", "shared-value");

    let refused = fixture.store(
        &fixture.claude,
        "shared/shared token#password",
        "overwritten",
    );

    assert_eq!(
        refused,
        Err(AgentError::NotOwnEntry(
            "shared/shared token#password".to_owned()
        ))
    );
    let entry = fixture.entry(&shared.to_hex());
    assert_eq!(entry.password().expose_secret(), "shared-value");
    assert!(entry.history().is_empty());
}

#[test]
fn store_wont_create_an_entry_outside_the_pairings_own_subgroup() {
    let fixture = Fixture::new();

    for reference in [
        "shared/new#token",
        "codex/new#token",
        "new#token",
        "claude/a/b#token",
    ] {
        let refused = fixture.store(&fixture.claude, reference, "value");
        assert_eq!(
            refused,
            Err(AgentError::NotOwnEntry(reference.to_owned())),
            "{reference}"
        );
    }
    assert!(fixture.entries_in(fixture.shared_group).is_empty());
    assert!(fixture.entries_in(fixture.own_group()).is_empty());
}

#[test]
fn store_by_an_unknown_uuid_is_refused_rather_than_creating_an_entry() {
    let fixture = Fixture::new();
    let uuid = "0123456789abcdef0123456789abcdef";

    let refused = fixture.store(&fixture.claude, &format!("{uuid}#token"), "value");

    assert_eq!(
        refused,
        Err(AgentError::UnknownReference(format!("{uuid}#token")))
    );
}

#[test]
fn store_refuses_notes_the_title_a_missing_field_and_an_empty_value() {
    let fixture = Fixture::new();
    fixture.add(fixture.own_group(), "GitHub", "kept");

    for reference in [
        "claude/GitHub#notes",
        "claude/GitHub#title",
        "claude/GitHub#Notes",
    ] {
        let refused = fixture.store(&fixture.claude, reference, "value");
        assert!(
            matches!(refused, Err(AgentError::BadEntry(_))),
            "{reference}: {refused:?}"
        );
    }
    assert_eq!(
        fixture.store(&fixture.claude, "claude/GitHub", "value"),
        Err(AgentError::NoField("claude/GitHub".to_owned()))
    );
    let empty = fixture.store(&fixture.claude, "claude/GitHub#password", "");
    assert!(matches!(empty, Err(AgentError::BadEntry(_))), "{empty:?}");
    let entry = fixture.entry(
        &fixture
            .store(&fixture.claude, "claude/GitHub#username", "octocat")
            .unwrap()
            .reference
            .uuid,
    );
    assert_eq!(entry.password().expose_secret(), "kept");
    assert_eq!(entry.notes(), "");
}

/// An audit record as the test reads it: pairing, action, Entry title, field and outcome.
type Summary = (String, String, Option<String>, Option<String>, AuditOutcome);

#[test]
fn writes_are_audited_with_the_entry_and_field_but_never_the_value() {
    let fixture = Fixture::new();
    fixture.add(fixture.shared_group, "shared token", "shared-value");

    fixture
        .create(&fixture.claude, new_entry("Fly.io"))
        .unwrap();
    fixture
        .store(&fixture.claude, "claude/new#token", "minted-token-value")
        .unwrap();
    fixture
        .store(
            &fixture.claude,
            "shared/shared token#password",
            "overwrite-value",
        )
        .unwrap_err();
    fixture
        .store(&fixture.codex, "codex/x#token", "codex-value")
        .unwrap_err();

    let records = fixture.audit.records();
    let summary: Vec<Summary> = records
        .iter()
        .map(|record| {
            (
                record.pairing.clone(),
                record.action.to_string(),
                record.entry.as_ref().map(|entry| entry.title.clone()),
                record.field.clone(),
                record.outcome,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "claude".to_owned(),
                "create".to_owned(),
                Some("Fly.io".to_owned()),
                None,
                AuditOutcome::Allowed
            ),
            (
                "claude".to_owned(),
                "store".to_owned(),
                Some("new".to_owned()),
                Some("token".to_owned()),
                AuditOutcome::Allowed
            ),
            (
                "claude".to_owned(),
                "store".to_owned(),
                Some("shared token".to_owned()),
                Some("password".to_owned()),
                AuditOutcome::Denied
            ),
            (
                "codex".to_owned(),
                "store".to_owned(),
                None,
                Some("token".to_owned()),
                AuditOutcome::Denied
            ),
        ]
    );
    for record in &records {
        let json = serde_json::to_string(record).unwrap();
        for value in [
            "minted-token-value",
            "overwrite-value",
            "codex-value",
            "shared-value",
        ] {
            assert!(!json.contains(value), "{json}");
        }
    }
}

#[test]
fn a_locked_vault_answers_locked_to_writes() {
    let fixture = Fixture::new();
    let vault = fixture.vault;
    let _ = fixture.workspace().lock_vault(vault, &mut NoClipboard);
    let _ = fixture.workspace().answer_lock(vault, UnsavedLock::Discard);

    assert_eq!(
        fixture.create(&fixture.claude, new_entry("token")),
        Err(AgentError::Locked)
    );
    assert_eq!(
        fixture.store(&fixture.claude, "claude/token#password", "value"),
        Err(AgentError::Locked)
    );
}

#[test]
fn an_agent_never_deletes_and_a_stored_entry_keeps_its_other_fields() {
    let fixture = Fixture::new();
    let own = fixture.own_group();
    let id = fixture
        .workspace()
        .create_entry(
            fixture.vault,
            own,
            &EntryFields {
                title: "Stripe".to_owned(),
                username: "billing".to_owned(),
                notes: "rotate yearly".to_owned(),
                custom_fields: vec![CustomField {
                    name: "Account".to_owned(),
                    value: SecretString::from("acct_1".to_owned()),
                    protected: false,
                }],
                ..EntryFields::default()
            },
        )
        .unwrap();

    fixture
        .store(&fixture.claude, "claude/Stripe#secret key", "sk_live_x")
        .unwrap();

    let entry = fixture.entry(&id.to_hex());
    assert_eq!(entry.username(), "billing");
    assert_eq!(entry.notes(), "rotate yearly");
    assert_eq!(
        Fixture::custom(&entry, "Account").as_deref(),
        Some("acct_1")
    );
    assert_eq!(
        Fixture::custom(&entry, "secret key").as_deref(),
        Some("sk_live_x")
    );
}

struct NoClipboard;

impl Clipboard for NoClipboard {
    fn read_text(&self) -> Option<String> {
        None
    }

    fn write_text(&mut self, _text: &str) {}
}
