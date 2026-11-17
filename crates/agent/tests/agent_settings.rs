//! Agent settings kept in the Vault's custom data (ADR 0005 update 2026-11-16): the Agent group,
//! the Default agent policy, Agent pairings and Agent access decisions. Each is read back after
//! the Vault is saved and opened again, and survives a Merge with changes made elsewhere.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_agent::{
    AgentCredential, AgentDecision, AgentPairing, AgentSettings, AgentSettingsError,
    AgentSettingsExt, AgentUse, DefaultAgentPolicy, NewAgentPairing,
};
use keefast_core::{EntryFields, EntryId, GroupId};
use keefast_workspace::{VaultId, Workspace};
use support::{FakeClock, KEEPASSXC_KDBX4, VaultFile};

const TOKEN: &str = "kfa_the-pairings-token";

/// A Vault file whose Vault has an Agent group, `Agents`, with subgroups `claude` and `shared`.
fn vault_with_agent_group(clock: &Arc<FakeClock>) -> (VaultFile, [GroupId; 3]) {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let (mut workspace, vault) = file.open(clock);
    let root = workspace.readable_vault(vault).unwrap().vault().root().id();
    let agents = workspace.create_group(vault, root, "Agents").unwrap();
    let claude = workspace.create_group(vault, agents, "claude").unwrap();
    let shared = workspace.create_group(vault, agents, "shared").unwrap();
    workspace.set_agent_group(vault, agents).unwrap();
    workspace.save_vault(vault).unwrap();
    (file, [agents, claude, shared])
}

fn pairing_named(name: &str, group: GroupId, readable: Vec<GroupId>) -> NewAgentPairing {
    NewAgentPairing {
        name: name.to_owned(),
        group,
        readable,
        can_create: true,
        credential: AgentCredential::token(TOKEN),
    }
}

fn pairing_names(workspace: &Workspace, vault: VaultId) -> Vec<String> {
    let settings = workspace.agent_settings(vault).unwrap();
    settings
        .pairings()
        .iter()
        .map(|p| p.name().to_owned())
        .collect()
}

#[test]
fn the_agent_group_and_default_agent_policy_are_read_back_after_a_save() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (mut workspace, vault) = file.open(&clock);
    let root = workspace.readable_vault(vault).unwrap().vault().root().id();
    let group = workspace.create_group(vault, root, "Agents").unwrap();

    workspace.set_agent_group(vault, group).unwrap();
    workspace
        .set_default_agent_policy(vault, DefaultAgentPolicy::Deny)
        .unwrap();
    workspace.save_vault(vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    assert_eq!(settings.agent_group(), Some(group));
    assert_eq!(settings.default_agent_policy(), DefaultAgentPolicy::Deny);
}

#[test]
fn a_vault_without_agent_settings_has_no_agent_group_and_asks() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let (workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));

    let settings: AgentSettings = workspace.agent_settings(vault).unwrap();

    assert_eq!(settings.agent_group(), None);
    assert_eq!(settings.default_agent_policy(), DefaultAgentPolicy::Ask);
}

#[test]
fn an_agent_pairing_is_read_back_after_a_save() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [_, claude, shared]) = vault_with_agent_group(&clock);
    let (mut workspace, vault) = file.open(&clock);

    let made = workspace
        .add_agent_pairing(vault, pairing_named("claude", claude, vec![shared]))
        .unwrap();
    workspace.save_vault(vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    let [pairing] = settings.pairings() else {
        panic!("expected one pairing, got {:?}", settings.pairings());
    };
    assert_eq!(pairing.id(), made.id());
    assert_eq!(pairing.name(), "claude");
    assert_eq!(pairing.group(), claude);
    assert_eq!(pairing.readable(), [shared]);
    assert!(pairing.can_create());
    assert!(pairing.credential().accepts_token(TOKEN));
    assert!(!pairing.credential().accepts_token("kfa_another-token"));
}

