//! The OAuth authorization server's HTTP side, beside the MCP endpoint (ADR 0012 update
//! 2026-11-17 "OAuth"): the protected-resource metadata (RFC 9728), the authorization-server
//! metadata (RFC 8414), registration (RFC 7591), the authorization endpoint with its consent
//! page, and the token and revocation endpoints. [`crate::oauth`] holds the rules.
//!
//! The host doesn't know its own public address: behind `tailscale serve` or a reverse proxy,
//! clients reach it by a name it never sees configured. So every address in the metadata is
//! built from the request, from `X-Forwarded-Host` and `X-Forwarded-Proto` when a proxy sets
//! them, else from the `Host` the client used. A client that asks for the metadata at one
//! address gets an issuer at the same one, as RFC 8414 requires.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use crate::approver::{APPROVER_CONSENTS_PATH, ConsentAnswer};
use crate::host::{ConsentApprovalError, HeadlessHost};
use crate::oauth::{
    AUTHORIZATION_SERVER_PATH, AUTHORIZE_PATH, AuthorizeError, AuthorizeRequest, CONSENT_PATH,
    ConsentError, ConsentState, PROTECTED_RESOURCE_PATH, REGISTER_PATH, REVOKE_PATH,
    RegistrationRequest, TOKEN_PATH, TokenRequest,
};

/// The consent page; `{{…}}` are filled in, escaped, by [`consent_page`].
const CONSENT_PAGE: &str = include_str!("consent.html");

#[derive(Clone)]
struct Endpoint {
    host: Arc<HeadlessHost>,
    /// Whether the endpoint itself serves TLS: the scheme when no proxy says otherwise.
    tls: bool,
}

/// The routes of the authorization server, and Approvers' answers to consents.
pub(crate) fn router(host: Arc<HeadlessHost>, tls: bool) -> Router {
    let endpoint = Endpoint { host, tls };
    Router::new()
        .route(PROTECTED_RESOURCE_PATH, get(protected_resource))
        .route(
            &format!("{PROTECTED_RESOURCE_PATH}/mcp"),
            get(protected_resource),
        )
        .route(AUTHORIZATION_SERVER_PATH, get(authorization_server))
        .route(REGISTER_PATH, post(register))
        .route(AUTHORIZE_PATH, get(authorize))
        .route(&format!("{CONSENT_PATH}/{{page}}"), get(consent))
        .route(
            &format!("{CONSENT_PATH}/{{page}}/status"),
            get(consent_status),
        )
        .route(&format!("{CONSENT_PATH}/{{page}}/cancel"), post(cancel))
        .route(TOKEN_PATH, post(token))
        .route(REVOKE_PATH, post(revoke))
        .route(
            &format!("{APPROVER_CONSENTS_PATH}/{{id}}"),
            post(answer_consent),
        )
        .with_state(endpoint)
}

/// The address clients reach the host at, without a trailing `/`.
pub(crate) fn base_url(headers: &HeaderMap, uri: &Uri, tls: bool) -> String {
    let first = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let scheme = match first("x-forwarded-proto") {
        Some(proto) if proto.eq_ignore_ascii_case("https") => "https",
        Some(proto) if proto.eq_ignore_ascii_case("http") => "http",
        _ if tls => "https",
        _ => "http",
    };
    let host = first("x-forwarded-host")
        .or_else(|| first(header::HOST.as_str()))
        .map(str::to_owned)
        .or_else(|| uri.authority().map(|authority| authority.to_string()))
        // Only what a host name and port are made of: nothing that could break out of the
        // URLs and the header the address goes in.
        .filter(|host| {
            host.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-:[]_".contains(&byte))
        })
        .unwrap_or_else(|| "localhost".to_owned());
    format!("{scheme}://{host}")
}

