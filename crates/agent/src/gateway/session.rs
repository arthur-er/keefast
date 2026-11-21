//! `GatewaySession`: one Gateway pairing's requests, handled against a Workspace, as
//! [`AgentSession`](crate::AgentSession) handles an Agent pairing's.
//!
//! A Gateway sees what an Agent pairing would (the Entries in its own subgroup and the
//! subgroups it reads, never their notes), through the same [`PairingScope`], and lists
//! them as the Entry mapping says. Each request reads the pairing afresh from the Vault, so a
//! revoked one stops at once, and a pairing of the other kind is refused as unknown. Every call
//! but `whoami` goes to the audit log, never with a value.

use std::sync::Arc;

use keefast_core::EntryId;
use keefast_workspace::{VaultId, VaultLocation, Workspace};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::items::{GatewayItem, gateway_item};
use crate::audit::{AuditAction, AuditEntry, AuditOutcome, AuditRecord, AuditSink, ClaimedFor};
use crate::scope::{PairingScope, ScopeError};
use crate::{AgentPairing, AgentPairingId, PairingKind, VaultAccess};

/// The longest Vault name `whoami` answers with, in characters.
const MAX_VAULT_NAME: usize = 512;

/// Why a Gateway's request was refused. Each has a code the Gateway tells apart, and a message
/// of keefast's own, which never holds a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    #[error(
        "keefast needs a Gateway pairing's token: Authorization: Bearer <token>, from \
         `keefast-agent pairing add --gateway`"
    )]
    Unauthorized,
    #[error(
        "keefast is locked: ask its operator to unlock it with `keefast-agent unlock`, then try \
         again"
    )]
    Locked,
    #[error(
        "keefast has no such item for this pairing: its Entry is gone, or no longer in the \
         pairing's subgroups"
    )]
    NotFound,
    /// A body that isn't what the route takes. The message says what it takes, never what was
    /// sent, which may hold a value.
    #[error("keefast can't read that request: {0}")]
    InvalidInput(&'static str),
    /// A write by a pairing without the right to write, or whose own subgroup is gone.
    #[error(
        "this Gateway pairing may only read: give it the right to write, with `keefast-agent \
         pairing add --gateway <name> --write` or in the desktop app, to create and change logins"
    )]
    ReadOnly,
    /// A change to an Entry the pairing sees outside its own subgroup.
    #[error(
        "keefast doesn't let this pairing change that Entry: a Gateway changes only the logins \
         in its own subgroup"
    )]
    NotWritable,
}

impl From<ScopeError> for GatewayError {
    fn from(error: ScopeError) -> Self {
        match error {
            ScopeError::Locked => Self::Locked,
            ScopeError::NotPaired => Self::Unauthorized,
        }
    }
}

impl GatewayError {
    /// Its `code` in the API's errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Locked => "locked",
            Self::NotFound => "not_found",
            Self::InvalidInput(_) => "invalid_input",
            Self::ReadOnly => "read_only",
            Self::NotWritable => "not_writable",
        }
    }
}

/// Who a Gateway is, as `GET /whoami` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Whoami {
    /// The Vault's name, or its file's when it has none.
    pub vault: String,
    pub pairing: String,
    /// The subgroups of the Agent group it reads, by name: its own first.
    pub subgroups: Vec<String>,
    /// Whether it may create and change logins in its own subgroup.
    pub can_write: bool,
}

/// The items a Gateway lists, and a tag that changes whenever they do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub items: Vec<GatewayItem>,
    /// An `ETag`, quoted: the same for the same items, so a Gateway polling a Vault that hasn't
    /// changed hears "not modified".
    #[serde(skip)]
    pub etag: String,
}

/// A resolve, as `POST /items/{ref}/resolve` takes it: `{ field, for? }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Resolve {
    pub field: Field,
    /// Whom the Gateway says the value is for: its claim, which keefast logs and can't verify.
    #[serde(default, rename = "for")]
    pub claimed_for: Option<ClaimedFor>,
}

/// The field a resolve asks for. Both read the Entry's password: a login's is its password,
/// and a key's is its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Password,
    Key,
}

/// What a call is audited as: its action and the Gateway's claim of whom it was for.
pub(super) struct Call<'a> {
    pub action: AuditAction,
    pub claimed_for: Option<&'a ClaimedFor>,
}

/// One Gateway pairing's requests.
#[derive(Clone)]
pub struct GatewaySession {
    pairing: AgentPairingId,
    audit: Arc<dyn AuditSink>,
}

impl GatewaySession {
    /// A session for the Gateway pairing `pairing`, recording what it does in `audit`.
    pub fn new(pairing: AgentPairingId, audit: Arc<dyn AuditSink>) -> Self {
        Self { pairing, audit }
    }