#[test]
fn a_pairings_token_is_kept_in_the_vault_only_as_a_hash() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [_, claude, _]) = vault_with_agent_group(&clock);
    let (mut workspace, vault) = file.open(&clock);

    workspace
        .add_agent_pairing(vault, pairing_named("claude", claude, vec![]))
        .unwrap();

    let custom_data: Vec<(String, String)> = workspace
        .readable_vault(vault)
        .unwrap()
        .vault()
        .custom_data()
        .collect();
    assert!(
        custom_data
            .iter()
            .all(|(key, value)| !key.contains(TOKEN) && !value.contains(TOKEN))
    );
}

#[test]
fn an_agent_pairing_can_be_found_by_its_token() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [agents, claude, _]) = vault_with_agent_group(&clock);
    let (mut workspace, vault) = file.open(&clock);
    let codex_group = workspace.create_group(vault, agents, "codex").unwrap();
    workspace
        .add_agent_pairing(vault, pairing_named("claude", claude, vec![]))
        .unwrap();
    let mut codex = pairing_named("codex", codex_group, vec![]);
    codex.credential = AgentCredential::token("kfa_codexs-token");
    workspace.add_agent_pairing(vault, codex).unwrap();

    let settings = workspace.agent_settings(vault).unwrap();

    let found = settings.pairing_with_token("kfa_codexs-token").unwrap();
    assert_eq!(found.name(), "codex");
    assert!(settings.pairing_with_token("kfa_nobodys-token").is_none());
}

#[test]
fn a_revoked_pairing_stays_revoked_through_a_merge_with_a_copy_that_still_has_it() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [_, claude, _]) = vault_with_agent_group(&clock);
    let (mut desktop, vault) = file.open(&clock);
    let pairing = desktop
        .add_agent_pairing(vault, pairing_named("claude", claude, vec![]))
        .unwrap();
    desktop.save_vault(vault).unwrap();
    let (mut host, host_vault) = file.open(&clock);

    clock.advance(Duration::from_secs(60));
    desktop.revoke_agent_pairing(vault, pairing.id()).unwrap();
    desktop.save_vault(vault).unwrap();
    clock.advance(Duration::from_secs(60));
    host.set_default_agent_policy(host_vault, DefaultAgentPolicy::Deny)
        .unwrap();
    host.save_vault(host_vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    assert!(pairing_names(&reopened, vault).is_empty());
    assert!(pairing_names(&host, host_vault).is_empty());
    let settings = reopened.agent_settings(vault).unwrap();
    assert!(settings.pairing_with_token(TOKEN).is_none());
}

#[test]
fn pairings_made_on_two_copies_of_the_vault_are_both_kept_after_a_merge() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [agents, claude, _]) = vault_with_agent_group(&clock);
    let (mut desktop, vault) = file.open(&clock);
    let (mut host, host_vault) = file.open(&clock);
    let codex = host.create_group(host_vault, agents, "codex").unwrap();

    desktop
        .add_agent_pairing(vault, pairing_named("claude", claude, vec![]))
        .unwrap();
    desktop.save_vault(vault).unwrap();
    clock.advance(Duration::from_secs(60));
    host.add_agent_pairing(host_vault, pairing_named("codex", codex, vec![]))
        .unwrap();
    host.save_vault(host_vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    assert_eq!(pairing_names(&reopened, vault), ["claude", "codex"]);
}

/// A Vault file with an Agent group whose `claude` subgroup holds an Entry `token`, and an
/// Agent pairing for it.
fn vault_with_paired_entry(clock: &Arc<FakeClock>) -> (VaultFile, AgentPairing, EntryId) {
    let (file, [_, claude, _]) = vault_with_agent_group(clock);
    let (mut workspace, vault) = file.open(clock);
    let pairing = workspace
        .add_agent_pairing(vault, pairing_named("claude", claude, vec![]))
        .unwrap();
    let fields = EntryFields {
        title: "token".to_owned(),
        ..EntryFields::default()
    };
    let entry = workspace.create_entry(vault, claude, &fields).unwrap();
    workspace.save_vault(vault).unwrap();
    (file, pairing, entry)
}

