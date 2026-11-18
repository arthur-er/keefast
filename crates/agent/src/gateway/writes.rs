//! A Gateway's writes (ADR 0015 "Writes stay in its own subgroup"): `POST /logins` creates a
//! login, with a password the Gateway supplies, directly in the pairing's own subgroup, and
//! `PATCH /logins/{ref}` changes the username or password of a login there.
//!
//! Only a pairing with the create right writes; one without is refused as *read only*. A login
//! the Gateway sees elsewhere, even in a subgroup it reads, is refused as *not writable*, so the
//! Gateway can tell the two apart. A change goes through the Workspace's ordinary edit, so the
//! version it replaces stays in the Entry's history for the user to see and restore. Nothing is
//! ever deleted. Every write, and every refused one, is audited with the Gateway's claim of whom
//! it was for, never with a value.

use keefast_core::{EntryFields, EntryId};
use keefast_workspace::{EditVaultError, VaultId, Workspace};
use secrecy::SecretString;
use serde::Deserialize;
use zeroize::Zeroizing;

use super::items::{GatewayItem, ItemDetails, LOGIN_TAG, gateway_item, is_https};
use super::session::{GatewayError, GatewaySession};
use crate::audit::{AuditAction, AuditEntry, AuditOutcome, AuditRecord, ClaimedFor};
use crate::session::{own_path, readable_entries};
use crate::{AgentPairing, AgentSettings, PairingKind, VaultAccess};

/// The longest title a login may have, in characters: agentes-chat drops an item with a longer
/// one.
const MAX_TITLE: usize = 512;

/// A login to create, as `POST /logins` takes it: `{ title, username?, password, website?,
/// notes?, for? }`. Its `Debug` hides the password.
#[derive(Clone, Deserialize)]
pub struct NewLogin {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    password: Option<Zeroizing<String>>,
    /// An `https` URL; a login without one is tagged [`LOGIN_TAG`], so it is still listed.
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Whom the Gateway says the login is for: its claim, which keefast logs and can't verify.
    #[serde(default, rename = "for")]
    pub claimed_for: Option<ClaimedFor>,
}

/// A change to a login, as `PATCH /logins/{ref}` takes it: `{ username?, password?, for? }`.
/// Its `Debug` hides the password.
#[derive(Clone, Deserialize)]
pub struct LoginChange {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    password: Option<Zeroizing<String>>,
    #[serde(default, rename = "for")]
    pub claimed_for: Option<ClaimedFor>,
}

impl std::fmt::Debug for NewLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewLogin")
            .field("title", &self.title)
            .field("username", &self.username)
            .field("website", &self.website)
            .field("claimed_for", &self.claimed_for)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for LoginChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginChange")
            .field("username", &self.username)
            .field("claimed_for", &self.claimed_for)
            .finish_non_exhaustive()
    }
}

/// What a write is audited as: its action and the Gateway's claim.
struct Write<'a> {
    action: AuditAction,
    claimed_for: Option<&'a ClaimedFor>,
}

impl GatewaySession {
    /// Creates `request` in the pairing's own subgroup, and answers it as the list shows it.
    pub fn create_login(
        &self,
        vault: &impl VaultAccess,
        request: &NewLogin,
    ) -> Result<GatewayItem, GatewayError> {
        let write = Write {
            action: AuditAction::CreateLogin,
            claimed_for: request.claimed_for.as_ref(),
        };
        self.write(vault, &write, |workspace, id, pairing| {
            let title = request.title.trim();
            if title.is_empty() {
                return Err(GatewayError::InvalidInput("a login needs a title"));
            }
            if title.chars().count() > MAX_TITLE {
                return Err(GatewayError::InvalidInput(
                    "a login's title is at most 512 characters",
                ));
            }
            let password = password_of(request.password.as_ref())?
                .ok_or(GatewayError::InvalidInput("a login needs a password"))?;
            let website = request
                .website
                .as_deref()
                .map(str::trim)
                .filter(|website| !website.is_empty());
            if website.is_some_and(|website| !is_https(website)) {
                return Err(GatewayError::InvalidInput(
                    "a login's website must be an https:// URL",
                ));
            }
            let fields = EntryFields {
                title: title.to_owned(),
                username: request.username.clone().unwrap_or_default(),
                password,
                url: website.unwrap_or_default().to_owned(),
                notes: request.notes.clone().unwrap_or_default(),
                tags: match website {
                    Some(_) => Vec::new(),
                    None => vec![LOGIN_TAG.to_owned()],
                },
                ..EntryFields::default()
            };
            let entry = workspace
                .create_entry(id, pairing.group(), &fields)
                .map_err(|error| refused_edit(error, GatewayError::ReadOnly))?;
            self.audit(
                workspace,
                pairing,
                &write,
                Some((entry, &fields.title)),
                None,
            );
            Ok(item_of(workspace, id, entry))
        })
    }

