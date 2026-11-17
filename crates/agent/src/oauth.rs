//! keefast's own OAuth 2.1 authorization server, for MCP clients that pair by OAuth rather than
//! with a bearer token from `keefast-agent pairing create` (ADR 0012 update 2026-11-17 "OAuth").
//!
//! It follows the MCP authorization spec: clients register themselves (Dynamic Client
//! Registration, RFC 7591) as public clients, and send the user's browser to the authorization
//! endpoint with a PKCE challenge, S256 only. There, instead of a sign-in or a Master key field,
//! the consent page shows a short code. The user approves that code on a desktop keefast paired
//! as an Approver, or with `keefast-agent pairing approve <code>` on the host, naming the
//! pairing and choosing what it reads; only then does the page send the browser back to the
//! client with an authorization code. Typing the code, rather than clicking Allow on a prompt,
//! is what ties the approval to the browser that asked: anyone who can reach the host can start
//! a consent, but only the person looking at its page knows its code.
//!
//! The client's tokens belong to this host, not the Vault (ADR 0005 update 2026-11-16): they
//! live in the host's state directory, as SHA-256 hashes, with the clients. The Vault keeps the
//! Agent pairing, whose credential is the OAuth client's id, so revoking the pairing in the Vault
//! cuts the client off once the host sees the change: every use looks the pairing up again.
//! Access tokens last an hour; refresh tokens rotate on every use, and a refresh token used a
//! second time revokes all of its client's tokens, as OAuth 2.1 asks for public clients.
//!
//! This module holds the state and the rules, with no HTTP and no Vault: the endpoint
//! (`serve`) and the Headless host put it on the network and create the pairing.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Where the endpoint serves the protected-resource metadata (RFC 9728) for `/mcp`.
pub const PROTECTED_RESOURCE_PATH: &str = "/.well-known/oauth-protected-resource";
/// Where the endpoint serves the authorization-server metadata (RFC 8414).
pub const AUTHORIZATION_SERVER_PATH: &str = "/.well-known/oauth-authorization-server";
pub const REGISTER_PATH: &str = "/oauth/register";
pub const AUTHORIZE_PATH: &str = "/oauth/authorize";
pub const TOKEN_PATH: &str = "/oauth/token";
pub const REVOKE_PATH: &str = "/oauth/revoke";
/// The consent page, at `/oauth/consent/<page id>`.
pub const CONSENT_PATH: &str = "/oauth/consent";

/// What an access token starts with, so a leaked one is easy to recognise.
pub const ACCESS_TOKEN_PREFIX: &str = "kfo_";
/// What a refresh token starts with.
pub const REFRESH_TOKEN_PREFIX: &str = "kfor_";

/// How long a consent, and so its code, waits to be approved.
pub const CONSENT_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// How long a client has to exchange its authorization code once the consent is approved.
const CODE_LIFETIME: Duration = Duration::from_secs(5 * 60);
const ACCESS_LIFETIME: Duration = Duration::from_secs(60 * 60);
const REFRESH_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Consents waiting at once: each is a prompt on every Approver, so a stranger who can reach
/// the host can't bury the user in them.
const MAX_CONSENTS: usize = 8;
/// Wrong codes typed for one consent before it is denied.
const MAX_WRONG_CODES: u32 = 5;
/// Registered clients kept; past this, the oldest never approved are forgotten.
const MAX_CLIENTS: usize = 256;
/// A client never approved within this long is forgotten when room is needed.
const UNAPPROVED_CLIENT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
/// The letters a consent code is made of: no 0/O, 1/I/L or U/V, which read alike.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTWXYZ23456789";

/// A client's registration request (RFC 7591 section 2), as far as keefast reads it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RegistrationRequest {
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub client_uri: Option<String>,
}

/// A registered client, as the registration response (RFC 7591 section 3.2.1) shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisteredClient {
    pub client_id: String,
    pub client_id_issued_at: i64,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<&'static str>,
    pub response_types: Vec<&'static str>,
    /// Always `none`: every client is public, and proves itself with PKCE.
    pub token_endpoint_auth_method: &'static str,
}

/// Why a registration was refused: an RFC 7591 error code and a description.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{description}")]
pub struct RegistrationError {
    pub error: &'static str,
    pub description: String,
}