fn gh() -> AgentUse {
    AgentUse::Program("gh".to_owned())
}

#[test]
fn an_agent_access_decision_is_read_back_after_a_save_for_its_pairing_entry_and_program() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, pairing, entry) = vault_with_paired_entry(&clock);
    let (mut workspace, vault) = file.open(&clock);

    workspace
        .remember_agent_decision(vault, entry, pairing.id(), &gh(), AgentDecision::Allow)
        .unwrap();
    let api = AgentUse::Host("api.github.com".to_owned());
    workspace
        .remember_agent_decision(vault, entry, pairing.id(), &api, AgentDecision::Deny)
        .unwrap();
    workspace.save_vault(vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    assert_eq!(
        settings.decision(entry, pairing.id(), &gh()),
        Some(AgentDecision::Allow)
    );
    assert_eq!(
        settings.decision(entry, pairing.id(), &api),
        Some(AgentDecision::Deny)
    );
    let curl = AgentUse::Program("curl".to_owned());
    assert_eq!(settings.decision(entry, pairing.id(), &curl), None);
    let decisions: Vec<_> = settings
        .decisions_on(entry)
        .map(|decision| (decision.agent_use.clone(), decision.decision))
        .collect();
    assert_eq!(
        decisions,
        [(api, AgentDecision::Deny), (gh(), AgentDecision::Allow)]
    );
}

#[test]
fn a_decision_for_one_pairing_is_not_another_pairings() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, pairing, entry) = vault_with_paired_entry(&clock);
    let (mut workspace, vault) = file.open(&clock);
    let other = workspace
        .add_agent_pairing(vault, pairing_named("codex", pairing.group(), vec![]))
        .unwrap();

    workspace
        .remember_agent_decision(vault, entry, pairing.id(), &gh(), AgentDecision::Allow)
        .unwrap();

    let settings = workspace.agent_settings(vault).unwrap();
    assert_eq!(settings.decision(entry, other.id(), &gh()), None);
}

#[test]
fn decisions_remembered_on_two_copies_of_the_vault_are_both_kept_after_a_merge() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, pairing, entry) = vault_with_paired_entry(&clock);
    let (mut desktop, vault) = file.open(&clock);
    let (mut laptop, laptop_vault) = file.open(&clock);
    let api = AgentUse::Host("api.github.com".to_owned());

    desktop
        .remember_agent_decision(vault, entry, pairing.id(), &gh(), AgentDecision::Allow)
        .unwrap();
    desktop.save_vault(vault).unwrap();
    clock.advance(Duration::from_secs(60));
    laptop
        .remember_agent_decision(
            laptop_vault,
            entry,
            pairing.id(),
            &api,
            AgentDecision::Allow,
        )
        .unwrap();
    laptop.save_vault(laptop_vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    assert_eq!(
        settings.decision(entry, pairing.id(), &gh()),
        Some(AgentDecision::Allow)
    );
    assert_eq!(
        settings.decision(entry, pairing.id(), &api),
        Some(AgentDecision::Allow)
    );
}

#[test]
fn a_forgotten_decision_stays_forgotten_through_a_merge_with_a_copy_that_still_has_it() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, pairing, entry) = vault_with_paired_entry(&clock);
    let (mut desktop, vault) = file.open(&clock);
    desktop
        .remember_agent_decision(vault, entry, pairing.id(), &gh(), AgentDecision::Allow)
        .unwrap();
    desktop.save_vault(vault).unwrap();
    let (mut host, host_vault) = file.open(&clock);

    clock.advance(Duration::from_secs(60));
    desktop
        .forget_agent_decision(vault, entry, pairing.id(), &gh())
        .unwrap();
    desktop.save_vault(vault).unwrap();
    clock.advance(Duration::from_secs(60));
    host.set_default_agent_policy(host_vault, DefaultAgentPolicy::Allow)
        .unwrap();
    host.save_vault(host_vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    assert_eq!(settings.decision(entry, pairing.id(), &gh()), None);
    assert_eq!(settings.decisions_on(entry).count(), 0);
}

