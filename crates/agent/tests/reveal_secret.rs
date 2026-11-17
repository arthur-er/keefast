//! `reveal_secret` through `AgentSession` (spec Seam 1): the rare secret that must reach the
//! model. It works only on Entries marked "Agents may reveal", asks the user every time, never
//! remembers the answer, and is audited as a reveal, never with the value.

mod support;

use std::sync::Arc;

use keefast_agent::{
    AgentDecision, AgentError, AgentRequest, AgentSession, AgentSettingsExt, AgentUse, Approval,
    AuditOutcome, DefaultAgentPolicy,
};
use keefast_core::{EntryFields, EntryId};
use keefast_workspace::VaultId;
use secrecy::SecretString;
use support::{FakeApprover, FakeClock, KEEPASSXC_KDBX4, MemoryAudit, SharedVault, VaultFile};

const SEED: &str = "the-seed-phrase-that-goes-in-a-file";

/// A Vault whose Agent group has a pairing, `claude`, with an Entry `Wallet` holding [`SEED`],
/// marked "Agents may reveal".
struct Fixture {
    shared: Arc<SharedVault>,
    vault: VaultId,
    session: AgentSession,
    audit: Arc<MemoryAudit>,
    approver: Arc<FakeApprover>,
    wallet: EntryId,
    _file: VaultFile,
}

impl Fixture {
    fn new() -> Self {
        let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
        let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
        let root = workspace.readable_vault(vault).unwrap().vault().root().id();
        let agents = workspace.create_group(vault, root, "Agents").unwrap();
        workspace.set_agent_group(vault, agents).unwrap();
        let created = workspace
            .create_agent_pairing(vault, "claude", vec![], false)
            .unwrap();
        let fields = EntryFields {
            title: "Wallet".to_owned(),
            password: SecretString::from(SEED.to_owned()),
            ..EntryFields::default()
        };
        let wallet = workspace
            .create_entry(vault, created.pairing.group(), &fields)
            .unwrap();
        workspace
            .set_agents_may_reveal(vault, wallet, true)
            .unwrap();
        let audit = Arc::new(MemoryAudit::default());
        let approver = FakeApprover::answering([]);
        Self {
            session: AgentSession::new(created.pairing.id().clone(), audit.clone())
                .with_approver(approver.clone()),
            audit,
            approver,
            wallet,
            shared: SharedVault::new(workspace, vault),
            vault,
            _file: file,
        }
    }

    fn reveal(&self, reference: &str) -> Result<String, AgentError> {
        self.session
            .handle(&*self.shared, AgentRequest::Reveal(reference.to_owned()))
            .map(|reply| reply.into_values().remove(0).value().to_owned())
    }

    fn set_policy(&self, policy: DefaultAgentPolicy) {
        self.shared
            .workspace()
            .set_default_agent_policy(self.vault, policy)
            .unwrap();
    }
}

#[test]
fn a_reveal_of_a_marked_entry_asks_saying_the_value_reaches_the_model_and_returns_it() {
    let fixture = Fixture::new();
    fixture.approver.answer(Approval::Allow { remember: false });

    assert_eq!(fixture.reveal("claude/Wallet#password").unwrap(), SEED);

    let prompts = fixture.approver.prompts();
    assert_eq!(prompts.len(), 1);
    let prompt = &prompts[0];
    assert!(prompt.reveal);
    assert!(!prompt.can_remember);
    assert_eq!(prompt.pairing, "claude");
    assert_eq!(prompt.entry_title, "Wallet");
    assert_eq!(prompt.reference, "claude/Wallet#password");
    assert_eq!(prompt.host, None);
}

#[test]
fn a_reveal_of_an_unmarked_entry_is_refused_without_asking() {
    let fixture = Fixture::new();
    fixture
        .shared
        .workspace()
        .set_agents_may_reveal(fixture.vault, fixture.wallet, false)
        .unwrap();
    fixture.approver.answer(Approval::Allow { remember: false });

    assert_eq!(
        fixture.reveal("claude/Wallet#password"),
        Err(AgentError::NotRevealable(
            "claude/Wallet#password".to_owned()
        ))
    );
    assert!(fixture.approver.prompts().is_empty());
    let records = fixture.audit.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].action, "reveal");
    assert_eq!(records[0].outcome, AuditOutcome::Denied);
    assert_eq!(
        records[0].reason.as_deref(),
        Some("not marked Agents may reveal")
    );
}