    /// Changes the username or password of the login `reference` in the pairing's own
    /// subgroup, keeping the version it replaces in the Entry's history, and answers it as the
    /// list shows it.
    pub fn update_login(
        &self,
        vault: &impl VaultAccess,
        reference: &str,
        request: &LoginChange,
    ) -> Result<GatewayItem, GatewayError> {
        let write = Write {
            action: AuditAction::UpdateLogin,
            claimed_for: request.claimed_for.as_ref(),
        };
        self.write(vault, &write, |workspace, id, pairing| {
            let (entry, mut fields) = {
                let vault = workspace
                    .readable_vault(id)
                    .ok_or(GatewayError::Locked)?
                    .vault();
                let settings = AgentSettings::of(vault);
                let found = EntryId::from_hex(reference).and_then(|wanted| {
                    readable_entries(vault, &settings, pairing)
                        .into_iter()
                        .map(|(entry, _)| entry)
                        .find(|entry| entry.id() == wanted)
                });
                let Some((entry, item)) =
                    found.and_then(|entry| Some((entry, gateway_item(entry)?)))
                else {
                    self.audit(workspace, pairing, &write, None, Some("not found"));
                    return Err(GatewayError::NotFound);
                };
                // Directly in its own subgroup, which `write` found in the Agent group.
                let own = vault
                    .group(pairing.group())
                    .is_some_and(|group| group.entries().iter().any(|e| e.id() == entry.id()));
                if !own {
                    self.audit(
                        workspace,
                        pairing,
                        &write,
                        Some((entry.id(), entry.title())),
                        Some("outside the pairing's own subgroup"),
                    );
                    return Err(GatewayError::NotWritable);
                }
                if !matches!(item.details, ItemDetails::Login { .. }) {
                    return Err(GatewayError::InvalidInput(
                        "that item is a key: only a login's username and password change here",
                    ));
                }
                (entry.id(), EntryFields::from(entry))
            };
            if let Some(password) = password_of(request.password.as_ref())? {
                fields.password = password;
            }
            if let Some(username) = &request.username {
                fields.username = username.clone();
            }
            workspace
                .update_entry(id, entry, &fields)
                .map_err(|error| refused_edit(error, GatewayError::NotWritable))?;
            self.audit(
                workspace,
                pairing,
                &write,
                Some((entry, &fields.title)),
                None,
            );
            Ok(item_of(workspace, id, entry))
        })
    }

    /// Runs `f` to change the Vault with the pairing, read afresh: `Locked` while the Vault is,
    /// `Unauthorized` once the pairing is gone, and `ReadOnly`, audited, without the create
    /// right or its own subgroup.
    fn write<T>(
        &self,
        vault: &impl VaultAccess,
        write: &Write<'_>,
        f: impl FnOnce(&mut Workspace, VaultId, &AgentPairing) -> Result<T, GatewayError>,
    ) -> Result<T, GatewayError> {
        vault
            .edit(|workspace, id| {
                let open = workspace.readable_vault(id).ok_or(GatewayError::Locked)?;
                let settings = AgentSettings::of(open.vault());
                let pairing = settings
                    .pairing_of_kind(self.pairing(), PairingKind::Gateway)
                    .ok_or(GatewayError::Unauthorized)?
                    .clone();
                let refusal = if !pairing.can_create() {
                    Some("the pairing may not write")
                } else if own_path(open.vault(), &settings, &pairing).is_none() {
                    Some("the pairing's own subgroup is gone")
                } else {
                    None
                };
                if let Some(refusal) = refusal {
                    self.audit(workspace, &pairing, write, None, Some(refusal));
                    return Err(GatewayError::ReadOnly);
                }
                f(workspace, id, &pairing)
            })
            .unwrap_or(Err(GatewayError::Locked))
    }

    /// Audits a write to `entry`: allowed without a `refusal`, denied with one.
    fn audit(
        &self,
        workspace: &Workspace,
        pairing: &AgentPairing,
        write: &Write<'_>,
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
            write.action,
            outcome,
        );
        record.entry = entry.map(|(id, title)| AuditEntry {
            uuid: id.to_hex(),
            title: title.to_owned(),
        });
        record.reason = refusal.map(str::to_owned);
        record.claimed_for = write.claimed_for.and_then(ClaimedFor::capped);
        self.audit_sink().record(record);
    }
}

/// The password a write sets: `None` when it sets none, and refused when it is empty.
fn password_of(password: Option<&Zeroizing<String>>) -> Result<Option<SecretString>, GatewayError> {
    match password {
        None => Ok(None),
        Some(password) if password.is_empty() => Err(GatewayError::InvalidInput(
            "a login needs a password, and it can't be empty",
        )),
        Some(password) => Ok(Some(SecretString::from(password.as_str().to_owned()))),
    }
}

/// An edit the Workspace refused: `Locked` if the Vault closed, or else `otherwise`.
fn refused_edit(error: EditVaultError, otherwise: GatewayError) -> GatewayError {
    match error {
        EditVaultError::VaultNotOpen => GatewayError::Locked,
        _ => otherwise,
    }
}

/// The login `entry`, just written, as the list shows it.
fn item_of(workspace: &Workspace, id: VaultId, entry: EntryId) -> GatewayItem {
    workspace
        .readable_vault(id)
        .and_then(|open| open.vault().entry(entry))
        .and_then(gateway_item)
        .expect("a login just written has an https website or the login tag, and a password")
}