impl RegistrationError {
    fn redirect(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_redirect_uri",
            description: description.into(),
        }
    }
}

/// The authorization request's parameters (OAuth 2.1 section 4.1.1).
#[derive(Debug, Clone, Default)]
pub struct AuthorizeRequest {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub state: Option<String>,
    pub resource: Option<String>,
}

/// A consent begun: the page to send the browser to, and what to ask Approvers.
#[derive(Debug, Clone)]
pub struct BegunConsent {
    /// The consent page's id, in its address.
    pub page: String,
    /// The id Approvers answer it by.
    pub prompt: String,
    pub client_id: String,
    pub client_name: String,
    /// Where the browser goes once it is approved, by its host: what the user checks.
    pub redirect_host: String,
}

/// Why an authorization request didn't begin a consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizeError {
    /// The client or its redirect URI isn't known: say so on a page, and never redirect, as
    /// the redirect URI can't be trusted (OAuth 2.1 section 4.1.2.1).
    Page(String),
    /// Anything else: send the browser back to the client with this error.
    Redirect(String),
}

/// How a consent stands, for its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentState {
    /// Waiting for its code to be approved.
    Waiting {
        code: String,
        client_name: String,
        redirect_host: String,
        seconds_left: u64,
    },
    /// Approved or denied: send the browser here.
    Done { redirect: String },
    /// No such consent, or it expired.
    Gone,
}

/// A consent found by its prompt id or code, for approving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingConsent {
    pub prompt: String,
    pub client_id: String,
    pub client_name: String,
    pub redirect_host: String,
}

/// Why a consent couldn't be approved or denied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConsentError {
    #[error("no client waits with that code: it may have expired, or been approved already")]
    UnknownCode,
    #[error("that isn't the code the consent page shows")]
    WrongCode,
    #[error("the consent was answered already, or it expired")]
    Gone,
}

/// A token request's parameters (OAuth 2.1 section 3.2.2).
#[derive(Debug, Clone, Default)]
pub struct TokenRequest {
    pub grant_type: Option<String>,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub resource: Option<String>,
}

/// Tokens issued: the only time anyone sees them.
#[derive(Clone, Serialize)]
pub struct IssuedTokens {
    pub access_token: Zeroizing<String>,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub refresh_token: Zeroizing<String>,
}

impl std::fmt::Debug for IssuedTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IssuedTokens(..)")
    }
}

/// Why a token request was refused: an OAuth error code and a description.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{description}")]
pub struct TokenError {
    pub error: &'static str,
    pub description: String,
}

impl TokenError {
    fn new(error: &'static str, description: impl Into<String>) -> Self {
        Self {
            error,
            description: description.into(),
        }
    }

    fn grant(description: impl Into<String>) -> Self {
        Self::new("invalid_grant", description)
    }
}

/// Whether an OAuth client still has an Agent pairing in the Vault: `None` while the Vault is
/// locked, when that can't be told.
pub type StillPaired<'a> = &'a dyn Fn(&str) -> Option<bool>;

/// Why the host's OAuth state couldn't be read or written.
#[derive(Debug, thiserror::Error)]
pub enum OAuthStateError {
    #[error("the OAuth file {0} can't be read or written: {1}")]
    Io(String, std::io::Error),
    #[error("the OAuth file {0} is damaged: {1}")]
    Damaged(String, serde_json::Error),
}