/// The `WWW-Authenticate` challenge of a 401: where the protected-resource metadata is, so an
/// MCP client can find out how to get a token (RFC 9728 section 5.1).
pub(crate) fn challenge(headers: &HeaderMap, uri: &Uri, tls: bool, invalid: bool) -> HeaderValue {
    let base = base_url(headers, uri, tls);
    let error = if invalid {
        r#", error="invalid_token""#
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        r#"Bearer resource_metadata="{base}{PROTECTED_RESOURCE_PATH}/mcp"{error}"#
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("Bearer"))
}

/// Where a request came from: the peer, or when the peer is a reverse proxy on this machine,
/// such as `tailscale serve`, the address it says it forwarded for (the last in
/// `X-Forwarded-For`, the one it added itself).
fn source(peer: std::net::IpAddr, headers: &HeaderMap) -> std::net::IpAddr {
    if !peer.is_loopback() {
        return peer;
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|address| address.trim().parse().ok())
        .next_back()
        .unwrap_or(peer)
}

fn json_response(status: StatusCode, body: serde_json::Value) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body.to_string(),
    )
        .into_response()
}

async fn protected_resource(
    State(endpoint): State<Endpoint>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let base = base_url(&headers, &uri, endpoint.tls);
    json_response(
        StatusCode::OK,
        json!({
            "resource": format!("{base}/mcp"),
            "authorization_servers": [base],
            "bearer_methods_supported": ["header"],
            "resource_name": "keefast",
        }),
    )
}

async fn authorization_server(
    State(endpoint): State<Endpoint>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let base = base_url(&headers, &uri, endpoint.tls);
    json_response(
        StatusCode::OK,
        json!({
            "issuer": base,
            "authorization_endpoint": format!("{base}{AUTHORIZE_PATH}"),
            "token_endpoint": format!("{base}{TOKEN_PATH}"),
            "registration_endpoint": format!("{base}{REGISTER_PATH}"),
            "revocation_endpoint": format!("{base}{REVOKE_PATH}"),
            "response_types_supported": ["code"],
            "response_modes_supported": ["query"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "revocation_endpoint_auth_methods_supported": ["none"],
            "authorization_response_iss_parameter_supported": true,
            "client_id_metadata_document_supported": false,
        }),
    )
}

async fn register(State(endpoint): State<Endpoint>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<RegistrationRequest>(&body) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid_client_metadata",
                    "error_description": format!("keefast expected client metadata as JSON: {error}"),
                }),
            );
        }
    };
    let host = endpoint.host;
    match tokio::task::spawn_blocking(move || host.oauth().register(request)).await {
        Ok(Ok(client)) => json_response(
            StatusCode::CREATED,
            serde_json::to_value(client).expect("a client is plain JSON"),
        ),
        Ok(Err(error)) => json_response(
            StatusCode::BAD_REQUEST,
            json!({ "error": error.error, "error_description": error.description }),
        ),
        Err(error) => server_error(error),
    }
}

fn server_error(error: impl std::fmt::Display) -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "error": "server_error", "error_description": error.to_string() }),
    )
}

/// The value of each parameter of a query string or form body; a repeated one counts as none,
/// as OAuth 2.1 forbids repeating them.
fn params(encoded: &[u8]) -> impl Fn(&str) -> Option<String> {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(encoded).into_owned().collect();
    move |name| {
        let mut values = pairs.iter().filter(|(key, _)| key == name);
        let value = values.next()?.1.clone();
        values.next().is_none().then_some(value)
    }
}

fn see_other(location: &str) -> Response {
    match HeaderValue::from_str(location) {
        Ok(location) => (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, location),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
        )
            .into_response(),
        Err(_) => message_page(
            StatusCode::BAD_REQUEST,
            "Can't send you back",
            "The client's address can't be used.",
        ),
    }
}