#[test]
fn creating_a_pairing_makes_its_subgroup_and_gives_its_token_once() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [agents, _, shared]) = vault_with_agent_group(&clock);
    let (mut workspace, vault) = file.open(&clock);

    let created = workspace
        .create_agent_pairing(vault, "codex", vec![shared], true)
        .unwrap();
    workspace.save_vault(vault).unwrap();

    assert!(created.token.starts_with(keefast_agent::TOKEN_PREFIX));
    let (reopened, vault) = file.open(&clock);
    let open = reopened.readable_vault(vault).unwrap();
    let group = open.vault().group(created.pairing.group()).unwrap();
    assert_eq!(group.name(), "codex");
    let agent_group = open.vault().group(agents).unwrap();
    assert!(agent_group.groups().iter().any(|g| g.id() == group.id()));
    let settings = reopened.agent_settings(vault).unwrap();
    let pairing = settings.pairing_with_token(&created.token).unwrap();
    assert_eq!(pairing.name(), "codex");
    assert_eq!(pairing.readable(), [shared]);
    assert!(pairing.can_create());
    let custom_data: Vec<(String, String)> = open.vault().custom_data().collect();
    assert!(
        custom_data
            .iter()
            .all(|(_, value)| !value.contains(created.token.as_str()))
    );
}

#[test]
fn a_pairing_made_again_after_a_revoke_gets_the_subgroup_its_entries_were_left_in() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [_, claude, _]) = vault_with_agent_group(&clock);
    let (mut workspace, vault) = file.open(&clock);
    let first = workspace
        .create_agent_pairing(vault, "claude", vec![], false)
        .unwrap();
    workspace
        .revoke_agent_pairing(vault, first.pairing.id())
        .unwrap();

    let again = workspace
        .create_agent_pairing(vault, "claude", vec![], false)
        .unwrap();

    assert_eq!(first.pairing.group(), claude);
    assert_eq!(again.pairing.group(), claude);
    assert_ne!(again.token, first.token);
}

#[test]
fn a_pairing_needs_an_agent_group_and_a_name_no_other_pairing_has() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let (mut workspace, vault) = file.open(&clock);
    assert_eq!(
        workspace
            .create_agent_pairing(vault, "claude", vec![], false)
            .unwrap_err(),
        AgentSettingsError::NoAgentGroup
    );

    let (file, _) = vault_with_agent_group(&clock);
    let (mut workspace, vault) = file.open(&clock);
    workspace
        .create_agent_pairing(vault, "claude", vec![], false)
        .unwrap();
    let refused = |workspace: &mut Workspace, name: &str| {
        workspace
            .create_agent_pairing(vault, name, vec![], false)
            .unwrap_err()
    };
    assert_eq!(
        refused(&mut workspace, " claude "),
        AgentSettingsError::PairingNameTaken("claude".to_owned())
    );
    assert_eq!(
        refused(&mut workspace, "  "),
        AgentSettingsError::PairingNameEmpty
    );
    assert_eq!(
        refused(&mut workspace, "a/b"),
        AgentSettingsError::PairingNameHasSlash
    );
}

#[test]
fn which_subgroups_a_pairing_reads_and_whether_it_creates_can_be_changed() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, [_, _, shared]) = vault_with_agent_group(&clock);
    let (mut workspace, vault) = file.open(&clock);
    let created = workspace
        .create_agent_pairing(vault, "claude", vec![], false)
        .unwrap();

    workspace
        .set_agent_pairing_access(vault, created.pairing.id(), vec![shared], true)
        .unwrap();
    workspace.save_vault(vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    let pairing = settings.pairing(created.pairing.id()).unwrap();
    assert_eq!(pairing.readable(), [shared]);
    assert!(pairing.can_create());
    assert!(pairing.credential().accepts_token(&created.token));
}

