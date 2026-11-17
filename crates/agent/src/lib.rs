//! Agents (ADR 0012): AI agents that reach a Vault's secrets through keefast's MCP server or
//! the `keefast-agent` command line, and the Headless host that serves a Vault to them.
//!
//! This crate sits on top of `keefast-workspace`, which stays the only owner of decrypted
//! Vaults: everything here reaches a Vault through a [`keefast_workspace::Workspace`]. It never
//! links Tauri or WebKit, so `keefast-agent` runs on a server without a display.

pub mod admin;
pub mod audit;
pub mod host;
pub mod listen;
pub mod programs;
pub mod run;
#[cfg(feature = "serve")]
pub mod serve;
mod session;
mod settings;

pub use audit::{AuditEntry, AuditLog, AuditOutcome, AuditRecord, AuditSink};
pub use session::{
    AgentError, AgentReply, AgentRequest, AgentSession, SecretListing, SecretReference,
    SecretValue, UseSecrets,
};
pub use settings::{
    AgentAccessDecision, AgentCredential, AgentDecision, AgentPairing, AgentPairingId,
    AgentSettings, AgentSettingsError, AgentSettingsExt, AgentUse, CreatedAgentPairing,
    DefaultAgentPolicy, NewAgentPairing, TOKEN_PREFIX,
};