    pub fn pairing(&self) -> &AgentPairingId {
        &self.pairing
    }

    /// Audits `call` on `entry`: allowed without a `refusal`, denied with one, and never with a
    /// value.
    pub(super) fn audit(
        &self,
        workspace: &Workspace,
        pairing: &AgentPairing,
        call: &Call<'_>,
        entry: Option<(EntryId, &str)>,
        refusal: Option<&str>,
    ) {
        let outcome = match refusal {
            Some(_) => AuditOutcome::Denied,
            None => AuditOutcome::Allowed,
        };
        let mut record = AuditRecord::of(
            workspace.now().unix_seconds(),
            pairing,
            call.action,
            outcome,
        );
        record.entry = entry.map(|(id, title)| AuditEntry {
            uuid: id.to_hex(),
            title: title.to_owned(),
        });
        record.reason = refusal.map(str::to_owned);
        record.claimed_for = call.claimed_for.and_then(ClaimedFor::capped);
        self.audit.record(record);
    }

    pub fn whoami(&self, vault: &impl VaultAccess) -> Result<Whoami, GatewayError> {
        self.read(vault, |workspace, id, scope| {
            let open = workspace.readable_vault(id).ok_or(GatewayError::Locked)?;
            let pairing = scope.pairing();
            let groups = std::iter::once(pairing.group()).chain(pairing.readable().iter().copied());
            Ok(Whoami {
                vault: vault_name(&open.vault().name(), open.location()),
                pairing: pairing.name().to_owned(),
                subgroups: groups
                    .filter_map(|group| open.vault().group(group))
                    .map(|group| group.name().to_owned())
                    .collect(),
                can_write: pairing.can_create(),
            })
        })
    }

    /// The Entries the pairing reads that map to items, logged as one `list`.
    pub fn list(&self, vault: &impl VaultAccess) -> Result<Listing, GatewayError> {
        self.read(vault, |workspace, _, scope| {
            let pairing = scope.pairing();
            let items: Vec<GatewayItem> = scope
                .entries()
                .iter()
                .filter_map(|readable| gateway_item(readable.entry))
                .collect();
            let list = Call {
                action: AuditAction::List,
                claimed_for: None,
            };
            self.audit(workspace, pairing, &list, None, None);
            let json = serde_json::to_vec(&items).expect("items are plain JSON");
            let digest = Sha256::digest(&json);
            let etag: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
            Ok(Listing {
                items,
                etag: format!("\"{etag}\""),
            })
        })
    }

    /// The value of the item `reference`: the Entry's password, whichever field is asked for.
    /// Being in the pairing's subgroups is the approval: nothing is asked, and neither Agent
    /// access decisions nor the Default agent policy apply.
    pub fn resolve(
        &self,
        vault: &impl VaultAccess,
        reference: &str,
        request: &Resolve,
    ) -> Result<SecretString, GatewayError> {
        self.read(vault, |workspace, _, scope| {
            let pairing = scope.pairing();
            let call = Call {
                action: AuditAction::Resolve,
                claimed_for: request.claimed_for.as_ref(),
            };
            let entry = EntryId::from_hex(reference)
                .and_then(|id| scope.entry(id))
                .map(|readable| readable.entry)
                .filter(|entry| gateway_item(entry).is_some());
            let Some(entry) = entry else {
                self.audit(workspace, pairing, &call, None, Some("not found"));
                return Err(GatewayError::NotFound);
            };
            self.audit(
                workspace,
                pairing,
                &call,
                Some((entry.id(), entry.title())),
                None,
            );
            Ok(SecretString::from(
                entry.password().expose_secret().to_owned(),
            ))
        })
    }

    /// Runs `f` with the pairing's scope, read afresh: `Locked` while the Vault is, and
    /// `Unauthorized` once the pairing is gone or if it isn't a Gateway pairing.
    fn read<T>(
        &self,
        vault: &impl VaultAccess,
        f: impl FnOnce(&Workspace, VaultId, &PairingScope<'_>) -> Result<T, GatewayError>,
    ) -> Result<T, GatewayError> {
        vault
            .read(|workspace, id| {
                let scope = PairingScope::open(workspace, id, &self.pairing, PairingKind::Gateway)?;
                f(workspace, id, &scope)
            })
            .unwrap_or(Err(GatewayError::Locked))
    }
}

/// The name `whoami` gives the Vault: its own, or else its file's, never empty.
fn vault_name(name: &str, location: &VaultLocation) -> String {
    let name = name.trim();
    let name = if name.is_empty() {
        let path = match location {
            VaultLocation::Local(path) => path.clone(),
            VaultLocation::Dropbox(path) => std::path::PathBuf::from(path),
        };
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| "keefast".to_owned())
    } else {
        name.to_owned()
    };
    name.chars().take(MAX_VAULT_NAME).collect()
}