#[test]
fn no_remembered_decision_or_allowing_policy_skips_the_prompt() {
    let fixture = Fixture::new();
    fixture.set_policy(DefaultAgentPolicy::Allow);
    fixture
        .shared
        .workspace()
        .remember_agent_decision(
            fixture.vault,
            fixture.wallet,
            fixture.session.pairing(),
            &AgentUse::Program("cat".to_owned()),
            AgentDecision::Allow,
        )
        .unwrap();
    fixture.approver.answer(Approval::Allow { remember: true });
    fixture.approver.answer(Approval::Allow { remember: true });

    fixture.reveal("claude/Wallet#password").unwrap();
    fixture.reveal("claude/Wallet#password").unwrap();

    assert_eq!(fixture.approver.prompts().len(), 2);
    let settings = fixture
        .shared
        .workspace()
        .agent_settings(fixture.vault)
        .unwrap();
    assert_eq!(settings.decisions_on(fixture.wallet).count(), 1);
    let reasons: Vec<_> = fixture
        .audit
        .records()
        .into_iter()
        .map(|record| record.reason.unwrap())
        .collect();
    assert_eq!(
        reasons,
        ["allowed once at a prompt", "allowed once at a prompt"]
    );
}

#[test]
fn with_no_approver_online_a_reveal_is_denied() {
    let fixture = Fixture::new();

    assert_eq!(
        fixture.reveal("claude/Wallet#password"),
        Err(AgentError::NoApproverToReveal(
            "claude/Wallet#password".to_owned()
        ))
    );
}

#[test]
fn a_deny_at_the_prompt_refuses_the_reveal() {
    let fixture = Fixture::new();
    fixture.approver.answer(Approval::Deny { remember: true });

    assert!(matches!(
        fixture.reveal("claude/Wallet#password"),
        Err(AgentError::Denied { .. })
    ));
    let settings = fixture
        .shared
        .workspace()
        .agent_settings(fixture.vault)
        .unwrap();
    assert_eq!(settings.decisions_on(fixture.wallet).count(), 0);
}

#[test]
fn a_denying_default_agent_policy_refuses_a_reveal_without_asking() {
    let fixture = Fixture::new();
    fixture.set_policy(DefaultAgentPolicy::Deny);
    fixture.approver.answer(Approval::Allow { remember: false });

    assert!(matches!(
        fixture.reveal("claude/Wallet#password"),
        Err(AgentError::Denied { .. })
    ));
    assert!(fixture.approver.prompts().is_empty());
}

#[test]
fn a_reveal_is_audited_as_a_reveal_without_the_value() {
    let fixture = Fixture::new();
    fixture.approver.answer(Approval::Allow { remember: false });

    fixture.reveal("claude/Wallet#password").unwrap();

    let records = fixture.audit.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.action, "reveal");
    assert_eq!(record.outcome, AuditOutcome::Allowed);
    assert_eq!(record.entry.as_ref().unwrap().title, "Wallet");
    assert_eq!(record.field.as_deref(), Some("password"));
    assert_eq!(record.program, None);
    assert!(!serde_json::to_string(record).unwrap().contains(SEED));
}

#[test]
fn a_reference_the_pairing_cant_read_is_unknown_and_never_asks() {
    let fixture = Fixture::new();

    assert_eq!(
        fixture.reveal("someone/Else#password"),
        Err(AgentError::UnknownReference(
            "someone/Else#password".to_owned()
        ))
    );
    assert!(fixture.approver.prompts().is_empty());
}

#[test]
fn the_mark_is_kept_in_the_vault_and_can_be_taken_off() {
    let fixture = Fixture::new();
    let marked = |fixture: &Fixture| {
        fixture
            .shared
            .workspace()
            .agent_settings(fixture.vault)
            .unwrap()
            .agents_may_reveal(fixture.wallet)
    };
    assert!(marked(&fixture));

    fixture
        .shared
        .workspace()
        .set_agents_may_reveal(fixture.vault, fixture.wallet, false)
        .unwrap();

    assert!(!marked(&fixture));
}
