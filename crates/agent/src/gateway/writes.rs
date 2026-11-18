//! A Gateway's writes (ADR 0015 "Writes stay in its own subgroup"): `POST /logins` creates a
//! login, with a password the Gateway supplies, directly in the pairing's own subgroup, and
//! `PATCH /logins/{ref}` changes the username or password of a login there.
//!
//! Only a pairing with the create right writes; one without is refused as *read only*. A login
//! the Gateway sees elsewhere, even in a subgroup it reads, is refused as *not writable*, so the
//! Gateway can tell the two apart. A change goes through the Workspace's ordinary edit, so the
//! version it replaces stays in the Entry's history for the user to see and restore. Nothing is
//! ever deleted. Every write, and every refused one, a body that can't be read included, is
//! audited with the Gateway's claim of whom it was for, never with a value or the body.

use std::sync::LazyLock;

use keefast_core::{EntryFields, EntryId};
use keefast_workspace::{EditVaultError, VaultId, Workspace};
use secrecy::SecretString;
use serde::Deserialize;
use zeroize::Zeroizing;

use super::items::{GatewayItem, ItemDetails, LOGIN_TAG, gateway_item, is_https};
use super::session::{Call, GatewayError, GatewaySession, find_item};
use crate::audit::{AuditAction, ClaimedFor};
use crate::session::own_path;
use crate::{AgentPairing, AgentSettings, VaultAccess};

/// The longest title a login may have, in characters: agentes-chat drops an item with a longer
/// one.
const MAX_TITLE: usize = 512;

/// Why a login's title is refused as too long.
static TITLE_TOO_LONG: LazyLock<String> =
    LazyLock::new(|| format!("a login's title is at most {MAX_TITLE} characters"));

/// Why a write whose body can't be read is refused, as the audit log records it.
const UNREADABLE_BODY: &str = "the request's body can't be read";

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