/// The host's OAuth clients and tokens, and the consents waiting.
pub struct OAuthServer {
    /// The file clients and tokens are kept in; `None` keeps them in memory, for tests.
    path: Option<PathBuf>,
    access_lifetime: Duration,
    stored: Mutex<Stored>,
    pending: Mutex<Pending>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    clients: Vec<StoredClient>,
    #[serde(default)]
    tokens: Vec<StoredToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredClient {
    id: String,
    name: String,
    redirect_uris: Vec<String>,
    /// Unix seconds.
    created: i64,
    /// Whether a consent for it was ever approved: unapproved ones are forgotten first.
    #[serde(default)]
    approved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TokenKind {
    Access,
    Refresh,
    /// A refresh token already rotated away: presented again, it was stolen, or the client
    /// lost track; either way all of its client's tokens go.
    Rotated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredToken {
    sha256: String,
    kind: TokenKind,
    client: String,
    /// Unix seconds.
    expires: i64,
}

#[derive(Default)]
struct Pending {
    consents: Vec<Consent>,
    codes: Vec<AuthorizationCode>,
}

struct Consent {
    page: String,
    prompt: String,
    code: String,
    client_id: String,
    client_name: String,
    redirect_uri: String,
    challenge: String,
    state: Option<String>,
    resource: Option<String>,
    issuer: String,
    expires: Instant,
    wrong_codes: u32,
    /// Where the browser goes, once approved or denied.
    done: Option<String>,
}

struct AuthorizationCode {
    sha256: String,
    client_id: String,
    redirect_uri: String,
    challenge: String,
    resource: Option<String>,
    expires: Instant,
}

impl OAuthServer {
    /// The clients and tokens kept in `path`, read now; a missing file is none yet.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, OAuthStateError> {
        let path = path.into();
        let shown = path.display().to_string();
        let stored = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| OAuthStateError::Damaged(shown, error))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stored::default(),
            Err(error) => return Err(OAuthStateError::Io(shown, error)),
        };
        Ok(Self::with(Some(path), stored))
    }

    /// Clients and tokens kept only while this lives, for tests.
    pub fn in_memory() -> Self {
        Self::with(None, Stored::default())
    }

    fn with(path: Option<PathBuf>, stored: Stored) -> Self {
        Self {
            path,
            access_lifetime: ACCESS_LIFETIME,
            stored: Mutex::new(stored),
            pending: Mutex::new(Pending::default()),
        }
    }

    /// Access tokens that last `lifetime` instead of an hour, for tests.
    pub fn with_access_lifetime(mut self, lifetime: Duration) -> Self {
        self.access_lifetime = lifetime;
        self
    }

