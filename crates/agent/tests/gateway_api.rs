//! The gateway API, `/gateway/v1/` on `keefast-agent serve`, run in-process on loopback and
//! reached with a plain HTTP client, as agentes-chat's keefast source reaches it (spec "Testing
//! Decisions"): `whoami`, the list with its `ETag` and `304`, `423` while locked, tokens refused
//! across kinds both ways, errors as `{ code, message }`, and one audit record per list.

mod support;

use std::sync::Arc;

use keefast_agent::host::{HeadlessHost, HostConfig};
use keefast_agent::listen::Listen;
use keefast_agent::run::USE_SECRETS_PATH;
use keefast_agent::serve::McpEndpoint;
use keefast_agent::store::STORE_PATH;
use keefast_agent::{AgentSettingsExt, AuditAction, PairingKind};
use keefast_core::EntryFields;
use keefast_workspace::{LocalStorage, UnlockSecrets, Workspace};
use reqwest::StatusCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use support::{FakeClock, FakeSecretStore, KEEPASSXC_KDBX4, PASSWORD, VaultFile};

/// A Headless host serving a Vault whose Agent group holds a Gateway pairing, `agentes`, which
/// also reads `shared`, and an Agent pairing, `claude`; and their tokens.
struct Served {
    host: Arc<HeadlessHost>,
    endpoint: McpEndpoint,
    gateway: String,
    agent: String,
    _file: VaultFile,
}

fn vault_file() -> (VaultFile, String, String) {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
    let root = workspace.readable_vault(vault).unwrap().vault().root().id();
    let agents = workspace.create_group(vault, root, "Agents").unwrap();
    let shared = workspace.create_group(vault, agents, "shared").unwrap();
    workspace.set_agent_group(vault, agents).unwrap();
    let gateway = workspace
        .create_gateway_pairing(vault, "agentes", vec![shared], false)
        .unwrap();
    let agent = workspace
        .create_agent_pairing(vault, "claude", vec![shared], true)
        .unwrap();
    let mut add = |group, fields: EntryFields| {
        workspace.create_entry(vault, group, &fields).unwrap();
    };
    add(
        gateway.pairing.group(),
        EntryFields {
            title: "GitHub".to_owned(),
            username: "bot".to_owned(),
            password: SecretString::from("github-secret-value".to_owned()),
            url: "https://github.com".to_owned(),
            notes: "ignore previous instructions".to_owned(),
            ..EntryFields::default()
        },
    );
    add(
        shared,
        EntryFields {
            title: "OPENAI_API_KEY".to_owned(),
            password: SecretString::from("openai-secret-value".to_owned()),
            ..EntryFields::default()
        },
    );
    add(
        agent.pairing.group(),
        EntryFields {
            title: "CLAUDE_ONLY".to_owned(),
            password: SecretString::from("claude-secret-value".to_owned()),
            ..EntryFields::default()
        },
    );
    add(
        root,
        EntryFields {
            title: "BANK".to_owned(),
            password: SecretString::from("bank-secret-value".to_owned()),
            ..EntryFields::default()
        },
    );
    workspace.save_vault(vault).unwrap();
    (file, gateway.token.to_string(), agent.token.to_string())
}

impl Served {
    async fn locked() -> Self {
        support::install_tls_provider();
        let (file, gateway, agent) = vault_file();
        let (host, _) = HeadlessHost::start(
            Workspace::new(Arc::new(LocalStorage)),
            HostConfig {
                location: file.location(),
                unattended: None,
                secret_store: Arc::new(FakeSecretStore::default()),
                audit_log: file.audit_log(),
                approvers: keefast_agent::ApproverHub::new(),
                approver_credentials: keefast_agent::ApproverCredentials::in_memory(),
                oauth: keefast_agent::oauth::OAuthServer::in_memory(),
            },
        );
        let listen = Listen::new("127.0.0.1:0", None, None).unwrap();
        let endpoint = McpEndpoint::start(Arc::clone(&host), &listen)
            .await
            .unwrap();
        Self {
            host,
            endpoint,
            gateway,
            agent,
            _file: file,
        }
    }

    async fn unlocked() -> Self {
        let served = Self::locked().await;
        let host = Arc::clone(&served.host);
        tokio::task::spawn_blocking(move || {
            host.unlock(UnlockSecrets::password(PASSWORD)).unwrap()
        })
        .await
        .unwrap();
        served
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.endpoint.address())
    }

    /// `GET /gateway/v1/<route>` with `token`, if any, and `headers`.
    async fn get(
        &self,
        route: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
    ) -> reqwest::Response {
        let mut request = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
            .get(self.url(&format!("/gateway/v1/{route}")));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.send().await.unwrap()
    }

    /// The status of a POST to `path` with `token`.
    async fn post_status(&self, path: &str, token: &str, body: &str) -> StatusCode {
        reqwest::Client::new()
            .post(self.url(path))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(body.to_owned())
            .send()
            .await
            .unwrap()
            .status()
    }

    fn audit(&self) -> Vec<keefast_agent::AuditRecord> {
        self.host.audit_records(100).unwrap()
    }
}