impl GatewaySession {
    /// Creates `request` in the pairing's own subgroup, and answers it as the list shows it.
    pub fn create_login(
        &self,
        vault: &impl VaultAccess,
        request: &NewLogin,
    ) -> Result<GatewayItem, GatewayError> {
        let call = Call {
            action: AuditAction::CreateLogin,
            claimed_for: request.claimed_for.as_ref(),
        };
        self.write(vault, &call, |workspace, id, _, pairing| {
            let fields = fields_of(request)
                .map_err(|reason| self.refuse_input(workspace, pairing, &call, None, reason))?;
            let entry = workspace
                .create_entry(id, pairing.group(), &fields)
                .map_err(|error| refused_edit(error, GatewayError::ReadOnly))?;
            self.audit(
                workspace,
                pairing,
                &call,
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
        let call = Call {
            action: AuditAction::UpdateLogin,
            claimed_for: request.claimed_for.as_ref(),
        };
        self.write(vault, &call, |workspace, id, settings, pairing| {
            let (entry, mut fields) = {
                let vault = workspace
                    .readable_vault(id)
                    .ok_or(GatewayError::Locked)?
                    .vault();
                let Some((entry, item)) = find_item(vault, settings, pairing, reference) else {
                    self.audit(workspace, pairing, &call, None, Some("not found"));
                    return Err(GatewayError::NotFound);
                };
                let audited = Some((entry.id(), entry.title()));
                // Directly in its own subgroup, which `write` found in the Agent group.
                let own = vault
                    .group(pairing.group())
                    .is_some_and(|group| group.entries().iter().any(|e| e.id() == entry.id()));
                if !own {
                    let refusal = Some("outside the pairing's own subgroup");
                    self.audit(workspace, pairing, &call, audited, refusal);
                    return Err(GatewayError::NotWritable);
                }
                if !matches!(item.details, ItemDetails::Login { .. }) {
                    return Err(self.refuse_input(
                        workspace,
                        pairing,
                        &call,
                        audited,
                        "that item is a key: only a login's username and password change here",
                    ));
                }
                (entry.id(), EntryFields::from(entry))
            };
            match password_of(request.password.as_ref()) {
                Ok(Some(password)) => fields.password = password,
                Ok(None) => {}
                Err(reason) => {
                    let audited = Some((entry, fields.title.as_str()));
                    return Err(self.refuse_input(workspace, pairing, &call, audited, reason));
                }
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
                &call,
                Some((entry, &fields.title)),
                None,
            );
            Ok(item_of(workspace, id, entry))
        })
    }

    /// Audits a write of `action` whose body can't be read as denied, with the Gateway's claim
    /// of whom it was for when the body is JSON with a `for` that reads as one, and never
    /// anything else of the body. The Gateway is answered `invalid_input` either way, so a
    /// locked Vault or a pairing gone since its token was checked goes unaudited.
    pub fn refuse_unreadable_write(
        &self,
        vault: &impl VaultAccess,
        action: AuditAction,
        body: &[u8],
    ) {
        let claimed_for = serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|mut body| body.get_mut("for").map(serde_json::Value::take))
            .and_then(|claim| serde_json::from_value::<ClaimedFor>(claim).ok());
        let call = Call {
            action,
            claimed_for: claimed_for.as_ref(),
        };
        vault.read(|workspace, id| {
            if let Ok((_, pairing)) = self.pairing_in(workspace, id) {
                self.audit(workspace, &pairing, &call, None, Some(UNREADABLE_BODY));
            }
        });
    }

    /// Runs `f` to change the Vault with the pairing, read afresh: `Locked` while the Vault is,
    /// `Unauthorized` once the pairing is gone, and `ReadOnly`, audited, without the create
    /// right or its own subgroup.
    fn write<T>(
        &self,
        vault: &impl VaultAccess,
        call: &Call<'_>,
        f: impl FnOnce(
            &mut Workspace,
            VaultId,
            &AgentSettings,
            &AgentPairing,
        ) -> Result<T, GatewayError>,
    ) -> Result<T, GatewayError> {
        vault
            .edit(|workspace, id| {
                let (settings, pairing) = self.pairing_in(workspace, id)?;
                let refusal = if !pairing.can_create() {
                    Some("the pairing may not write")
                } else if workspace
                    .readable_vault(id)
                    .and_then(|open| own_path(open.vault(), &settings, &pairing))
                    .is_none()
                {
                    Some("the pairing's own subgroup is gone")
                } else {
                    None
                };
                if let Some(refusal) = refusal {
                    self.audit(workspace, &pairing, call, None, Some(refusal));
                    return Err(GatewayError::ReadOnly);
                }
                f(workspace, id, &settings, &pairing)
            })
            .unwrap_or(Err(GatewayError::Locked))
    }

    /// Audits `call` as refused for `reason`, which says what the route takes and never what
    /// was sent, and answers it as `invalid_input`.
    fn refuse_input(
        &self,
        workspace: &Workspace,
        pairing: &AgentPairing,
        call: &Call<'_>,
        entry: Option<(EntryId, &str)>,
        reason: &'static str,
    ) -> GatewayError {
        self.audit(workspace, pairing, call, entry, Some(reason));
        GatewayError::InvalidInput(reason)
    }
}

/// The Entry `request` creates, or why it is refused.
fn fields_of(request: &NewLogin) -> Result<EntryFields, &'static str> {
    let title = request.title.trim();
    if title.is_empty() {
        return Err("a login needs a title");
    }
    if title.chars().count() > MAX_TITLE {
        return Err(TITLE_TOO_LONG.as_str());
    }
    let password = password_of(request.password.as_ref())?.ok_or("a login needs a password")?;
    let website = request
        .website
        .as_deref()
        .map(str::trim)
        .filter(|website| !website.is_empty());
    if website.is_some_and(|website| !is_https(website)) {
        return Err("a login's website must be an https:// URL");
    }
    Ok(EntryFields {
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
    })
}

/// The password a write sets: `None` when it sets none, and refused when it is empty.
fn password_of(password: Option<&Zeroizing<String>>) -> Result<Option<SecretString>, &'static str> {
    match password {
        None => Ok(None),
        Some(password) if password.is_empty() => {
            Err("a login needs a password, and it can't be empty")
        }
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
