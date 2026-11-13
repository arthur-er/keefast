//! `keefast-fill-form`, keefast's own protocol extension (ADR 0003, ADR 0010): the values of a
//! picked Entry that the Companion extension fills into a web form other than a login, such as a
//! payment form from the Entry's Card.
//!
//! What can be filled comes from the Plugins granted the `browserFill` Plugin capability
//! (ADR 0009), which the app hands the Workspace as [`FormFill`]s: custom field names and the
//! `autocomplete` token of the form field each goes into. The Workspace never runs a Plugin.
//!
//! As with `keefast-pick-entry` (ADR 0007), the user picking the Entry in the command palette is
//! the Access decision: it is remembered as allowed for the site and for the origin of the frame
//! the form is in. An Entry denied either, or in a Vault whose Default access policy is to deny,
//! can't be picked. The reply holds the values, protected ones too, and only the reply does.

use keefast_core::{AccessDecision, Entry, EntryId, Site};
use secrecy::ExposeSecret;
use serde_json::{Map, Value, json};

use super::search_entries::{hosts, keys, searchable};
use super::{BrowserReply, BrowserSession, ProtocolError, Request, browser_hash, text};
use crate::{VaultId, Workspace};

/// A kind of data Entries keep that the Companion extension may fill into web forms: a Plugin's
/// facet with the `browserFill` Plugin capability, named by its custom fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormFill {
    /// The kind of form it fills, as the protocol names it: `payment`.
    pub form: String,
    /// An Entry has it when it has a custom field of each of these names.
    pub detect: Vec<String>,
    /// The custom fields filled, by name, each with the `autocomplete` token of the form field
    /// it goes into, such as `cc-csc`.
    pub fields: Vec<(String, String)>,
}

impl FormFill {
    /// Whether the Entry has this kind of data.
    pub(super) fn is_on(&self, entry: &Entry) -> bool {
        self.detect.iter().all(|name| {
            entry
                .custom_fields()
                .iter()
                .any(|field| &field.name == name)
        })
    }

    /// The values to fill from the Entry, by token: those it has and that aren't empty.
    fn values<'e>(&self, entry: &'e Entry) -> Vec<(&str, &'e str)> {
        self.fields
            .iter()
            .filter_map(|(name, token)| {
                let field = entry.custom_fields().iter().find(|f| &f.name == name)?;
                let value = field.value.expose_secret();
                (!value.is_empty()).then_some((token.as_str(), value))
            })
            .collect()
    }
}

/// The user picking an Entry in the command palette to fill a form other than a login.
#[derive(Debug, Clone)]
pub struct FormPick {
    vault: VaultId,
    entry: EntryId,
    form: String,
    /// The hosts the Access decision is remembered for: the site's, its form's if it submits
    /// elsewhere, and the frame's the form is in if that is another.
    hosts: Vec<String>,
    /// The `id` the extension sent, echoed back.
    id: String,
    request: Request,
}

impl BrowserSession {
    /// Takes the user's pick of an Entry to fill a form of the request's kind: one with that
    /// kind of data, in a Vault the extension is paired with that may be searched, and not
    /// denied any of the hosts the values go to.
    pub(super) fn fill_form(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        let entry =
            EntryId::from_hex(text(&message, "uuid")).ok_or(ProtocolError::NoValidUuidProvided)?;
        let site = Site::parse(text(&message, "url")).ok_or(ProtocolError::NoUrlProvided)?;
        let mut hosts = hosts(&site, text(&message, "submitUrl"));
        if let Some(frame) = Site::parse(text(&message, "frameUrl")).map(|frame| frame.host())
            && !hosts.contains(&frame)
        {
            hosts.push(frame);
        }
        let form = text(&message, "form");
        let keys = keys(&message);
        let fills: Vec<&FormFill> = workspace.fills_for(form).collect();
        let vault = searchable(workspace, &keys)
            .find(|open| {
                open.vault.entries_for_browsers().iter().any(|found| {
                    found.entry.id() == entry
                        && found.entry.access_decision(&hosts) != Some(AccessDecision::Deny)
                        && fills.iter().any(|fill| fill.is_on(found.entry))
                })
            })
            .map(|open| open.id)
            .ok_or(ProtocolError::NoLoginsFound)?;
        Ok(BrowserReply::FillForm(FormPick {
            vault,
            entry,
            form: form.to_owned(),
            hosts,
            id: text(&message, "id").to_owned(),
            request,
        }))
    }

    /// The reply with the picked Entry's values for the form, by `autocomplete` token.
    fn form_reply(&self, workspace: &Workspace, pick: &FormPick) -> String {
        let Some(open) = workspace.readable_vault(pick.vault) else {
            return self.error(&pick.request, ProtocolError::DatabaseNotOpened);
        };
        let found = open.vault.entries_for_browsers();
        let Some(entry) = found
            .iter()
            .map(|found| found.entry)
            .find(|entry| entry.id() == pick.entry)
        else {
            return self.error(&pick.request, ProtocolError::NoLoginsFound);
        };
        let Some(fill) = workspace
            .fills_for(&pick.form)
            .find(|fill| fill.is_on(entry))
        else {
            return self.error(&pick.request, ProtocolError::NoLoginsFound);
        };
        let fields: Vec<Value> = fill
            .values(entry)
            .into_iter()
            .map(|(token, value)| json!({ "autocomplete": token, "value": value }))
            .collect();
        if fields.is_empty() {
            return self.error(&pick.request, ProtocolError::NoLoginsFound);
        }
        self.encrypted_reply(
            &pick.request,
            json!({
                "uuid": entry.id().to_hex(),
                "name": entry.title(),
                "form": pick.form,
                "fields": fields,
                "hash": browser_hash(workspace, pick.vault),
                "id": pick.id,
            }),
        )
    }
}

impl Workspace {
    /// Sets what the Companion extension may fill into forms other than logins: the facets of
    /// the running Plugins with the `browserFill` Plugin capability. The app calls it whenever
    /// the Plugins running change, so a Plugin disabled or removed fills nothing from then on.
    pub fn set_form_fills(&mut self, fills: Vec<FormFill>) {
        self.browser.fills = fills;
    }

    /// What the Companion extension may fill into forms other than logins, as last set.
    pub fn form_fills(&self) -> &[FormFill] {
        &self.browser.fills
    }

    /// The kinds of data that fill forms of this kind.
    pub(super) fn fills_for<'a>(&'a self, form: &'a str) -> impl Iterator<Item = &'a FormFill> {
        self.browser
            .fills
            .iter()
            .filter(move |fill| !form.is_empty() && fill.form == form)
    }

    /// Fills a form from the Entry the user picked in the command palette: remembers it as
    /// allowed for the hosts the values go to, and returns the reply with its values.
    pub(super) fn give_form(&mut self, session: &BrowserSession, pick: &FormPick) -> String {
        let edited = self.edit(pick.vault, |vault, now| {
            vault.remember_access(pick.entry, &pick.hosts, AccessDecision::Allow, now)
        });
        if edited.is_err() {
            return session.error(&pick.request, ProtocolError::NoLoginsFound);
        }
        session.form_reply(self, pick)
    }
}