    /// Registers a public client (RFC 7591).
    pub fn register(
        &self,
        request: RegistrationRequest,
    ) -> Result<RegisteredClient, RegistrationError> {
        if request.redirect_uris.is_empty() {
            return Err(RegistrationError::redirect(
                "keefast needs at least one redirect_uri",
            ));
        }
        if request.redirect_uris.len() > 10 {
            return Err(RegistrationError::redirect(
                "keefast takes ten redirect_uris at most",
            ));
        }
        for uri in &request.redirect_uris {
            check_redirect_uri(uri)?;
        }
        let name = request
            .client_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.chars().filter(|c| !c.is_control()).take(80).collect())
            .unwrap_or_else(|| {
                redirect_host(&request.redirect_uris[0]).unwrap_or_else(|| "an MCP client".into())
            });
        let now = unix_now();
        let client = StoredClient {
            id: format!("kfc_{}", random_text(16)),
            name,
            redirect_uris: request.redirect_uris,
            created: now,
            approved: false,
        };
        self.change(|stored| {
            if stored.clients.len() >= MAX_CLIENTS {
                let stale = now - UNAPPROVED_CLIENT_LIFETIME.as_secs() as i64;
                stored
                    .clients
                    .retain(|client| client.approved || client.created > stale);
            }
            if stored.clients.len() >= MAX_CLIENTS {
                return Err(RegistrationError {
                    error: "temporarily_unavailable",
                    description: "keefast holds too many OAuth clients: try again tomorrow".into(),
                });
            }
            stored.clients.push(client.clone());
            Ok(())
        })
        .map_err(|error| RegistrationError {
            error: "server_error",
            description: error,
        })??;
        Ok(RegisteredClient {
            client_id: client.id,
            client_id_issued_at: now,
            client_name: client.name,
            redirect_uris: client.redirect_uris,
            grant_types: vec!["authorization_code", "refresh_token"],
            response_types: vec!["code"],
            token_endpoint_auth_method: "none",
        })
    }

    /// Begins a consent for an authorization request to this host, which its own address
    /// names as `issuer`, with its MCP endpoint at `resource`.
    pub fn begin(
        &self,
        request: AuthorizeRequest,
        issuer: &str,
        resource: &str,
    ) -> Result<BegunConsent, AuthorizeError> {
        let client_id = request
            .client_id
            .ok_or_else(|| AuthorizeError::Page("The client sent no client_id.".into()))?;
        let client = self
            .stored()
            .clients
            .iter()
            .find(|client| client.id == client_id)
            .cloned()
            .ok_or_else(|| {
                AuthorizeError::Page(
                    "keefast doesn't know this client: it may have been registered with another \
                     host, or forgotten. Add the server again in your MCP client."
                        .into(),
                )
            })?;
        let redirect_uri = match request.redirect_uri {
            Some(uri)
                if client
                    .redirect_uris
                    .iter()
                    .any(|known| redirect_matches(known, &uri)) =>
            {
                uri
            }
            Some(_) => {
                return Err(AuthorizeError::Page(
                    "The client asked to be sent somewhere it didn't register.".into(),
                ));
            }
            None if client.redirect_uris.len() == 1 => client.redirect_uris[0].clone(),
            None => {
                return Err(AuthorizeError::Page(
                    "The client sent no redirect_uri.".into(),
                ));
            }
        };
        let back = |error: &str, description: &str| {
            AuthorizeError::Redirect(redirect(
                &redirect_uri,
                &[("error", error), ("error_description", description)],
                request.state.as_deref(),
                issuer,
            ))
        };
        if request.response_type.as_deref() != Some("code") {
            return Err(back(
                "unsupported_response_type",
                "keefast supports only the authorization code flow",
            ));
        }
        let challenge = match (
            &request.code_challenge,
            request.code_challenge_method.as_deref(),
        ) {
            (Some(challenge), Some("S256")) if is_challenge(challenge) => challenge.clone(),
            _ => {
                return Err(back(
                    "invalid_request",
                    "keefast needs PKCE with code_challenge_method S256",
                ));
            }
        };
        if let Some(asked) = &request.resource
            && !same_resource(asked, resource)
        {
            return Err(back(
                "invalid_target",
                "keefast issues tokens only for its own MCP endpoint",
            ));
        }
        let mut pending = self.pending();
        pending.forget_expired();
        let waiting = pending
            .consents
            .iter()
            .filter(|consent| consent.done.is_none())
            .count();
        if waiting >= MAX_CONSENTS {
            return Err(back(
                "temporarily_unavailable",
                "too many consents wait to be approved on this keefast host: try again later",
            ));
        }
        let consent = Consent {
            page: random_text(24),
            prompt: random_text(16),
            code: new_code(&pending),
            client_id: client.id.clone(),
            client_name: client.name.clone(),
            redirect_uri: redirect_uri.clone(),
            challenge,
            state: request.state,
            resource: request.resource,
            issuer: issuer.to_owned(),
            expires: Instant::now() + CONSENT_LIFETIME,
            wrong_codes: 0,
            done: None,
        };
        let begun = BegunConsent {
            page: consent.page.clone(),
            prompt: consent.prompt.clone(),
            client_id: client.id,
            client_name: client.name,
            redirect_host: redirect_host(&redirect_uri).unwrap_or(redirect_uri),
        };
        pending.consents.push(consent);
        Ok(begun)
    }

    /// How the consent whose page is `page` stands.
    pub fn consent_state(&self, page: &str) -> ConsentState {
        let mut pending = self.pending();
        pending.forget_expired();
        let Some(consent) = pending.consents.iter().find(|consent| consent.page == page) else {
            return ConsentState::Gone;
        };
        match &consent.done {
            Some(redirect) => ConsentState::Done {
                redirect: redirect.clone(),
            },
            None => ConsentState::Waiting {
                code: format_code(&consent.code),
                client_name: consent.client_name.clone(),
                redirect_host: redirect_host(&consent.redirect_uri)
                    .unwrap_or_else(|| consent.redirect_uri.clone()),
                seconds_left: consent
                    .expires
                    .saturating_duration_since(Instant::now())
                    .as_secs(),
            },
        }
    }

    /// The consent waiting with `code`, as `keefast-agent pairing approve` types it.
    pub fn consent_with_code(&self, code: &str) -> Result<PendingConsent, ConsentError> {
        let code = normalise_code(code);
        let mut pending = self.pending();
        pending.forget_expired();
        pending
            .consents
            .iter()
            .find(|consent| {
                consent.done.is_none() && consent.code.as_bytes().ct_eq(code.as_bytes()).into()
            })
            .map(Consent::summary)
            .ok_or(ConsentError::UnknownCode)
    }

    /// The consent an Approver answers as `prompt`, if `code` is the one its page shows. Too
    /// many wrong codes deny it.
    pub fn consent_for_prompt(
        &self,
        prompt: &str,
        code: &str,
    ) -> Result<PendingConsent, ConsentError> {
        let code = normalise_code(code);
        let mut pending = self.pending();
        pending.forget_expired();
        let consent = pending
            .consents
            .iter_mut()
            .find(|consent| consent.done.is_none() && consent.prompt == prompt)
            .ok_or(ConsentError::Gone)?;
        if bool::from(consent.code.as_bytes().ct_eq(code.as_bytes())) {
            return Ok(consent.summary());
        }
        consent.wrong_codes += 1;
        if consent.wrong_codes >= MAX_WRONG_CODES {
            consent.deny("access_denied", "the code was typed wrong too many times");
        }
        Err(ConsentError::WrongCode)
    }

    /// Whether the consent Approvers know as `prompt` still waits for an answer.
    pub fn is_waiting(&self, prompt: &str) -> bool {
        let mut pending = self.pending();
        pending.forget_expired();
        pending
            .consents
            .iter()
            .any(|consent| consent.done.is_none() && consent.prompt == prompt)
    }

    /// Approves the consent answered as `prompt`, now that its client has an Agent pairing:
    /// its page sends the browser back to the client with an authorization code.
    pub fn approve(&self, prompt: &str) -> Result<(), ConsentError> {
        let client_id = {
            let mut pending = self.pending();
            pending.forget_expired();
            let Pending { consents, codes } = &mut *pending;
            let consent = consents
                .iter_mut()
                .find(|consent| consent.done.is_none() && consent.prompt == prompt)
                .ok_or(ConsentError::Gone)?;
            let code = random_text(32);
            codes.push(AuthorizationCode {
                sha256: hash(&code),
                client_id: consent.client_id.clone(),
                redirect_uri: consent.redirect_uri.clone(),
                challenge: consent.challenge.clone(),
                resource: consent.resource.clone(),
                expires: Instant::now() + CODE_LIFETIME,
            });
            consent.done = Some(redirect(
                &consent.redirect_uri,
                &[("code", &code)],
                consent.state.as_deref(),
                &consent.issuer,
            ));
            consent.client_id.clone()
        };
        // Kept, so the client isn't forgotten to make room for strangers.
        let _ = self.change(|stored| {
            if let Some(client) = stored.clients.iter_mut().find(|c| c.id == client_id) {
                client.approved = true;
            }
        });
        Ok(())
    }

    /// Denies the consent answered as `prompt`: its page sends the browser back to the client
    /// with `access_denied`.
    pub fn deny(&self, prompt: &str) -> Result<(), ConsentError> {
        let mut pending = self.pending();
        let consent = pending
            .consents
            .iter_mut()
            .find(|consent| consent.done.is_none() && consent.prompt == prompt)
            .ok_or(ConsentError::Gone)?;
        consent.deny("access_denied", "the user denied the request");
        Ok(())
    }

    /// Cancels the consent whose page is `page`, from the page itself; the prompt id, to
    /// dismiss it on Approvers, and where to send the browser.
    pub fn cancel(&self, page: &str) -> Option<(String, String)> {
        let mut pending = self.pending();
        let consent = pending
            .consents
            .iter_mut()
            .find(|consent| consent.page == page)?;
        if consent.done.is_none() {
            consent.deny("access_denied", "the user cancelled on the consent page");
        }
        Some((
            consent.prompt.clone(),
            consent.done.clone().unwrap_or_default(),
        ))
    }

    /// The prompt ids of consents that expired unanswered, now forgotten, so Approvers can
    /// dismiss them.
    pub fn expired_prompts(&self) -> Vec<String> {
        self.pending().forget_expired()
    }

    /// Answers a token request for the MCP endpoint `resource`. A refresh is refused once the
    /// client has no Agent pairing, if `still_paired` can tell.
    pub fn token(
        &self,
        request: TokenRequest,
        resource: &str,
        still_paired: StillPaired<'_>,
    ) -> Result<IssuedTokens, TokenError> {
        if let Some(asked) = &request.resource
            && !same_resource(asked, resource)
        {
            return Err(TokenError::new(
                "invalid_target",
                "keefast issues tokens only for its own MCP endpoint",
            ));
        }
        let client_id = request
            .client_id
            .ok_or_else(|| TokenError::new("invalid_client", "keefast needs the client_id"))?;
        match request.grant_type.as_deref() {
            Some("authorization_code") => {
                let code = request
                    .code
                    .ok_or_else(|| TokenError::new("invalid_request", "no code was sent"))?;
                let verifier = request.code_verifier.ok_or_else(|| {
                    TokenError::new("invalid_request", "keefast needs the PKCE code_verifier")
                })?;
                let issued = {
                    let mut pending = self.pending();
                    let at = pending
                        .codes
                        .iter()
                        .position(|known| {
                            known.sha256.as_bytes().ct_eq(hash(&code).as_bytes()).into()
                        })
                        .ok_or_else(|| TokenError::grant("the code is unknown, used or expired"))?;
                    // Single use, whatever happens next.
                    pending.codes.remove(at)
                };
                if issued.expires <= Instant::now() {
                    return Err(TokenError::grant("the code expired"));
                }
                if issued.client_id != client_id {
                    return Err(TokenError::grant("the code was issued to another client"));
                }
                if request
                    .redirect_uri
                    .as_ref()
                    .is_some_and(|uri| uri != &issued.redirect_uri)
                {
                    return Err(TokenError::grant(
                        "the redirect_uri isn't the one the code was issued for",
                    ));
                }
                if let (Some(asked), Some(issued)) = (&request.resource, &issued.resource)
                    && !same_resource(asked, issued)
                {
                    return Err(TokenError::new(
                        "invalid_target",
                        "the resource isn't the one the code was issued for",
                    ));
                }
                let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                if !is_verifier(&verifier)
                    || !bool::from(computed.as_bytes().ct_eq(issued.challenge.as_bytes()))
                {
                    return Err(TokenError::grant("the code_verifier doesn't match"));
                }
                self.issue(&client_id)
            }
            Some("refresh_token") => {
                let token = request.refresh_token.ok_or_else(|| {
                    TokenError::new("invalid_request", "no refresh_token was sent")
                })?;
                let presented = hash(&token);
                let now = unix_now();
                let found = self
                    .stored()
                    .tokens
                    .iter()
                    .find(|known| {
                        known.kind != TokenKind::Access
                            && bool::from(known.sha256.as_bytes().ct_eq(presented.as_bytes()))
                    })
                    .cloned();
                let Some(found) = found.filter(|found| found.client == client_id) else {
                    return Err(TokenError::grant("the refresh token is unknown or expired"));
                };
                if found.kind == TokenKind::Rotated {
                    self.forget_client_tokens(&found.client);
                    return Err(TokenError::grant(
                        "the refresh token was used already: every token of this client is \
                         revoked, so authorize again",
                    ));
                }
                if found.expires <= now {
                    return Err(TokenError::grant("the refresh token expired"));
                }
                if still_paired(&client_id) == Some(false) {
                    self.forget_client(&client_id);
                    return Err(TokenError::grant(
                        "this client's Agent pairing was revoked: authorize again",
                    ));
                }
                let _ = self.change(|stored| {
                    if let Some(known) = stored
                        .tokens
                        .iter_mut()
                        .find(|known| known.sha256 == presented)
                    {
                        known.kind = TokenKind::Rotated;
                    }
                });
                self.issue(&client_id)
            }
            Some(_) => Err(TokenError::new(
                "unsupported_grant_type",
                "keefast supports authorization_code and refresh_token",
            )),
            None => Err(TokenError::new("invalid_request", "no grant_type was sent")),
        }
    }

    fn issue(&self, client: &str) -> Result<IssuedTokens, TokenError> {
        let access = Zeroizing::new(format!("{ACCESS_TOKEN_PREFIX}{}", random_text(32)));
        let refresh = Zeroizing::new(format!("{REFRESH_TOKEN_PREFIX}{}", random_text(32)));
        let now = unix_now();
        self.change(|stored| {
            stored.tokens.retain(|token| token.expires > now);
            stored.tokens.push(StoredToken {
                sha256: hash(&access),
                kind: TokenKind::Access,
                client: client.to_owned(),
                expires: now + seconds(self.access_lifetime),
            });
            stored.tokens.push(StoredToken {
                sha256: hash(&refresh),
                kind: TokenKind::Refresh,
                client: client.to_owned(),
                expires: now + seconds(REFRESH_LIFETIME),
            });
        })
        .map_err(|error| TokenError::new("server_error", error))?;
        Ok(IssuedTokens {
            access_token: access,
            token_type: "Bearer",
            expires_in: self.access_lifetime.as_secs(),
            refresh_token: refresh,
        })
    }

    /// The OAuth client whose live access token `token` is.
    pub fn client_with_access_token(&self, token: &str) -> Option<String> {
        if !token.starts_with(ACCESS_TOKEN_PREFIX) {
            return None;
        }
        let presented = hash(token);
        let now = unix_now();
        self.stored()
            .tokens
            .iter()
            .find(|known| {
                known.kind == TokenKind::Access
                    && known.expires > now
                    && bool::from(known.sha256.as_bytes().ct_eq(presented.as_bytes()))
            })
            .map(|known| known.client.clone())
    }

    /// Revokes `token`, an access or refresh token (RFC 7009): unknown ones are ignored.
    pub fn revoke(&self, token: &str) {
        let presented = hash(token);
        let _ = self.change(|stored| stored.tokens.retain(|known| known.sha256 != presented));
    }

    /// Forgets `client` and its tokens: its Agent pairing is gone.
    pub fn forget_client(&self, client: &str) {
        let _ = self.change(|stored| {
            stored.tokens.retain(|token| token.client != client);
            stored.clients.retain(|known| known.id != client);
        });
    }

    fn forget_client_tokens(&self, client: &str) {
        let _ = self.change(|stored| stored.tokens.retain(|token| token.client != client));
    }

    /// The name `client` registered with, if it is known.
    pub fn client_name(&self, client: &str) -> Option<String> {
        self.stored()
            .clients
            .iter()
            .find(|known| known.id == client)
            .map(|known| known.name.clone())
    }

    fn stored(&self) -> MutexGuard<'_, Stored> {
        self.stored
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn pending(&self) -> MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Changes the stored clients and tokens and writes them, all or nothing.
    fn change<T>(&self, f: impl FnOnce(&mut Stored) -> T) -> Result<T, String> {
        let mut stored = self.stored();
        let mut changed = stored.clone();
        let answer = f(&mut changed);
        self.write(&changed)?;
        *stored = changed;
        Ok(answer)
    }

    /// Writes the file whole, readable only by this user, through a temporary file renamed
    /// over it, so a crash never leaves half of it.
    fn write(&self, stored: &Stored) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let io = |error: std::io::Error| format!("{} can't be written: {error}", path.display());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let json = serde_json::to_vec_pretty(stored).expect("OAuth state is plain JSON");
        let staged = path.with_extension("json.new");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&staged).map_err(io)?;
        std::io::Write::write_all(&mut file, &json).map_err(io)?;
        file.sync_all().map_err(io)?;
        std::fs::rename(&staged, path).map_err(io)
    }
}