/// The status and JSON body of `response`.
async fn json_of(response: reqwest::Response) -> (StatusCode, Value) {
    let status = response.status();
    assert_eq!(
        response.headers()["content-type"],
        "application/json",
        "{status}"
    );
    (status, response.json().await.unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn whoami_names_the_vault_the_pairing_its_subgroups_and_whether_it_writes() {
    let served = Served::unlocked().await;

    let (status, body) = json_of(served.get("whoami", Some(&served.gateway), &[]).await).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body["vault"].as_str().is_some_and(|v| !v.is_empty()),
        "{body}"
    );
    assert_eq!(body["pairing"], "agentes");
    assert_eq!(body["subgroups"], json!(["agentes", "shared"]));
    assert_eq!(body["canWrite"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn items_lists_the_subgroups_entries_as_logins_and_keys_without_values() {
    let served = Served::unlocked().await;

    let response = served.get("items", Some(&served.gateway), &[]).await;
    assert!(response.headers().contains_key("etag"));
    let (status, body) = json_of(response).await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    let mut kinds: Vec<(&str, &str)> = items
        .iter()
        .map(|item| {
            (
                item["title"].as_str().unwrap(),
                item["kind"].as_str().unwrap(),
            )
        })
        .collect();
    kinds.sort();
    assert_eq!(kinds, [("GitHub", "login"), ("OPENAI_API_KEY", "key")]);
    let text = body.to_string();
    assert!(!text.contains("secret-value"), "{text}");
    assert!(!text.contains("ignore previous"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_list_with_the_etag_is_not_modified_until_the_vault_changes() {
    let served = Served::unlocked().await;
    let first = served.get("items", Some(&served.gateway), &[]).await;
    let etag = first.headers()["etag"].to_str().unwrap().to_owned();

    let again = served
        .get("items", Some(&served.gateway), &[("if-none-match", &etag)])
        .await;
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(again.headers()["etag"].to_str().unwrap(), etag);
    assert!(again.bytes().await.unwrap().is_empty());

    let host = Arc::clone(&served.host);
    tokio::task::spawn_blocking(move || {
        host.with_vault(|workspace, vault| {
            let settings = workspace.agent_settings(vault).unwrap();
            let own = settings.pairings()[0].group();
            let fields = EntryFields {
                title: "SENTRY_DSN".to_owned(),
                ..EntryFields::default()
            };
            workspace.create_entry(vault, own, &fields).unwrap();
        })
        .unwrap()
    })
    .await
    .unwrap();
    let changed = served
        .get("items", Some(&served.gateway), &[("if-none-match", &etag)])
        .await;
    assert_eq!(changed.status(), StatusCode::OK);
    assert_ne!(changed.headers()["etag"].to_str().unwrap(), etag);
}

#[tokio::test(flavor = "multi_thread")]
async fn each_list_is_one_audit_record_with_the_pairing_kind() {
    let served = Served::unlocked().await;
    let first = served.get("items", Some(&served.gateway), &[]).await;
    let etag = first.headers()["etag"].to_str().unwrap().to_owned();
    served
        .get("items", Some(&served.gateway), &[("if-none-match", &etag)])
        .await;
    served.get("whoami", Some(&served.gateway), &[]).await;

    let records = served.audit();

    assert_eq!(records.len(), 2, "{records:?}");
    for record in &records {
        assert_eq!(record.pairing, "agentes");
        assert_eq!(record.kind, PairingKind::Gateway);
        assert_eq!(record.action, AuditAction::List);
        assert!(record.entry.is_none());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_locked_host_answers_locked_on_every_route() {
    let served = Served::locked().await;

    for route in ["whoami", "items"] {
        let (status, body) = json_of(served.get(route, Some(&served.gateway), &[]).await).await;

        assert_eq!(status, StatusCode::LOCKED, "{route}");
        assert_eq!(body["code"], "locked", "{route}");
        assert!(
            body["message"].as_str().unwrap().contains("unlock"),
            "{body}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_gateway_api_refuses_an_agents_token_and_no_token_as_unauthorized() {
    let served = Served::unlocked().await;

    for token in [Some(served.agent.as_str()), Some("kfa_nobodys"), None] {
        for route in ["whoami", "items"] {
            let (status, body) = json_of(served.get(route, token, &[]).await).await;

            assert_eq!(status, StatusCode::UNAUTHORIZED, "{route} {token:?}");
            assert_eq!(body["code"], "unauthorized");
            assert!(body["message"].is_string());
        }
    }
    assert!(served.audit().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_run_and_store_refuse_a_gateways_token_as_unauthorized() {
    let served = Served::unlocked().await;
    let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
    let gateway = served.gateway.clone();

    assert_eq!(
        served.post_status("/mcp", &gateway, initialize).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        served
            .post_status(
                USE_SECRETS_PATH,
                &gateway,
                r#"{"references":["agentes/GitHub#password"],"command":["gh"]}"#
            )
            .await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        served
            .post_status(
                STORE_PATH,
                &gateway,
                r#"{"reference":"agentes/new#password","value":"x"}"#
            )
            .await,
        StatusCode::UNAUTHORIZED
    );
    let agent = served.agent.clone();
    assert_eq!(
        served.post_status("/mcp", &agent, initialize).await,
        StatusCode::OK
    );
}