async fn authorize(
    State(endpoint): State<Endpoint>,
    peer: Option<axum::Extension<crate::serve::Peer>>,
    headers: HeaderMap,
    uri: Uri,
    RawQuery(query): RawQuery,
) -> Response {
    let get = params(query.as_deref().unwrap_or_default().as_bytes());
    let request = AuthorizeRequest {
        response_type: get("response_type"),
        client_id: get("client_id"),
        redirect_uri: get("redirect_uri"),
        code_challenge: get("code_challenge"),
        code_challenge_method: get("code_challenge_method"),
        state: get("state"),
        resource: get("resource"),
        source: peer.map(|axum::Extension(peer)| source(peer.0.ip(), &headers)),
    };
    let base = base_url(&headers, &uri, endpoint.tls);
    let host = endpoint.host;
    let begun = tokio::task::spawn_blocking(move || {
        host.begin_consent(request, &base, &format!("{base}/mcp"))
    })
    .await;
    match begun {
        Ok(Ok(begun)) => see_other(&format!("{CONSENT_PATH}/{}", begun.page)),
        Ok(Err(AuthorizeError::Redirect(location))) => see_other(&location),
        Ok(Err(AuthorizeError::Page(why))) => message_page(
            StatusCode::BAD_REQUEST,
            "This request can't be approved",
            &why,
        ),
        Err(error) => message_page(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Something went wrong",
            &error.to_string(),
        ),
    }
}

async fn consent(State(endpoint): State<Endpoint>, Path(page): Path<String>) -> Response {
    endpoint.host.withdraw_expired_consents();
    match endpoint.host.oauth().consent_state(&page) {
        ConsentState::Waiting {
            code,
            client_name,
            redirect_host,
            seconds_left,
        } => consent_page(&page, &code, &client_name, &redirect_host, seconds_left),
        ConsentState::Done { redirect } => see_other(&redirect),
        ConsentState::Gone => message_page(
            StatusCode::NOT_FOUND,
            "This request expired",
            "Nobody approved it in time. Go back to your MCP client and connect again.",
        ),
    }
}

async fn consent_status(State(endpoint): State<Endpoint>, Path(page): Path<String>) -> Response {
    let body = match endpoint.host.oauth().consent_state(&page) {
        ConsentState::Waiting { seconds_left, .. } => {
            json!({ "state": "waiting", "seconds_left": seconds_left })
        }
        ConsentState::Done { redirect } => json!({ "state": "done", "redirect": redirect }),
        ConsentState::Gone => json!({ "state": "gone" }),
    };
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body.to_string(),
    )
        .into_response()
}

async fn cancel(State(endpoint): State<Endpoint>, Path(page): Path<String>) -> Response {
    match endpoint.host.cancel_consent(&page) {
        Some(redirect) => see_other(&redirect),
        None => message_page(
            StatusCode::NOT_FOUND,
            "This request expired",
            "There is nothing left to cancel.",
        ),
    }
}

async fn token(
    State(endpoint): State<Endpoint>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let get = params(&body);
    let request = TokenRequest {
        grant_type: get("grant_type"),
        code: get("code"),
        redirect_uri: get("redirect_uri"),
        client_id: get("client_id"),
        code_verifier: get("code_verifier"),
        refresh_token: get("refresh_token"),
        resource: get("resource"),
    };
    let resource = format!("{}/mcp", base_url(&headers, &uri, endpoint.tls));
    let host = endpoint.host;
    match tokio::task::spawn_blocking(move || host.oauth_token(request, &resource)).await {
        Ok(Ok(tokens)) => json_response(
            StatusCode::OK,
            serde_json::to_value(&tokens).expect("tokens are plain JSON"),
        ),
        Ok(Err(error)) => {
            let status = if error.error == "invalid_client" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_REQUEST
            };
            json_response(
                status,
                json!({ "error": error.error, "error_description": error.description }),
            )
        }
        Err(error) => server_error(error),
    }
}

/// Revokes a token (RFC 7009): always 200, whether it was known or not.
async fn revoke(State(endpoint): State<Endpoint>, body: Bytes) -> Response {
    let Some(token) = params(&body)("token") else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({ "error": "invalid_request", "error_description": "no token was sent" }),
        );
    };
    let host = endpoint.host;
    let _ = tokio::task::spawn_blocking(move || host.oauth().revoke(&token)).await;
    json_response(StatusCode::OK, json!({}))
}

