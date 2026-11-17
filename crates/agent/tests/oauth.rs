//! MCP clients pairing with a Headless host by OAuth 2.1, keefast being its own authorization
//! server (spec Seam 2, ADR 0012 update 2026-11-17 "OAuth"): `keefast-agent serve` in-process on
//! loopback, an rmcp client doing the MCP authorization flow as Executor or claude.ai would
//! (discovery from the 401, registration, PKCE S256), and the consent code approved by an
//! Approver or by `keefast-agent pairing approve`. The tokens it gets list the pairing's
//! secrets, rotate when refreshed, and stop working once the pairing is revoked in the Vault.

mod support;

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keefast_agent::admin::{AdminReply, AdminRequest};
use keefast_agent::host::{HeadlessHost, HostConfig, PairingChoice};
use keefast_agent::listen::Listen;
use keefast_agent::oauth::OAuthServer;
use keefast_agent::serve::McpEndpoint;
use keefast_agent::{AgentCredential, AgentSettingsExt, ApproverCredentials, ApproverHub};
use keefast_core::EntryFields;
use keefast_workspace::{LocalStorage, UnlockSecrets, Workspace};
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::auth::{AuthClient, AuthorizationRequest, OAuthState};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{FakeClock, FakeSecretStore, KEEPASSXC_KDBX4, PASSWORD, VaultFile};

const REDIRECT: &str = "http://127.0.0.1:33418/callback";

/// A Headless host serving a Vault whose Agent group has a `shared` subgroup with one Entry,
/// unlocked, with an Approver paired as `laptop`.
struct Served {
    host: Arc<HeadlessHost>,
    endpoint: McpEndpoint,
    laptop: String,
    _file: VaultFile,
}

impl Served {
    async fn start() -> Self {
        Self::start_with(OAuthServer::in_memory()).await
    }

    async fn start_with(oauth: OAuthServer) -> Self {
        let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
        {
            let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
            let root = workspace.readable_vault(vault).unwrap().vault().root().id();
            let agents = workspace.create_group(vault, root, "Agents").unwrap();
            let shared = workspace.create_group(vault, agents, "shared").unwrap();
            workspace.set_agent_group(vault, agents).unwrap();
            let fields = EntryFields {
                title: "shared key".to_owned(),
                password: SecretString::from("shared-secret-value-1234"),
                ..EntryFields::default()
            };
            workspace.create_entry(vault, shared, &fields).unwrap();
            workspace.save_vault(vault).unwrap();
        }
        let (host, _) = HeadlessHost::start(
            Workspace::new(Arc::new(LocalStorage)),
            HostConfig {
                location: file.location(),
                unattended: None,
                secret_store: Arc::new(FakeSecretStore::default()),
                audit_log: file.audit_log(),
                approvers: ApproverHub::new(),
                approver_credentials: ApproverCredentials::in_memory(),
                oauth,
            },
        );
        let listen = Listen::new("127.0.0.1:0", None, None).unwrap();
        let endpoint = McpEndpoint::start(Arc::clone(&host), &listen)
            .await
            .unwrap();
        let (_, laptop) = host.approver_credentials().pair("laptop", 0).unwrap();
        let served = Self {
            host,
            endpoint,
            laptop: laptop.to_string(),
            _file: file,
        };
        served.unlock().await;
        served
    }

    async fn unlock(&self) {
        let host = Arc::clone(&self.host);
        tokio::task::spawn_blocking(move || {
            host.unlock(UnlockSecrets::password(PASSWORD)).unwrap()
        })
        .await
        .unwrap();
    }

    fn url(&self, path: &str) -> String {
        self.endpoint.url().replace("/mcp", path)
    }

    async fn approver_stream(&self) -> ApproverStream {
        let response = http()
            .get(self.url("/approver/events"))
            .header("authorization", format!("Bearer {}", self.laptop))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        ApproverStream {
            response,
            buffer: String::new(),
        }
    }

    /// Answers the consent `id` as the `laptop` Approver: the status and the reply.
    async fn answer_consent(&self, id: &str, answer: Value) -> (u16, String) {
        let response = http()
            .post(self.url(&format!("/approver/consents/{id}")))
            .header("authorization", format!("Bearer {}", self.laptop))
            .body(answer.to_string())
            .send()
            .await
            .unwrap();
        (response.status().as_u16(), response.text().await.unwrap())
    }