impl Pending {
    /// Forgets expired consents and codes; the prompt ids of consents that were still waiting.
    fn forget_expired(&mut self) -> Vec<String> {
        let now = Instant::now();
        let mut expired = Vec::new();
        self.consents.retain(|consent| {
            let keep = consent.expires > now;
            if !keep && consent.done.is_none() {
                expired.push(consent.prompt.clone());
            }
            keep
        });
        self.codes.retain(|code| code.expires > now);
        expired
    }
}

impl Consent {
    fn summary(&self) -> PendingConsent {
        PendingConsent {
            prompt: self.prompt.clone(),
            client_id: self.client_id.clone(),
            client_name: self.client_name.clone(),
            redirect_host: redirect_host(&self.redirect_uri)
                .unwrap_or_else(|| self.redirect_uri.clone()),
        }
    }

    fn deny(&mut self, error: &str, description: &str) {
        self.done = Some(redirect(
            &self.redirect_uri,
            &[("error", error), ("error_description", description)],
            self.state.as_deref(),
            &self.issuer,
        ));
    }
}

/// `uri` with `params`, `state` and the issuer (RFC 9207) added to its query.
fn redirect(uri: &str, params: &[(&str, &str)], state: Option<&str>, issuer: &str) -> String {
    let (base, fragment) = match uri.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (uri, None),
    };
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        query.append_pair(key, value);
    }
    if let Some(state) = state {
        query.append_pair("state", state);
    }
    query.append_pair("iss", issuer);
    let query = query.finish();
    let separator = if base.contains('?') { '&' } else { '?' };
    let mut out = format!("{base}{separator}{query}");
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// Whether `uri` may be a redirect URI: absolute, without a fragment, and `http` only to this
/// machine (OAuth 2.1 section 2.3.1); a native app's own scheme is fine.
fn check_redirect_uri(uri: &str) -> Result<(), RegistrationError> {
    let parsed = url::Url::parse(uri)
        .map_err(|_| RegistrationError::redirect(format!("{uri} isn't an absolute URI")))?;
    if parsed.fragment().is_some() {
        return Err(RegistrationError::redirect(format!(
            "{uri} has a fragment, which a redirect URI can't"
        )));
    }
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(&parsed) => Ok(()),
        "http" => Err(RegistrationError::redirect(format!(
            "{uri} would send the authorization code in the clear: use https, or http to \
             localhost"
        ))),
        "javascript" | "data" | "file" | "vbscript" | "blob" | "about" => Err(
            RegistrationError::redirect(format!("{uri} can't be a redirect URI")),
        ),
        _ => Ok(()),
    }
}

