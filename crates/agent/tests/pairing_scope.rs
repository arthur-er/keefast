//! `PairingScope`, what one pairing reaches in a Vault (ADR 0012, ADR 0015): the Entries in its
//! own subgroup and the subgroups it reads, with their subgroups, never the Recycle Bin's; which
//! of them it may write, known by its own subgroup's Group rather than by a name or path; and
//! how a Secret reference names one of them and a value.

mod support;

use keefast_agent::{
    AgentPairingId, AgentSettingsExt, Lookup, PairingKind, PairingScope, ReferenceError, ScopeError,
};
use keefast_core::{CustomField, EntryFields, EntryId, GroupId};
use keefast_workspace::{Clipboard, UnsavedLock, VaultId, Workspace};
use secrecy::SecretString;
use support::{FakeClock, KEEPASSXC_KDBX4, VaultFile};

/// A Vault whose Agent group, `Agents`, holds the Agent pairing `bot`'s own subgroup, a second
/// subgroup also named `bot` and `shared`, both of which `bot` reads, and the Gateway pairing
/// `gateway`'s own.
struct Fixture {
    workspace: Workspace,
    vault: VaultId,
    agents: GroupId,
    bot: AgentPairingId,
    own: GroupId,
    sibling: GroupId,
    shared: GroupId,
    gateway: AgentPairingId,
    _file: VaultFile,
}

impl Fixture {
    fn new() -> Self {
        let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
        let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
        let root = workspace.readable_vault(vault).unwrap().vault().root().id();
        let agents = workspace.create_group(vault, root, "Agents").unwrap();
        workspace.set_agent_group(vault, agents).unwrap();
        let bot = workspace
            .create_agent_pairing(vault, "bot", vec![], true)
            .unwrap()
            .pairing;
        let sibling = workspace.create_group(vault, agents, "bot").unwrap();
        let shared = workspace.create_group(vault, agents, "shared").unwrap();
        workspace
            .set_agent_pairing_access(vault, bot.id(), vec![sibling, shared], true)
            .unwrap();
        let gateway = workspace
            .create_gateway_pairing(vault, "gateway", vec![], true)
            .unwrap()
            .pairing;
        Self {
            workspace,
            vault,
            agents,
            bot: bot.id().clone(),
            own: bot.group(),
            sibling,
            shared,
            gateway: gateway.id().clone(),
            _file: file,
        }
    }

    fn add(&mut self, group: GroupId, fields: EntryFields) -> EntryId {
        self.workspace
            .create_entry(self.vault, group, &fields)
            .unwrap()
    }

    fn add_titled(&mut self, group: GroupId, title: &str) -> EntryId {
        self.add(
            group,
            EntryFields {
                title: title.to_owned(),
                password: SecretString::from(format!("{title}-value")),
                ..EntryFields::default()
            },
        )
    }

    fn scope(&self) -> PairingScope<'_> {
        PairingScope::open(&self.workspace, self.vault, &self.bot, PairingKind::Agent).unwrap()
    }
}

/// Each Entry the scope reads, as `(path, own)`, sorted.
fn seen(scope: &PairingScope<'_>) -> Vec<(String, bool)> {
    let mut seen: Vec<(String, bool)> = scope
        .entries()
        .iter()
        .map(|readable| (readable.path.clone(), readable.own))
        .collect();
    seen.sort();
    seen
}

#[test]
fn only_entries_directly_in_the_own_subgroups_group_are_own_whatever_their_paths() {
    let mut fixture = Fixture::new();
    let mine = fixture.add_titled(fixture.own, "mine");
    let theirs = fixture.add_titled(fixture.sibling, "theirs");
    fixture.add_titled(fixture.own, "a/b");
    fixture.add_titled(fixture.shared, "shared");
    let below = fixture
        .workspace
        .create_group(fixture.vault, fixture.own, "below")
        .unwrap();
    fixture.add_titled(below, "deeper");

    let scope = fixture.scope();

    assert_eq!(
        seen(&scope),
        [
            ("bot/a/b".to_owned(), true),
            ("bot/below/deeper".to_owned(), false),
            ("bot/mine".to_owned(), true),
            ("bot/theirs".to_owned(), false),
            ("shared/shared".to_owned(), false),
        ]
    );
    assert_eq!(scope.own_path(), Some("bot/"));
    assert!(scope.entry(mine).unwrap().own);
    assert!(!scope.entry(theirs).unwrap().own);
    assert!(!scope.find("bot/theirs").unwrap().own);
}

