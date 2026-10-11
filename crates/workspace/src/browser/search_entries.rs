//! keefast's own protocol extensions (ADR 0003, ADR 0007): the Companion extension's command
//! palette searching the paired Vaults for an Entry when none is for the site, and filling the
//! Entry the user picks there.
//!
//! `keefast-search-entries` answers with what the palette lists of each Entry, never its
//! password. `keefast-pick-entry` gives the picked Entry to the site: the user picking it is
//! the Access decision, so it is remembered as allowed for the site, and with `addUrl` the site
//! is added to the Entry's URLs, so it is offered there from now on.
//!
//! With a `form`, such as `payment`, the search finds only Entries with data that fills that
//! kind of form, such as a Card (`keefast-fill-form`, ADR 0010), and an empty query lists them
//! all, up to the palette's few: a Vault holds few of them, and the palette lists them by name.

use keefast_core::{
    AccessDecision, BrowserEntry, DefaultAccessPolicy, EntryId, EntrySearch, EntrySort, Site,
    SiteEntry,
};
use serde_json::{Map, Value, json};

use super::{BrowserReply, BrowserSession, Candidate, ProtocolError, Request, browser_hash, text};
use crate::{OpenVault, VaultId, Workspace};

/// The most Entries a search answers with: the palette shows the best few.
const MAX_RESULTS: usize = 20;

/// The user picking an Entry for a site in the command palette, to be given to the site.
#[derive(Debug, Clone)]
pub struct PickRequest {
    vault: VaultId,
    entry: EntryId,
    site: Site,
    /// The hosts the Access decision is remembered for: the site's, and its form's if it
    /// submits elsewhere.
    hosts: Vec<String>,
    /// Whether to add the site to the Entry's URLs.
    add_url: bool,
    /// The `id` the extension sent, echoed back.
    id: String,
    request: Request,
}

impl BrowserSession {
    /// Answers with the Entries matching the request's query in the open Vaults the extension
    /// is paired with, best match first. A Vault whose Default access policy is to deny is not
    /// searched, nor are Entries denied the site. An empty query finds nothing: the palette
    /// never lists a whole Vault. With a `form`, only Entries that fill it are searched, and an
    /// empty query lists them.
    pub(super) fn search_entries(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        if text(&message, "action") != "keefast-search-entries" {
            return Err(ProtocolError::IncorrectAction);
        }
        let site = Site::parse(text(&message, "url")).ok_or(ProtocolError::NoUrlProvided)?;
        let hosts = hosts(&site, text(&message, "submitUrl"));
        let keys = keys(&message);
        let search = EntrySearch::new(text(&message, "query"));
        let form = text(&message, "form");
        let fills: Vec<_> = workspace.fills_for(form).collect();
        let found: Vec<(VaultId, BrowserEntry)> = if search.is_empty() && form.is_empty() {
            Vec::new()
        } else {
            let offered = searchable(workspace, &keys).flat_map(|open| {
                let (hosts, fills) = (&hosts, &fills);
                open.vault
                    .entries_for_browsers()
                    .into_iter()
                    .filter(move |found| {
                        found.entry.access_decision(hosts) != Some(AccessDecision::Deny)
                            && (form.is_empty() || fills.iter().any(|fill| fill.is_on(found.entry)))
                    })
                    .map(move |found| (open.id, found))
            });
            search.rank(offered, |(_, found)| found.entry, EntrySort::Title)
        };
        let entries: Vec<Value> = found
            .iter()
            .take(MAX_RESULTS)
            .map(
                |(
                    _,
                    BrowserEntry {
                        entry, group_path, ..
                    },
                )| {
                    json!({
                        "uuid": entry.id().to_hex(),
                        "name": entry.title(),
                        "login": entry.username(),
                        "url": entry.url(),
                        "group": group_path.join(" / "),
                        "hasTotp": matches!(entry.totp(), Some(Ok(_))),
                    })
                },
            )
            .collect();
        Ok(BrowserReply::Send(self.encrypted_reply(
            &request,
            json!({ "count": entries.len(), "entries": entries }),
        )))
    }