/// Takes a paired Approver's answer to the consent `id`: 204, 409 if it was answered already
/// or expired, 422 with a wrong code or a pairing that can't be made, 503 while locked.
async fn answer_consent(
    State(endpoint): State<Endpoint>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let host = endpoint.host;
    let approver = crate::serve::bearer(&headers)
        .and_then(|token| host.approver_credentials().approver_with_token(&token));
    if approver.is_none() {
        return crate::serve::unauthorized_approver();
    }
    let Ok(answer) = serde_json::from_slice::<ConsentAnswer>(&body) else {
        return crate::serve::refusal(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "keefast expected {\"approve\": bool, \"code\", \"name\", \"reads\", \"can_create\"}"
                .to_owned(),
        );
    };
    let answered = tokio::task::spawn_blocking(move || host.answer_consent(&id, answer)).await;
    let error = match answered {
        Ok(Ok(_)) => return StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => error,
        Err(error) => {
            return crate::serve::refusal(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                error.to_string(),
            );
        }
    };
    let (status, kind) = match &error {
        ConsentApprovalError::Consent(ConsentError::Gone | ConsentError::UnknownCode) => {
            (StatusCode::CONFLICT, "gone")
        }
        ConsentApprovalError::Consent(ConsentError::WrongCode) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "wrong_code")
        }
        ConsentApprovalError::Locked => (StatusCode::SERVICE_UNAVAILABLE, "locked"),
        ConsentApprovalError::Pairing(_) | ConsentApprovalError::NoSubgroup(_) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "pairing")
        }
        ConsentApprovalError::Save(_) => (StatusCode::INTERNAL_SERVER_ERROR, "save"),
    };
    crate::serve::refusal(status, kind, error.to_string())
}

/// `text` with `&`, `<`, `>`, `"` and `'` escaped for HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn html(status: StatusCode, body: String, nonce: Option<&str>) -> Response {
    let script = nonce.map_or_else(|| "'none'".to_owned(), |nonce| format!("'nonce-{nonce}'"));
    let policy = format!(
        "default-src 'none'; style-src 'unsafe-inline'; connect-src 'self'; \
         script-src {script}; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"
    );
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(policy) = HeaderValue::from_str(&policy) {
        headers.insert(header::CONTENT_SECURITY_POLICY, policy);
    }
    response
}

fn consent_page(
    page: &str,
    code: &str,
    client: &str,
    redirect: &str,
    seconds_left: u64,
) -> Response {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).expect("the OS has randomness");
    let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let body = CONSENT_PAGE
        .replace("{{client}}", &escape(client))
        .replace("{{code}}", &escape(code))
        .replace("{{redirect}}", &escape(redirect))
        .replace("{{status_url}}", &format!("{CONSENT_PATH}/{page}/status"))
        .replace("{{cancel_url}}", &format!("{CONSENT_PATH}/{page}/cancel"))
        .replace("{{minutes}}", &seconds_left.div_ceil(60).to_string())
        .replace("{{seconds}}", &seconds_left.to_string())
        .replace("{{nonce}}", &nonce);
    html(StatusCode::OK, body, Some(&nonce))
}

/// A page saying `title` and `text`, styled as the consent page is.
fn message_page(status: StatusCode, title: &str, text: &str) -> Response {
    let style = CONSENT_PAGE
        .split_once("<style>")
        .and_then(|(_, rest)| rest.split_once("</style>"))
        .map_or("", |(style, _)| style);
    let body = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} · keefast</title><style>{style}</style></head><body><main>\
         <div class=\"brand\">keefast</div><h1>{title}</h1><p>{text}</p></main></body></html>",
        title = escape(title),
        text = escape(text),
    );
    html(status, body, None)
}