#[test]
fn a_new_title_is_one_step_directly_below_the_own_subgroups_path() {
    let fixture = Fixture::new();
    let scope = fixture.scope();

    assert_eq!(scope.new_title("bot/new"), Some("new"));
    for path in ["bot/a/b", "bot/", "bot/  ", "shared/new", "new", "bots/new"] {
        assert_eq!(scope.new_title(path), None, "{path}");
    }
}

#[test]
fn entries_outside_the_pairings_subgroups_are_out_of_scope() {
    let mut fixture = Fixture::new();
    let root = fixture
        .workspace
        .readable_vault(fixture.vault)
        .unwrap()
        .vault()
        .root()
        .id();
    let bank = fixture.add_titled(root, "bank");
    let loose = fixture.add_titled(fixture.agents, "loose");
    let gateway_group = fixture
        .workspace
        .agent_settings(fixture.vault)
        .unwrap()
        .pairing(&fixture.gateway)
        .unwrap()
        .group();
    let gateways = fixture.add_titled(gateway_group, "gateways");

    let scope = fixture.scope();

    assert!(seen(&scope).is_empty(), "{:?}", seen(&scope));
    for id in [bank, loose, gateways] {
        assert!(scope.entry(id).is_none());
        assert_eq!(scope.find(&id.to_hex()).unwrap_err(), Lookup::Unknown);
    }
}

#[test]
fn a_path_two_entries_share_is_ambiguous_but_their_uuids_are_not() {
    let mut fixture = Fixture::new();
    let mine = fixture.add_titled(fixture.own, "token");
    fixture.add_titled(fixture.sibling, "token");
    let scope = fixture.scope();

    assert_eq!(scope.find("bot/token").unwrap_err(), Lookup::Ambiguous);
    assert_eq!(
        scope.resolve("bot/token#password").unwrap_err(),
        ReferenceError::Ambiguous("bot/token#password".to_owned())
    );
    assert_eq!(scope.find(&mine.to_hex()).unwrap().entry.id(), mine);
    assert_eq!(scope.find("bot/nothing").unwrap_err(), Lookup::Unknown);
}

#[test]
fn resolve_reads_a_field_by_uuid_or_path_and_says_why_it_cant() {
    let mut fixture = Fixture::new();
    let github = fixture.add(
        fixture.own,
        EntryFields {
            title: "GitHub".to_owned(),
            username: "octocat".to_owned(),
            password: SecretString::from("ghp_value".to_owned()),
            custom_fields: vec![CustomField {
                name: "token".to_owned(),
                value: SecretString::from("token-value".to_owned()),
                protected: true,
            }],
            ..EntryFields::default()
        },
    );
    let scope = fixture.scope();

    let (entry, value) = scope.resolve("bot/GitHub#password").unwrap();
    assert_eq!((entry.id(), value.as_str()), (github, "ghp_value"));
    let (_, value) = scope
        .resolve(&format!("{}#token", github.to_hex()))
        .unwrap();
    assert_eq!(value.as_str(), "token-value");
    let (_, value) = scope.resolve("bot/GitHub#username").unwrap();
    assert_eq!(value.as_str(), "octocat");

    let refused = |reference: &str| scope.resolve(reference).unwrap_err();
    assert_eq!(
        refused("bot/GitHub"),
        ReferenceError::NoField("bot/GitHub".to_owned())
    );
    for empty_or_missing in ["bot/GitHub#url", "bot/GitHub#nope", "bot/GitHub#"] {
        assert_eq!(
            refused(empty_or_missing),
            ReferenceError::UnknownField(empty_or_missing.to_owned())
        );
    }
    assert_eq!(
        refused("bot/Gitlab#password"),
        ReferenceError::Unknown("bot/Gitlab#password".to_owned())
    );
}

