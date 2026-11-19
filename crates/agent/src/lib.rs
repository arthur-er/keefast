//! Agents (ADR 0012): AI agents that reach a Vault's secrets through keefast's MCP server or
//! the `keefast-agent` command line, and the Headless host that serves a Vault to them.
//!
//! This crate sits on top of `keefast-workspace`, which stays the only owner of decrypted
//! Vaults: everything here reaches a Vault through a [`keefast_workspace::Workspace`]. It never
//! links Tauri or WebKit, so `keefast-agent` runs on a server without a display.
//!
//! # Modules
//!
//! - `session` (`AgentSession`) and `session/writes`: one Agent pairing's requests against a
//!   Workspace: listing, using and creating secrets.
//! - `settings`: the Agent settings a Vault keeps: Agent group, policy, pairings, decisions.
//! - `approver`: who is asked when a use needs the user's say; `approver_client`: the desktop's
//!   side of a Headless host's prompt stream.
//! - `audit`: the audit log of every list and use.
//! - `gateway` and `gateway/{items,session,writes}`: Gateway pairings (ADR 0015) and their
//!   rules; `gateway_endpoint`: their `/gateway/v1/` HTTP side.
//! - `host`: the Headless host (`keefast-agent serve`); `serve`: its MCP endpoint at `/mcp`;
//!   `listen`: where it listens, with what TLS; `trust`: which certificates clients trust.
//! - `oauth`: the OAuth authorization server for MCP clients; `oauth_endpoint`: its HTTP side.
//! - `admin`: the admin socket `keefast-agent unlock`, `lock` and `status` use.
//! - `local`: how `keefast-agent mcp`, `run` and `store` reach the desktop's in-app host.
//! - `http` and `hosts`: `http_request`, and the hosts an Entry allows it to reach.
//! - `run` and `programs`: `keefast-agent run`, and programs as decisions name them.
//! - `store`: `keefast-agent store`.
//! - `installed`: where the `keefast-agent` command is for Agents to run.
//! - `main.rs`: the `keefast-agent` command line.

pub mod admin;
pub mod approver;
pub mod approver_client;
pub mod audit;
pub mod gateway;
#[cfg(feature = "serve")]
pub mod gateway_endpoint;
pub mod host;
pub mod hosts;
pub mod http;
pub mod installed;
pub mod listen;
pub mod local;
pub mod oauth;
#[cfg(feature = "serve")]
mod oauth_endpoint;
pub mod programs;
pub mod run;
#[cfg(feature = "serve")]
pub mod serve;
mod session;
mod settings;
pub mod store;
pub mod trust;

pub use approver::{
    AgentPrompt, Approval, Approver, ApproverCredentials, ApproverHub, ConsentAnswer,
    ConsentPrompt, NoApprover,
};
pub use audit::{
    AuditAction, AuditEntry, AuditLog, AuditOutcome, AuditRecord, AuditSink, ClaimedFor,
};
pub use session::{
    AgentError, AgentReply, AgentRequest, AgentSession, CreateEntry, SecretListing,
    SecretReference, SecretValue, ServesAgents, StoreSecret, UseSecrets, VaultAccess, WrittenEntry,
    pairings_reading,
};
pub use settings::{
    AgentAccessDecision, AgentCredential, AgentDecision, AgentPairing, AgentPairingId,
    AgentSettings, AgentSettingsError, AgentSettingsExt, AgentUse, CreatedAgentPairing,
    DefaultAgentPolicy, NewAgentPairing, PairingKind, TOKEN_PREFIX,
};
