//! `GatewaySession`: one Gateway pairing's requests, handled against a Workspace, as
//! [`AgentSession`](crate::AgentSession) handles an Agent pairing's.
//!
//! A Gateway sees what an Agent pairing would (the Entries in its own subgroup and the
//! subgroups it reads, never their notes), through the same [`readable_entries`], and lists
//! them as the Entry mapping says. Each request reads the pairing afresh from the Vault, so a
//! revoked one stops at once, and a pairing of the other kind is refused as unknown. Every call
//! but `whoami` goes to the audit log, never with a value.

use std::sync::Arc;

use keefast_workspace::{VaultId, VaultLocation, Workspace};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::items::{GatewayItem, gateway_item};
use crate::audit::{AuditAction, AuditOutcome, AuditRecord, AuditSink};
use crate::session::readable_entries;
use crate::{AgentPairing, AgentPairingId, AgentSettings, PairingKind, VaultAccess};

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
}

impl GatewayError {
    /// Its `code` in the API's errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Locked => "locked",
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

    pub fn whoami(&self, vault: &impl VaultAccess) -> Result<Whoami, GatewayError> {
        self.read(vault, |workspace, id, _, pairing| {
            let open = workspace.readable_vault(id).ok_or(GatewayError::Locked)?;
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
        self.read(vault, |workspace, id, settings, pairing| {
            let vault = workspace
                .readable_vault(id)
                .ok_or(GatewayError::Locked)?
                .vault();
            let items: Vec<GatewayItem> = readable_entries(vault, settings, pairing)
                .into_iter()
                .filter_map(|(entry, _)| gateway_item(entry))
                .collect();
            self.audit.record(AuditRecord::of(
                workspace.now().unix_seconds(),
                pairing,
                AuditAction::List,
                AuditOutcome::Allowed,
            ));
            let json = serde_json::to_vec(&items).expect("items are plain JSON");
            let digest = Sha256::digest(&json);
            let etag: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
            Ok(Listing {
                items,
                etag: format!("\"{etag}\""),
            })
        })
    }

    /// Runs `f` with the Vault and the pairing, read afresh: `Locked` while the Vault is, and
    /// `Unauthorized` once the pairing is gone or if it isn't a Gateway pairing.
    fn read<T>(
        &self,
        vault: &impl VaultAccess,
        f: impl FnOnce(&Workspace, VaultId, &AgentSettings, &AgentPairing) -> Result<T, GatewayError>,
    ) -> Result<T, GatewayError> {
        vault
            .read(|workspace, id| {
                let open = workspace.readable_vault(id).ok_or(GatewayError::Locked)?;
                let settings = AgentSettings::of(open.vault());
                let pairing = settings
                    .pairing_of_kind(&self.pairing, PairingKind::Gateway)
                    .ok_or(GatewayError::Unauthorized)?;
                f(workspace, id, &settings, pairing)
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