#[test]
fn the_recycle_bin_is_never_in_scope_even_given_to_read() {
    let mut fixture = Fixture::new();
    let root = fixture
        .workspace
        .readable_vault(fixture.vault)
        .unwrap()
        .vault()
        .root()
        .id();
    // With the Agent group at the root, the Recycle Bin is one of its subgroups.
    fixture
        .workspace
        .set_agent_group(fixture.vault, root)
        .unwrap();
    let deleted = fixture.add_titled(fixture.shared, "deleted");
    fixture.add_titled(fixture.shared, "kept");
    fixture
        .workspace
        .delete_entry(fixture.vault, deleted)
        .unwrap();
    let bin = fixture
        .workspace
        .readable_vault(fixture.vault)
        .unwrap()
        .vault()
        .recycle_bin()
        .expect("deleting made a Recycle Bin");
    fixture
        .workspace
        .set_agent_pairing_access(fixture.vault, &fixture.bot, vec![fixture.shared, bin], true)
        .unwrap();

    let scope = fixture.scope();

    assert_eq!(seen(&scope), [("Agents/shared/kept".to_owned(), false)]);
    assert!(scope.entry(deleted).is_none());
}

#[test]
fn an_own_subgroup_gone_from_the_agent_group_or_into_the_recycle_bin_has_no_path() {
    let mut fixture = Fixture::new();
    fixture.add_titled(fixture.own, "mine");
    let root = fixture
        .workspace
        .readable_vault(fixture.vault)
        .unwrap()
        .vault()
        .root()
        .id();
    fixture
        .workspace
        .move_group(fixture.vault, fixture.own, root)
        .unwrap();
    {
        let scope = fixture.scope();
        assert_eq!(scope.own_path(), None);
        assert_eq!(scope.new_title("bot/new"), None);
        assert!(scope.entries().iter().all(|readable| !readable.own));
    }

    fixture
        .workspace
        .move_group(fixture.vault, fixture.own, fixture.agents)
        .unwrap();
    assert_eq!(fixture.scope().own_path(), Some("bot/"));
    fixture
        .workspace
        .delete_group(fixture.vault, fixture.own)
        .unwrap();
    fixture
        .workspace
        .set_agent_group(fixture.vault, root)
        .unwrap();

    let scope = fixture.scope();
    assert_eq!(scope.own_path(), None);
    assert!(scope.entries().iter().all(|readable| !readable.own));
}

#[test]
fn a_locked_vault_has_no_scope() {
    let mut fixture = Fixture::new();
    let vault = fixture.vault;
    let _ = fixture.workspace.lock_vault(vault, &mut NoClipboard);
    let _ = fixture.workspace.answer_lock(vault, UnsavedLock::Discard);

    let opened = PairingScope::open(&fixture.workspace, vault, &fixture.bot, PairingKind::Agent);

    assert_eq!(opened.err(), Some(ScopeError::Locked));
}

#[test]
fn a_pairing_gone_or_of_the_other_kind_is_not_paired() {
    let mut fixture = Fixture::new();
    let open = |fixture: &Fixture, pairing: &AgentPairingId, kind| {
        PairingScope::open(&fixture.workspace, fixture.vault, pairing, kind).err()
    };

    assert_eq!(
        open(&fixture, &fixture.bot, PairingKind::Gateway),
        Some(ScopeError::NotPaired)
    );
    assert_eq!(
        open(&fixture, &fixture.gateway, PairingKind::Agent),
        Some(ScopeError::NotPaired)
    );
    assert_eq!(open(&fixture, &fixture.gateway, PairingKind::Gateway), None);

    let bot = fixture.bot.clone();
    fixture
        .workspace
        .revoke_agent_pairing(fixture.vault, &bot)
        .unwrap();
    assert_eq!(
        open(&fixture, &fixture.bot, PairingKind::Agent),
        Some(ScopeError::NotPaired)
    );
    let vault = fixture
        .workspace
        .readable_vault(fixture.vault)
        .unwrap()
        .vault();
    assert!(PairingScope::of(vault, &fixture.bot, PairingKind::Agent).is_none());
}

struct NoClipboard;

impl Clipboard for NoClipboard {
    fn read_text(&self) -> Option<String> {
        None
    }

    fn write_text(&mut self, _text: &str) {}
}