    /// Registers a client called `name` by hand, as an MCP client does: its id.
    async fn register(&self, name: &str) -> String {
        let response = http()
            .post(self.url("/oauth/register"))
            .json(&json!({
                "client_name": name,
                "redirect_uris": [REDIRECT],
                "token_endpoint_auth_method": "none",
                "grant_types": ["authorization_code", "refresh_token"],
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let body: Value = response.json().await.unwrap();
        body["client_id"].as_str().unwrap().to_owned()
    }

    /// Sends a browser to the authorization endpoint for `client` with PKCE: the consent
    /// page's address and the code it shows, and the PKCE verifier.
    async fn begin(&self, client: &str) -> (Consent, String) {
        let verifier = "a-verifier-of-at-least-forty-three-characters-0123456789".to_owned();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let response = self
            .authorize(&[
                ("response_type", "code"),
                ("client_id", client),
                ("redirect_uri", REDIRECT),
                ("code_challenge", &challenge),
                ("code_challenge_method", "S256"),
                ("state", "xyz"),
                ("resource", &self.endpoint.url()),
            ])
            .await;
        assert_eq!(response.status(), 303);
        (self.consent_at(location(&response)).await, verifier)
    }

    async fn authorize(&self, query: &[(&str, &str)]) -> reqwest::Response {
        http()
            .get(format!(
                "{}?{}",
                self.url("/oauth/authorize"),
                encode(query)
            ))
            .send()
            .await
            .unwrap()
    }

    /// The consent page at `path`, as the browser shows it.
    async fn consent_at(&self, path: String) -> Consent {
        let page = http().get(self.url(&path)).send().await.unwrap();
        assert_eq!(page.status(), 200);
        let csp = page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        let html = page.text().await.unwrap();
        let code = html
            .split("<b>")
            .nth(1)
            .and_then(|rest| rest.split("</b>").next())
            .unwrap()
            .to_owned();
        Consent {
            path,
            code,
            html,
            csp,
        }
    }

    /// Where the consent page sends the browser, once it is answered.
    async fn redirect_when_answered(&self, consent: &Consent) -> String {
        for _ in 0..50 {
            let status: Value = http()
                .get(self.url(&format!("{}/status", consent.path)))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if status["state"] == "done" {
                let page = http().get(self.url(&consent.path)).send().await.unwrap();
                assert_eq!(page.status(), 303);
                assert_eq!(location(&page), status["redirect"].as_str().unwrap());
                return location(&page);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the consent was never answered");
    }

    async fn token(&self, form: &[(&str, &str)]) -> (u16, Value) {
        let response = http()
            .post(self.url("/oauth/token"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(encode(form))
            .send()
            .await
            .unwrap();
        (response.status().as_u16(), response.json().await.unwrap())
    }

    /// The tokens for `client` after its consent is approved and its code exchanged.
    async fn paired_tokens(&self, client: &str, name: &str) -> (String, String) {
        let (consent, verifier) = self.begin(client).await;
        self.approve_on_host(&consent.code, name).await.unwrap();
        let redirect = self.redirect_when_answered(&consent).await;
        let code = query(&redirect, "code").unwrap();
        let (status, tokens) = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT),
                ("client_id", client),
                ("code_verifier", &verifier),
                ("resource", &self.endpoint.url()),
            ])
            .await;
        assert_eq!(status, 200, "{tokens}");
        (
            tokens["access_token"].as_str().unwrap().to_owned(),
            tokens["refresh_token"].as_str().unwrap().to_owned(),
        )
    }

    /// `keefast-agent pairing approve <code> --name <name>`, over the admin socket's handler.
    async fn approve_on_host(&self, code: &str, name: &str) -> Result<AdminReply, AdminReply> {
        let host = Arc::clone(&self.host);
        let request = AdminRequest::PairingApprove {
            code: code.to_owned(),
            pairing: PairingChoice {
                name: name.to_owned(),
                reads: vec!["shared".to_owned()],
                can_create: false,
            },
        };
        let reply =
            tokio::task::spawn_blocking(move || keefast_agent::admin::handle(&host, request))
                .await
                .unwrap();
        match reply {
            AdminReply::Error { .. } => Err(reply),
            reply => Ok(reply),
        }
    }

    /// The status of a bare MCP request with `token`, and its `WWW-Authenticate`.
    async fn mcp_status(&self, token: Option<&str>) -> (u16, String) {
        let mut request = http()
            .post(self.endpoint.url())
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(INITIALIZE);
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = request.send().await.unwrap();
        let challenge = response
            .headers()
            .get("www-authenticate")
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_default();
        (response.status().as_u16(), challenge)
    }

    fn revoke_in_vault(&self, name: &str) {
        self.host
            .edit(|workspace, vault| {
                let settings = workspace.agent_settings(vault).unwrap();
                let pairing = settings
                    .pairings()
                    .iter()
                    .find(|pairing| pairing.name() == name)
                    .unwrap()
                    .id()
                    .clone();
                workspace.revoke_agent_pairing(vault, &pairing).unwrap();
            })
            .unwrap()
            .1
            .unwrap();
    }
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;

struct Consent {
    path: String,
    code: String,
    html: String,
    csp: String,
}

struct ApproverStream {
    response: reqwest::Response,
    buffer: String,
}

impl ApproverStream {
    /// The next event: its name and its JSON.
    async fn next(&mut self) -> (String, Value) {
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                let field = |name: &str| -> String {
                    block
                        .lines()
                        .filter_map(|line| line.strip_prefix(name))
                        .map(str::trim_start)
                        .collect()
                };
                let data = field("data:");
                if data.is_empty() {
                    continue;
                }
                return (field("event:"), serde_json::from_str(&data).unwrap());
            }
            let chunk = tokio::time::timeout(Duration::from_secs(10), self.response.chunk())
                .await
                .expect("an event within 10 s")
                .unwrap()
                .expect("the stream stays open");
            self.buffer.push_str(&String::from_utf8_lossy(&chunk));
        }
    }
}

/// A browser that doesn't follow redirects, so the test sees where it is sent.
fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

fn location(response: &reqwest::Response) -> String {
    response.headers()["location"].to_str().unwrap().to_owned()
}

fn encode(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

fn query(url: &str, name: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_without_a_token_says_where_the_protected_resource_metadata_is() {
    let served = Served::start().await;

    let (status, challenge) = served.mcp_status(None).await;
    let metadata_url = challenge
        .split("resource_metadata=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    let resource: Value = http()
        .get(&metadata_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let issuer = resource["authorization_servers"][0].as_str().unwrap();
    let server: Value = http()
        .get(format!("{issuer}/.well-known/oauth-authorization-server"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(status, 401);
    assert!(challenge.starts_with("Bearer "), "{challenge}");
    assert_eq!(
        metadata_url,
        served.url("/.well-known/oauth-protected-resource/mcp")
    );
    assert_eq!(resource["resource"], served.endpoint.url());
    assert_eq!(server["issuer"], issuer);
    assert_eq!(server["code_challenge_methods_supported"], json!(["S256"]));
    assert_eq!(
        server["registration_endpoint"],
        served.url("/oauth/register")
    );
    assert_eq!(
        server["token_endpoint_auth_methods_supported"],
        json!(["none"])
    );
    assert_eq!(
        server["grant_types_supported"],
        json!(["authorization_code", "refresh_token"])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_mcp_client_pairs_by_oauth_once_an_approver_types_the_code_and_lists_its_secrets() {
    let served = Served::start().await;
    let mut laptop = served.approver_stream().await;
    let (_, challenge) = served.mcp_status(None).await;

    // The MCP client's side, as rmcp does it: discovery from the 401, registration, PKCE.
    let mut oauth = OAuthState::new(served.endpoint.url(), None).await.unwrap();
    oauth
        .start_authorization(
            AuthorizationRequest::new(REDIRECT)
                .with_client_name("Executor")
                .with_challenge(challenge),
        )
        .await
        .unwrap();
    let authorize_url = oauth.get_authorization_url().await.unwrap();
    // The user's browser follows it to the consent page.
    let response = http().get(&authorize_url).send().await.unwrap();
    assert_eq!(response.status(), 303);
    let consent = served.consent_at(location(&response)).await;
    // The desktop keefast is asked, and the user types the code the page shows.
    let (event, offered) = laptop.next().await;
    let id = offered["id"].as_str().unwrap().to_owned();
    let wrong = served
        .answer_consent(
            &id,
            json!({ "approve": true, "code": "AAAA-AAAA", "name": "executor" }),
        )
        .await;
    let right = served
        .answer_consent(
            &id,
            json!({
                "approve": true,
                "code": consent.code.to_lowercase(),
                "name": "executor",
                "reads": ["shared"],
            }),
        )
        .await;
    let dismissed = laptop.next().await;
    let redirect = served.redirect_when_answered(&consent).await;
    oauth.handle_callback_url(&redirect).await.unwrap();
    let manager = oauth.into_authorization_manager().unwrap();
    let access = manager.get_access_token().await.unwrap();
    let client = ()
        .serve(StreamableHttpClientTransport::with_client(
            AuthClient::new(reqwest::Client::new(), manager),
            StreamableHttpClientTransportConfig::with_uri(served.endpoint.url()),
        ))
        .await
        .unwrap();
    let listed = client
        .call_tool(CallToolRequestParams::new("list_secrets"))
        .await
        .unwrap();
    let listed: String = listed
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect();

    assert_eq!(event, "consent");
    assert_eq!(offered["consent"]["client"], "Executor");
    assert_eq!(offered["consent"]["redirect"], "127.0.0.1");
    assert_eq!(offered["consent"]["subgroups"], json!(["shared"]));
    assert!(offered["consent"].get("code").is_none(), "{offered}");
    assert_eq!(consent.code.len(), 9, "{}", consent.code);
    assert!(consent.html.contains("Executor"));
    // No sign-in, and no Master key field: nothing to type at all.
    assert!(!consent.html.contains("<input"), "{}", consent.html);
    assert!(
        consent.csp.contains("frame-ancestors 'none'"),
        "{}",
        consent.csp
    );
    assert_eq!(wrong.0, 422, "{}", wrong.1);
    assert_eq!(right.0, 204, "{}", right.1);
    assert_eq!(
        dismissed,
        (
            "dismissed".to_owned(),
            json!({ "event": "dismissed", "id": id })
        )
    );
    assert_eq!(
        query(&redirect, "state").as_deref(),
        Some(query(&authorize_url, "state").unwrap().as_str())
    );
    assert_eq!(query(&redirect, "iss").unwrap(), served.url(""));
    assert!(access.starts_with("kfo_"), "{access}");
    assert!(listed.contains("shared key"), "{listed}");
    assert!(!listed.contains("shared-secret-value"), "{listed}");
    let settings = served.host.agent_settings().unwrap();
    let pairing = settings
        .pairings()
        .iter()
        .find(|pairing| pairing.name() == "executor")
        .unwrap();
    assert!(matches!(
        pairing.credential(),
        AgentCredential::OAuthClient(_)
    ));
    assert_eq!(pairing.readable().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_approve_on_the_host_approves_the_code_the_page_shows() {
    let served = Served::start().await;
    let client = served.register("Executor").await;
    let (consent, verifier) = served.begin(&client).await;

    let unknown = served.approve_on_host("ZZZZ-ZZZZ", "executor").await;
    let approved = served.approve_on_host(&consent.code, "").await.unwrap();
    let redirect = served.redirect_when_answered(&consent).await;
    let (status, tokens) = served
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &query(&redirect, "code").unwrap()),
            ("redirect_uri", REDIRECT),
            ("client_id", &client),
            ("code_verifier", &verifier),
        ])
        .await;
    let (mcp, _) = served.mcp_status(tokens["access_token"].as_str()).await;

    assert!(
        matches!(unknown, Err(AdminReply::Error { ref message, .. }) if message.contains("no client waits")),
        "{unknown:?}"
    );
    assert!(
        matches!(&approved, AdminReply::ConsentApproved { pairing, again: false, .. } if pairing == "Executor"),
        "{approved:?}"
    );
    assert_eq!(query(&redirect, "state").as_deref(), Some("xyz"));
    assert_eq!(status, 200, "{tokens}");
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["expires_in"], 3600);
    assert_eq!(mcp, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_code_is_exchanged_once_and_only_with_its_pkce_verifier() {
    let served = Served::start().await;
    let client = served.register("Executor").await;
    let (consent, verifier) = served.begin(&client).await;
    served
        .approve_on_host(&consent.code, "executor")
        .await
        .unwrap();
    let code = query(&served.redirect_when_answered(&consent).await, "code").unwrap();
    let exchange = |verifier: String| {
        let code = code.clone();
        let client = client.clone();
        let served = &served;
        async move {
            served
                .token(&[
                    ("grant_type", "authorization_code"),
                    ("code", &code),
                    ("redirect_uri", REDIRECT),
                    ("client_id", &client),
                    ("code_verifier", &verifier),
                ])
                .await
        }
    };

    let wrong = exchange("x".repeat(43)).await;
    let after_a_wrong_verifier = exchange(verifier.clone()).await;

    assert_eq!((wrong.0, &wrong.1["error"]), (400, &json!("invalid_grant")));
    // The code was spent by the first try: a stolen code is useless without the verifier, and
    // a guessed verifier gets one try.
    assert_eq!(
        (after_a_wrong_verifier.0, &after_a_wrong_verifier.1["error"]),
        (400, &json!("invalid_grant"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refresh_tokens_rotate_and_one_used_twice_revokes_the_client() {
    let served = Served::start().await;
    let client = served.register("Executor").await;
    let (access, refresh) = served.paired_tokens(&client, "executor").await;

    let (status, rotated) = served
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client),
        ])
        .await;
    let new_access = rotated["access_token"].as_str().unwrap().to_owned();
    let (reused, refusal) = served
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client),
        ])
        .await;
    let (new_refresh_after, _) = served
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", rotated["refresh_token"].as_str().unwrap()),
            ("client_id", &client),
        ])
        .await;

    assert_eq!(status, 200, "{rotated}");
    assert_ne!(rotated["refresh_token"].as_str().unwrap(), refresh);
    assert_ne!(new_access, access);
    assert_eq!((reused, &refusal["error"]), (400, &json!("invalid_grant")));
    assert_eq!(new_refresh_after, 400);
    assert_eq!(served.mcp_status(Some(&new_access)).await.0, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_the_pairing_in_the_vault_cuts_the_client_off() {
    let served = Served::start().await;
    let client = served.register("Executor").await;
    let (access, refresh) = served.paired_tokens(&client, "executor").await;
    assert_eq!(served.mcp_status(Some(&access)).await.0, 200);

    served.revoke_in_vault("executor");
    let (status, challenge) = served.mcp_status(Some(&access)).await;
    let (refreshed, refusal) = served
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client),
        ])
        .await;

    assert_eq!(status, 401);
    assert!(challenge.contains("invalid_token"), "{challenge}");
    assert_eq!(
        (refreshed, &refusal["error"]),
        (400, &json!("invalid_grant"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_paired_already_is_let_in_again_as_the_same_pairing() {
    let served = Served::start().await;
    let client = served.register("Executor").await;
    served.paired_tokens(&client, "executor").await;

    let (consent, _) = served.begin(&client).await;
    let again = served
        .approve_on_host(&consent.code, "another")
        .await
        .unwrap();

    assert!(
        matches!(&again, AdminReply::ConsentApproved { pairing, again: true, .. } if pairing == "executor"),
        "{again:?}"
    );
    assert_eq!(served.host.agent_settings().unwrap().pairings().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_approver_that_denies_or_a_cancel_on_the_page_sends_the_browser_back_denied() {
    let served = Served::start().await;
    let mut laptop = served.approver_stream().await;
    let client = served.register("Executor").await;
    let (denied, _) = served.begin(&client).await;
    let (_, offered) = laptop.next().await;

    let answered = served
        .answer_consent(offered["id"].as_str().unwrap(), json!({ "approve": false }))
        .await;
    let back = served.redirect_when_answered(&denied).await;
    let (first_gone, _) = laptop.next().await;
    let (cancelled, _) = served.begin(&client).await;
    let (_, second) = laptop.next().await;
    let cancel = http()
        .post(served.url(&format!("{}/cancel", cancelled.path)))
        .send()
        .await
        .unwrap();
    let (event, gone) = loop {
        let (event, body) = laptop.next().await;
        if body["id"] == second["id"] {
            break (event, body);
        }
    };

    assert_eq!(answered.0, 204);
    assert_eq!(first_gone, "dismissed");
    assert_eq!(query(&back, "error").as_deref(), Some("access_denied"));
    assert!(query(&back, "code").is_none());
    assert_eq!(cancel.status(), 303);
    assert_eq!(
        query(&location(&cancel), "error").as_deref(),
        Some("access_denied")
    );
    assert_eq!(event, "dismissed", "{gone}");
    assert!(served.host.agent_settings().unwrap().pairings().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_authorization_endpoint_needs_pkce_s256_and_a_registered_redirect() {
    let served = Served::start().await;
    let client = served.register("Executor").await;
    let plain = served
        .authorize(&[
            ("response_type", "code"),
            ("client_id", &client),
            ("redirect_uri", REDIRECT),
            ("code_challenge", "plain-challenge"),
            ("code_challenge_method", "plain"),
            ("state", "s"),
        ])
        .await;
    let elsewhere = served
        .authorize(&[
            ("response_type", "code"),
            ("client_id", &client),
            ("redirect_uri", "https://attacker.example/cb"),
            ("code_challenge", &"a".repeat(43)),
            ("code_challenge_method", "S256"),
        ])
        .await;
    let unknown = served
        .authorize(&[("response_type", "code"), ("client_id", "kfc_nobody")])
        .await;
    let other_resource = served
        .authorize(&[
            ("response_type", "code"),
            ("client_id", &client),
            ("redirect_uri", REDIRECT),
            ("code_challenge", &"a".repeat(43)),
            ("code_challenge_method", "S256"),
            ("resource", "https://other.example/mcp"),
        ])
        .await;
    let insecure = http()
        .post(served.url("/oauth/register"))
        .json(&json!({ "redirect_uris": ["http://attacker.example/cb"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(plain.status(), 303);
    assert!(location(&plain).starts_with(REDIRECT));
    assert_eq!(
        query(&location(&plain), "error").as_deref(),
        Some("invalid_request")
    );
    assert_eq!(elsewhere.status(), 400);
    assert!(elsewhere.headers().get("location").is_none());
    assert_eq!(unknown.status(), 400);
    assert_eq!(
        query(&location(&other_resource), "error").as_deref(),
        Some("invalid_target")
    );
    assert_eq!(insecure.status(), 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_access_token_works_while_live_and_its_expiry_asks_for_a_refresh_even_while_locked() {
    let served =
        Served::start_with(OAuthServer::in_memory().with_access_lifetime(Duration::from_secs(1)))
            .await;
    let client = served.register("Executor").await;
    let (access, refresh) = served.paired_tokens(&client, "executor").await;

    let live = served.mcp_status(Some(&access)).await.0;
    let host = Arc::clone(&served.host);
    tokio::task::spawn_blocking(move || host.lock().unwrap())
        .await
        .unwrap();
    let live_while_locked = served.mcp_status(Some(&access)).await.0;
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let expired_while_locked = served.mcp_status(Some(&access)).await.0;
    let (refreshed, _) = served
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client),
        ])
        .await;

    assert_eq!(live, 200);
    // While locked, MCP answers, and each tool says keefast is locked.
    assert_eq!(live_while_locked, 200);
    assert_eq!(expired_while_locked, 401);
    assert_eq!(refreshed, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn clients_and_hashed_tokens_survive_a_restart_of_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("oauth.json");
    let served = Served::start_with(OAuthServer::open(&state).unwrap()).await;
    let client = served.register("Executor").await;
    let (_, refresh) = served.paired_tokens(&client, "executor").await;

    let kept = std::fs::read_to_string(&state).unwrap();
    let reopened = OAuthServer::open(&state).unwrap();

    assert!(!kept.contains(&refresh), "the file keeps only hashes");
    assert!(kept.contains(&client));
    assert_eq!(reopened.client_name(&client).as_deref(), Some("Executor"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&state).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_desktops_approver_client_hears_the_consent_and_approves_it_with_the_code() {
    use keefast_agent::ConsentAnswer;
    use keefast_agent::approver_client::{
        AgentHost, AnswerPromptError, ApproverClient, ApproverClientEvent, answer_consent,
    };
    let served = Served::start().await;
    let host = AgentHost {
        url: served.endpoint.url(),
        token: zeroize::Zeroizing::new(served.laptop.clone()),
    };
    let (send, heard) = std::sync::mpsc::channel();
    let _client = ApproverClient::start(host.clone(), move |event| {
        let _ = send.send(event);
    });
    let next =
        || tokio::task::block_in_place(|| heard.recv_timeout(Duration::from_secs(10)).unwrap());
    assert_eq!(next(), ApproverClientEvent::Connected);
    let client = served.register("Executor").await;
    let (consent, _) = served.begin(&client).await;

    let ApproverClientEvent::Consent {
        id,
        consent: offered,
        seconds_left,
    } = next()
    else {
        panic!("expected a consent");
    };
    let answer = |code: &str| ConsentAnswer {
        approve: true,
        code: code.to_owned(),
        name: "executor".to_owned(),
        reads: vec![],
        can_create: true,
    };
    let wrong = tokio::task::block_in_place(|| answer_consent(&host, &id, &answer("AAAAAAAA")));
    let right = tokio::task::block_in_place(|| answer_consent(&host, &id, &answer(&consent.code)));
    let again = tokio::task::block_in_place(|| answer_consent(&host, &id, &answer(&consent.code)));

    assert_eq!(offered.client, "Executor");
    assert!((590..=600).contains(&seconds_left), "{seconds_left}");
    assert!(
        matches!(&wrong, Err(AnswerPromptError::Refused(why)) if why.contains("isn't the code")),
        "{wrong:?}"
    );
    assert_eq!(right, Ok(()));
    assert_eq!(again, Err(AnswerPromptError::Gone));
    assert_eq!(next(), ApproverClientEvent::Dismissed { id });
    let settings = served.host.agent_settings().unwrap();
    assert!(
        settings
            .pairings()
            .iter()
            .any(|p| p.name() == "executor" && p.can_create())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn one_source_can_keep_only_two_consents_waiting_so_it_cant_crowd_out_the_others() {
    let served = Served::start().await;
    let client = served.register("Executor").await;

    let mut from_one = Vec::new();
    for _ in 0..3 {
        from_one.push(served.authorize_from("203.0.113.7", &client).await);
    }
    let from_another = served.authorize_from("198.51.100.9", &client).await;

    assert_eq!(from_one[0].status(), 303);
    assert!(location(&from_one[0]).starts_with("/oauth/consent/"));
    assert!(location(&from_one[1]).starts_with("/oauth/consent/"));
    assert_eq!(
        query(&location(&from_one[2]), "error").as_deref(),
        Some("temporarily_unavailable")
    );
    assert!(location(&from_another).starts_with("/oauth/consent/"));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_web_page_elsewhere_may_read_registration_or_token_answers() {
    // MCP clients register and fetch tokens from their own processes or servers, never from a
    // web page, so the host lets no other origin's page read the answers.
    let served = Served::start().await;
    let register = http()
        .post(served.url("/oauth/register"))
        .header("origin", "https://attacker.example")
        .json(&json!({ "client_name": "page", "redirect_uris": [REDIRECT] }))
        .send()
        .await
        .unwrap();
    let token = http()
        .post(served.url("/oauth/token"))
        .header("origin", "https://attacker.example")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(encode(&[("grant_type", "refresh_token"), ("refresh_token", "kfor_nobody")]))
        .send()
        .await
        .unwrap();
    let preflight = http()
        .request(reqwest::Method::OPTIONS, served.url("/oauth/token"))
        .header("origin", "https://attacker.example")
        .header("access-control-request-method", "POST")
        .send()
        .await
        .unwrap();

    for response in [&register, &token, &preflight] {
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "{} lets other origins read it",
            response.url()
        );
    }
}
