//! The audit log covers everything an Agent can ask for (ADR 0012 "Audit"): listing, `run`'s
//! uses, `http_request`, `create_entry`, `store` and `reveal_secret`, each with the decision
//! that let it through or stopped it. And whatever an Agent asks, no secret value reaches the
//! log file: not a stored, generated, used, sent or revealed one.

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use keefast_agent::http::HttpRequest;
use keefast_agent::{
    AgentRequest, AgentSession, AgentSettingsExt, Approval, AuditOutcome, AuditSink, CreateEntry,
    StoreSecret, UseSecrets,
};
use keefast_core::{CharacterRecipe, EntryFields, EntryId, PasswordRecipe};
use secrecy::{ExposeSecret, SecretString};
use support::{FakeApprover, FakeClock, FakeNetwork, KEEPASSXC_KDBX4, SharedVault, VaultFile};
use zeroize::Zeroizing;

const TOKEN: &str = "ghp_the-github-token-value";
const SEED: &str = "the-seed-phrase-that-goes-in-a-file";
const STORED: &str = "sk_the-value-an-agent-stored";
const API_KEY: &str = "the-custom-api-key-value";

#[test]
fn every_kind_of_request_is_in_the_log_with_its_decision_and_no_value_is() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
    let root = workspace.readable_vault(vault).unwrap().vault().root().id();
    let agents = workspace.create_group(vault, root, "Agents").unwrap();
    workspace.set_agent_group(vault, agents).unwrap();
    let created = workspace
        .create_agent_pairing(vault, "claude", vec![], true)
        .unwrap();
    let own = created.pairing.group();
    let github = workspace
        .create_entry(
            vault,
            own,
            &EntryFields {
                title: "GitHub".to_owned(),
                username: "octocat".to_owned(),
                password: SecretString::from(TOKEN.to_owned()),
                custom_fields: vec![keefast_core::CustomField {
                    name: "API key".to_owned(),
                    value: SecretString::from(API_KEY.to_owned()),
                    protected: true,
                }],
                ..EntryFields::default()
            },
        )
        .unwrap();
    workspace
        .set_agent_allowed_hosts(vault, github, &["api.github.com".to_owned()])
        .unwrap();
    let wallet = workspace
        .create_entry(
            vault,
            own,
            &EntryFields {
                title: "Wallet".to_owned(),
                password: SecretString::from(SEED.to_owned()),
                ..EntryFields::default()
            },
        )
        .unwrap();
    workspace
        .set_agents_may_reveal(vault, wallet, true)
        .unwrap();

    let log = file.audit_log();
    let audit: Arc<dyn AuditSink> = log.clone();
    let approver = FakeApprover::answering([]);
    let network = FakeNetwork::new();
    network.resolves("api.github.com", &["140.82.112.6"]);
    network.answers(
        "https://api.github.com/user",
        200,
        &[("x-echo", TOKEN)],
        TOKEN.as_bytes(),
    );
    let session = AgentSession::new(created.pairing.id().clone(), audit)
        .with_approver(approver.clone())
        .with_transport(network);
    let shared = SharedVault::new(workspace, vault);
    let handle = |request| session.handle(&*shared, request);

    handle(AgentRequest::ListSecrets).unwrap();
    // The user allows `gh` at a prompt and remembers it; the second use needs no prompt.
    approver.answer(Approval::Allow { remember: true });
    for _ in 0..2 {
        handle(AgentRequest::UseSecrets(UseSecrets {
            references: vec![
                "claude/GitHub#password".to_owned(),
                "claude/GitHub#API key".to_owned(),
            ],
            command: vec!["gh".to_owned(), "auth".to_owned()],
        }))
        .unwrap();
    }
    approver.answer(Approval::Deny { remember: true });
    handle(AgentRequest::UseSecrets(UseSecrets {
        references: vec!["claude/GitHub#password".to_owned()],
        command: vec!["curl".to_owned()],
    }))
    .unwrap_err();
    approver.answer(Approval::Allow { remember: false });
    handle(AgentRequest::Http(HttpRequest {
        method: "GET".to_owned(),
        url: "https://api.github.com/user".to_owned(),
        headers: BTreeMap::from([(
            "Authorization".to_owned(),
            "Bearer {{secret:claude/GitHub#password}}".to_owned(),
        )]),
        ..HttpRequest::default()
    }))
    .unwrap();
    let database = handle(AgentRequest::CreateEntry(CreateEntry {
        title: "Database".to_owned(),
        password: Some(PasswordRecipe::Characters(CharacterRecipe {
            length: 32,
            ..CharacterRecipe::default()
        })),
        ..CreateEntry::default()
    }))
    .unwrap()
    .into_written()
    .unwrap();
    handle(AgentRequest::Store(StoreSecret::new(
        "claude/Database#Stripe key".to_owned(),
        Zeroizing::new(STORED.to_owned()),
    )))
    .unwrap();
    approver.answer(Approval::Allow { remember: false });
    handle(AgentRequest::Reveal("claude/Wallet#password".to_owned())).unwrap();
    approver.answer(Approval::Deny { remember: false });
    handle(AgentRequest::Reveal("claude/Wallet#password".to_owned())).unwrap_err();

    let records = log.read(100).unwrap();
    let summary: Vec<(&str, AuditOutcome, Option<&str>)> = records
        .iter()
        .map(|r| (r.action.as_str(), r.outcome, r.reason.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("list", AuditOutcome::Allowed, None),
            (
                "use",
                AuditOutcome::Allowed,
                Some("allowed and remembered at a prompt")
            ),
            ("use", AuditOutcome::Allowed, None),
            (
                "use",
                AuditOutcome::Denied,
                Some("denied and remembered at a prompt")
            ),
            (
                "http_request",
                AuditOutcome::Allowed,
                Some("allowed once at a prompt")
            ),
            ("create", AuditOutcome::Allowed, None),
            ("store", AuditOutcome::Allowed, None),
            (
                "reveal",
                AuditOutcome::Allowed,
                Some("allowed once at a prompt")
            ),
            (
                "reveal",
                AuditOutcome::Denied,
                Some("denied once at a prompt")
            ),
        ],
        "{records:#?}"
    );

    let generated = {
        let workspace = shared.workspace();
        let id = EntryId::from_hex(&database.reference.uuid).unwrap();
        workspace
            .entry(vault, id)
            .unwrap()
            .password()
            .expose_secret()
            .to_owned()
    };
    assert_eq!(generated.chars().count(), 32);
    let written = std::fs::read_to_string(log.path()).unwrap();
    for value in [TOKEN, SEED, STORED, API_KEY, generated.as_str()] {
        assert!(
            !written.contains(value),
            "{value} is in the log:\n{written}"
        );
    }
}