#[test]
fn an_entrys_allowed_hosts_are_kept_tidy_and_read_back_after_a_save() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, _, entry) = vault_with_paired_entry(&clock);
    let (mut workspace, vault) = file.open(&clock);

    let kept = workspace
        .set_agent_allowed_hosts(
            vault,
            entry,
            &[
                " API.GitHub.com. ".to_owned(),
                "https://uploads.github.com/repos".to_owned(),
                "*.githubusercontent.com".to_owned(),
                "api.github.com".to_owned(),
                "[::1]".to_owned(),
            ],
        )
        .unwrap();
    workspace.save_vault(vault).unwrap();

    let expected = [
        "api.github.com",
        "uploads.github.com",
        "*.githubusercontent.com",
        "[::1]",
    ];
    assert_eq!(kept, expected);
    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    assert_eq!(settings.allowed_hosts(entry), expected);
}

#[test]
fn a_host_that_isnt_one_is_refused_and_nothing_changes() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, _, entry) = vault_with_paired_entry(&clock);
    let (mut workspace, vault) = file.open(&clock);
    workspace
        .set_agent_allowed_hosts(vault, entry, &["api.github.com".to_owned()])
        .unwrap();

    for bad in ["", "api.github.com:443", "*", "*.com.*", "two words", "a/b"] {
        let refused = workspace.set_agent_allowed_hosts(
            vault,
            entry,
            &["example.com".to_owned(), bad.to_owned()],
        );
        assert!(
            matches!(refused, Err(AgentSettingsError::NotAHost(_))),
            "{bad:?} was {refused:?}"
        );
    }
    let settings = workspace.agent_settings(vault).unwrap();
    assert_eq!(settings.allowed_hosts(entry), ["api.github.com"]);
}

#[test]
fn allowed_hosts_changed_on_another_copy_later_win_a_merge_and_emptied_ones_stay_empty() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, _, entry) = vault_with_paired_entry(&clock);
    let (mut desktop, vault) = file.open(&clock);
    desktop
        .set_agent_allowed_hosts(vault, entry, &["api.github.com".to_owned()])
        .unwrap();
    desktop.save_vault(vault).unwrap();
    let (mut host, host_vault) = file.open(&clock);

    clock.advance(Duration::from_secs(60));
    desktop.set_agent_allowed_hosts(vault, entry, &[]).unwrap();
    desktop.save_vault(vault).unwrap();
    clock.advance(Duration::from_secs(60));
    host.set_default_agent_policy(host_vault, DefaultAgentPolicy::Allow)
        .unwrap();
    host.save_vault(host_vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    let settings = reopened.agent_settings(vault).unwrap();
    assert!(settings.allowed_hosts(entry).is_empty());
}

#[test]
fn agents_may_reveal_is_read_back_after_a_save_and_taking_it_off_later_wins_a_merge() {
    let clock = FakeClock::at_unix_seconds(1_900_000_000);
    let (file, _, entry) = vault_with_paired_entry(&clock);
    let (mut desktop, vault) = file.open(&clock);
    assert!(
        !desktop
            .agent_settings(vault)
            .unwrap()
            .agents_may_reveal(entry)
    );
    desktop.set_agents_may_reveal(vault, entry, true).unwrap();
    desktop.save_vault(vault).unwrap();
    let (mut host, host_vault) = file.open(&clock);
    assert!(
        host.agent_settings(host_vault)
            .unwrap()
            .agents_may_reveal(entry)
    );

    clock.advance(Duration::from_secs(60));
    desktop.set_agents_may_reveal(vault, entry, false).unwrap();
    desktop.save_vault(vault).unwrap();
    clock.advance(Duration::from_secs(60));
    host.set_default_agent_policy(host_vault, DefaultAgentPolicy::Allow)
        .unwrap();
    host.save_vault(host_vault).unwrap();

    let (reopened, vault) = file.open(&clock);
    assert!(
        !reopened
            .agent_settings(vault)
            .unwrap()
            .agents_may_reveal(entry)
    );
}