fn is_loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Whether `asked` is the registered redirect URI `known`. A loopback one may name any port,
/// as a native app picks a free one each time (RFC 8252 section 7.3).
fn redirect_matches(known: &str, asked: &str) -> bool {
    if known == asked {
        return true;
    }
    match (url::Url::parse(known), url::Url::parse(asked)) {
        (Ok(mut known), Ok(mut asked)) if known.scheme() == "http" && is_loopback(&known) => {
            let _ = known.set_port(None);
            let _ = asked.set_port(None);
            known == asked
        }
        _ => false,
    }
}

/// `uri`'s host, or its scheme for a native app's own: what the user checks the browser goes
/// back to.
pub fn redirect_host(uri: &str) -> Option<String> {
    let parsed = url::Url::parse(uri).ok()?;
    match parsed.host_str() {
        Some(host) => Some(host.to_owned()),
        None => Some(format!("{}:", parsed.scheme())),
    }
}

/// Whether the resource a client names is this host's MCP endpoint. The scheme is left out:
/// behind a proxy that ends TLS, the host may not know which one the client used.
fn same_resource(asked: &str, ours: &str) -> bool {
    fn strip(uri: &str) -> &str {
        let uri = uri.split_once("://").map_or(uri, |(_, rest)| rest);
        uri.trim_end_matches('/')
    }
    strip(asked).eq_ignore_ascii_case(strip(ours))
}

