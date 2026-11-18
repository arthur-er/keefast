//! The gateway API's HTTP side, beside the MCP endpoint (ADR 0015): JSON under
//! [`GATEWAY_PATH`], for Gateway pairings' bearer tokens only. [`crate::gateway`] holds the
//! rules; this only translates HTTP into [`GatewaySession`] calls and back.
//!
//! Every route sits behind [`require_gateway`], which answers `401 unauthorized` for no token,
//! one nobody has, or an Agent pairing's, without saying that a token works elsewhere, and
//! `423 locked` while the Vault is locked, when no token can be checked. Errors are
//! `{ "code", "message" }`. No route redirects: agentes-chat's client treats a 3xx as failure.
//!
//! Each route is a handler that takes the [`GatewaySession`] the bearer check left in the
//! request; later ones (create and change a login) slot in beside these in [`router`]. A body
//! that doesn't parse is `400 invalid_input` with keefast's own message, never echoing it.

use std::sync::Arc;

use axum::Extension;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use secrecy::ExposeSecret;
use serde_json::json;

use crate::gateway::{GatewayError, GatewaySession, Resolve};
use crate::host::HeadlessHost;
use crate::serve::bearer;

/// Where the gateway API is served.
pub const GATEWAY_PATH: &str = "/gateway/v1";

/// The gateway API's routes.
pub(crate) fn router(host: Arc<HeadlessHost>) -> Router {
    Router::new()
        .route(&format!("{GATEWAY_PATH}/whoami"), get(whoami))
        .route(&format!("{GATEWAY_PATH}/items"), get(items))
        .route(
            &format!("{GATEWAY_PATH}/items/{{reference}}/resolve"),
            post(resolve),
        )
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&host),
            require_gateway,
        ))
        .with_state(host)
}

/// Lets through a request with a Gateway pairing's token, with its [`GatewaySession`].
async fn require_gateway(
    State(host): State<Arc<HeadlessHost>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(token) = bearer(request.headers()) else {
        return refusal(&GatewayError::Unauthorized);
    };
    match tokio::task::spawn_blocking(move || host.gateway_session(&token)).await {
        Ok(Ok(session)) => {
            request.extensions_mut().insert(session);
            next.run(request).await
        }
        Ok(Err(error)) => refusal(&error),
        Err(error) => internal(&error),
    }
}

/// `GET /whoami`: `{ vault, pairing, subgroups, canWrite }`.
async fn whoami(
    State(host): State<Arc<HeadlessHost>>,
    Extension(session): Extension<GatewaySession>,
) -> Response {
    match tokio::task::spawn_blocking(move || session.whoami(&*host)).await {
        Ok(Ok(whoami)) => json_response(StatusCode::OK, &whoami, []),
        Ok(Err(error)) => refusal(&error),
        Err(error) => internal(&error),
    }
}

/// `GET /items`: `{ items }` with an `ETag`, or `304` with no body when `If-None-Match` names
/// the current one. Either way it is a list, and is logged as one.
async fn items(
    State(host): State<Arc<HeadlessHost>>,
    Extension(session): Extension<GatewaySession>,
    headers: HeaderMap,
) -> Response {
    let listing = match tokio::task::spawn_blocking(move || session.list(&*host)).await {
        Ok(Ok(listing)) => listing,
        Ok(Err(error)) => return refusal(&error),
        Err(error) => return internal(&error),
    };
    let etag = HeaderValue::from_str(&listing.etag).expect("an ETag is hex in quotes");
    if not_modified(&headers, &listing.etag) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    json_response(StatusCode::OK, &listing, [(header::ETAG, etag)])
}

/// `POST /items/{ref}/resolve` with `{ field, for? }`: `{ value }`, the Entry's password.
async fn resolve(
    State(host): State<Arc<HeadlessHost>>,
    Extension(session): Extension<GatewaySession>,
    Path(reference): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(request) = serde_json::from_slice::<Resolve>(&body) else {
        return refusal(&GatewayError::InvalidInput(
            "send { \"field\": \"password\" or \"key\", \"for\"?: { \"agent\"?, \"person\"?, \
             \"run\"? } } as JSON",
        ));
    };
    match tokio::task::spawn_blocking(move || session.resolve(&*host, &reference, &request)).await {
        Ok(Ok(value)) => json_response(
            StatusCode::OK,
            &json!({ "value": value.expose_secret() }),
            [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        ),
        Ok(Err(error)) => refusal(&error),
        Err(error) => internal(&error),
    }
}

/// Whether `If-None-Match` names `etag`, or any version.
fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.trim_start_matches("W/") == etag)
}

/// A [`GatewayError`] as the API answers it.
fn refusal(error: &GatewayError) -> Response {
    let status = match error {
        GatewayError::Unauthorized => StatusCode::UNAUTHORIZED,
        GatewayError::Locked => StatusCode::LOCKED,
        GatewayError::NotFound => StatusCode::NOT_FOUND,
        GatewayError::InvalidInput(_) => StatusCode::BAD_REQUEST,
    };
    let body = json!({ "code": error.code(), "message": error.to_string() });
    let challenge = match error {
        GatewayError::Unauthorized => Some(HeaderValue::from_static("Bearer")),
        _ => None,
    };
    let headers = challenge.map(|value| (header::WWW_AUTHENTICATE, value));
    json_response(status, &body, headers)
}

fn internal(error: &impl std::fmt::Display) -> Response {
    let body = json!({ "code": "internal", "message": format!("keefast failed: {error}") });
    json_response(StatusCode::INTERNAL_SERVER_ERROR, &body, [])
}

/// `body` as JSON, with `status` and `headers`.
fn json_response(
    status: StatusCode,
    body: &impl serde::Serialize,
    headers: impl IntoIterator<Item = (header::HeaderName, HeaderValue)>,
) -> Response {
    let json = serde_json::to_vec(body).expect("a reply is plain JSON");
    let mut response = (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        json,
    )
        .into_response();
    response.headers_mut().extend(headers);
    response
}