    /// Takes the user's pick of an Entry for a site, for the Workspace to give it: an Entry
    /// the search could have found, in a Vault the extension is paired with.
    pub(super) fn pick_entry(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        if text(&message, "action") != "keefast-pick-entry" {
            return Err(ProtocolError::IncorrectAction);
        }
        let entry =
            EntryId::from_hex(text(&message, "uuid")).ok_or(ProtocolError::NoValidUuidProvided)?;
        let site = Site::parse(text(&message, "url")).ok_or(ProtocolError::NoUrlProvided)?;
        let hosts = hosts(&site, text(&message, "submitUrl"));
        let keys = keys(&message);
        let vault = searchable(workspace, &keys)
            .find(|open| {
                open.vault.entries_for_browsers().iter().any(|found| {
                    found.entry.id() == entry
                        && found.entry.access_decision(&hosts) != Some(AccessDecision::Deny)
                })
            })
            .map(|open| open.id)
            .ok_or(ProtocolError::NoLoginsFound)?;
        Ok(BrowserReply::Pick(PickRequest {
            vault,
            entry,
            site,
            hosts,
            add_url: message.get("addUrl") == Some(&Value::Bool(true)),
            id: text(&message, "id").to_owned(),
            request,
        }))
    }

    /// The reply giving the picked Entry to the site, as `get-logins` gives Entries, with its
    /// current TOTP code if it has TOTP.
    fn picked_reply(&self, workspace: &Workspace, pick: &PickRequest) -> String {
        let Some(open) = workspace.readable_vault(pick.vault) else {
            return self.error(&pick.request, ProtocolError::DatabaseNotOpened);
        };
        let found = open.vault.entries_for_browsers();
        let Some(found) = found.iter().find(|found| found.entry.id() == pick.entry) else {
            return self.error(&pick.request, ProtocolError::NoLoginsFound);
        };
        let candidate = Candidate {
            vault: pick.vault,
            found: SiteEntry {
                entry: found.entry,
                group: found.group,
            },
            decision: Some(AccessDecision::Allow),
        };
        self.encrypted_reply(
            &pick.request,
            json!({
                "count": 1,
                "entries": [candidate.to_protocol(workspace.now())],
                "hash": browser_hash(workspace, pick.vault),
                "id": pick.id,
            }),
        )
    }
}

impl Workspace {
    /// Gives the Entry the user picked in the command palette to the site: remembers it as
    /// allowed there, adds the site to its URLs if asked, and returns the reply.
    pub(super) fn give_picked_entry(
        &mut self,
        session: &BrowserSession,
        pick: &PickRequest,
    ) -> String {
        let edited = self.edit(pick.vault, |vault, now| {
            vault.remember_access(pick.entry, &pick.hosts, AccessDecision::Allow, now)?;
            if pick.add_url {
                vault.add_site_url(pick.entry, &pick.site, now)?;
            }
            Ok(())
        });
        if edited.is_err() {
            return session.error(&pick.request, ProtocolError::NoLoginsFound);
        }
        session.picked_reply(self, pick)
    }
}

/// The open Vaults the palette may search: those paired with one of `keys`, unless their
/// Default access policy is to deny.
pub(super) fn searchable<'a>(
    workspace: &'a Workspace,
    keys: &'a [(String, String)],
) -> impl Iterator<Item = &'a OpenVault> {
    workspace.readable_vaults().filter(|open| {
        open.vault.default_access_policy() != DefaultAccessPolicy::AlwaysDeny
            && keys
                .iter()
                .any(|(name, key)| open.vault.is_paired_with(name, key))
    })
}

/// The site's host, and its form's if that submits elsewhere.
pub(super) fn hosts(site: &Site, submit_url: &str) -> Vec<String> {
    let host = site.host();
    let form = Site::parse(submit_url)
        .map(|form| form.host())
        .filter(|form| *form != host);
    std::iter::once(host).chain(form).collect()
}

/// The Pairings the request holds keys for, as name and key.
pub(super) fn keys(message: &Map<String, Value>) -> Vec<(String, String)> {
    match message.get("keys") {
        Some(Value::Array(keys)) => keys
            .iter()
            .filter_map(Value::as_object)
            .map(|key| (text(key, "id").to_owned(), text(key, "key").to_owned()))
            .collect(),
        _ => Vec::new(),
    }
}