/// A PKCE challenge: S256's base64url SHA-256, 43 characters.
fn is_challenge(challenge: &str) -> bool {
    challenge.len() == 43
        && challenge
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// A PKCE verifier: 43 to 128 unreserved characters (RFC 7636 section 4.1).
fn is_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
}

/// A new consent code, eight letters, unlike any waiting.
fn new_code(pending: &Pending) -> String {
    loop {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("the OS has randomness");
        // 29 letters: the bias of a byte modulo 29 is a few percent, harmless here.
        let code: String = bytes
            .iter()
            .map(|byte| CODE_ALPHABET[usize::from(*byte) % CODE_ALPHABET.len()] as char)
            .collect();
        if pending.consents.iter().all(|consent| consent.code != code) {
            return code;
        }
    }
}

/// A code as people read it: `ABCD-EFGH`.
pub fn format_code(code: &str) -> String {
    let (head, tail) = code.split_at(code.len() / 2);
    format!("{head}-{tail}")
}

/// A code as typed: upper case, without dashes or spaces.
fn normalise_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn random_text(bytes: usize) -> String {
    let mut random = Zeroizing::new(vec![0u8; bytes]);
    getrandom::fill(&mut random).expect("the OS has randomness");
    URL_SAFE_NO_PAD.encode(&*random)
}

fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, seconds)
}

fn seconds(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}
