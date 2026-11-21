//! `PairingScope`: what one pairing, Agent or Gateway, can reach in a Vault (ADR 0012 "What
//! Agents see, and who they are" and "Agents create secrets", ADR 0015 "Writes stay in its own
//! subgroup").
//!
//! A pairing reads the Entries in its own subgroup of the Agent group and in the subgroups it
//! was given to read, with their subgroups, never the Recycle Bin's, and never the Agent group's
//! own Entries, which belong to no pairing. It writes only Entries directly in its own subgroup.
//! That subgroup is known by its Group, never by its name or its path: two subgroups of the
//! Agent group may share a name, and an Entry in the one the pairing only reads then has a path
//! that looks like one of its own.
//!
//! [`AgentSession`](crate::AgentSession) and [`GatewaySession`](crate::gateway::GatewaySession)
//! both look at a Vault through a scope, read afresh for each request, so what a pairing reaches
//! is decided here once; what each shows of an Entry, how it audits and how it answers stay
//! theirs.

use keefast_core::{Entry, EntryId, Group, Vault};
use keefast_workspace::{VaultId, Workspace};
use secrecy::ExposeSecret;
use zeroize::Zeroizing;

use crate::{AgentPairing, AgentPairingId, AgentSettings, PairingKind};

/// Why a scope can't be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    #[error("the Vault is locked")]
    Locked,
    /// The pairing is gone, revoked, or of the other kind.
    #[error("the Vault has no such pairing of that kind")]
    NotPaired,
}

/// Why [`PairingScope::find`] found no single Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// No Entry the pairing reads has that UUID or path.
    Unknown,
    /// More than one Entry the pairing reads has that path.
    Ambiguous,
}

/// Why a Secret reference names no value, each with the reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    #[error("{0} names no field")]
    NoField(String),
    #[error("no Entry the pairing reads matches {0}")]
    Unknown(String),
    #[error("{0} matches more than one Entry")]
    Ambiguous(String),
    #[error("the Entry {0} names has no such field, or it is empty")]
    UnknownField(String),
}

/// An Entry a pairing reads.
#[derive(Debug, Clone)]
pub struct Readable<'v> {
    pub entry: &'v Entry,
    /// Its subgroups below the Agent group and its title, joined by `/`, as a Secret
    /// reference's path names it.
    pub path: String,
    /// Whether it is directly in the pairing's own subgroup, so the pairing may write it.
    pub own: bool,
}

/// What one pairing can reach in a Vault, as the Vault is now.
pub struct PairingScope<'v> {
    vault: &'v Vault,
    settings: AgentSettings,
    pairing: AgentPairing,
    readable: Vec<Readable<'v>>,
    own_path: Option<String>,
}

impl<'v> PairingScope<'v> {
    /// The scope of the pairing `pairing` of `kind` in the Vault `id`: `Locked` while that Vault
    /// is, and `NotPaired` once the pairing is gone, or if it is of the other kind.
    pub fn open(
        workspace: &'v Workspace,
        id: VaultId,
        pairing: &AgentPairingId,
        kind: PairingKind,
    ) -> Result<Self, ScopeError> {
        let vault = workspace.readable_vault(id).ok_or(ScopeError::Locked)?;
        Self::of(vault.vault(), pairing, kind).ok_or(ScopeError::NotPaired)
    }

    /// The scope of the pairing `pairing` of `kind` in `vault`; `None` if it has none of that
    /// kind.
    pub fn of(vault: &'v Vault, pairing: &AgentPairingId, kind: PairingKind) -> Option<Self> {
        let settings = AgentSettings::of(vault);
        let pairing = settings.pairing_of_kind(pairing, kind)?.clone();
        let mut walk = Walk {
            vault,
            pairing: &pairing,
            readable: Vec::new(),
            own_path: None,
        };
        if let Some(agent_group) = settings.agent_group().and_then(|id| vault.group(id)) {
            // The Agent group's own Entries belong to no pairing, so the walk starts below it.
            for subgroup in agent_group.groups() {
                walk.group(subgroup, "", false);
            }
        }
        let Walk {
            readable, own_path, ..
        } = walk;
        Some(Self {
            vault,
            settings,
            pairing,
            readable,
            own_path,
        })
    }

    pub fn pairing(&self) -> &AgentPairing {
        &self.pairing
    }

    /// The Vault's Agent settings, read with the scope.
    pub fn settings(&self) -> &AgentSettings {
        &self.settings
    }

    pub fn vault(&self) -> &'v Vault {
        self.vault
    }

    /// The Entries the pairing reads, in the Vault's order.
    pub fn entries(&self) -> &[Readable<'v>] {
        &self.readable
    }

    /// The Entry with the UUID `id`, if the pairing reads it.
    pub fn entry(&self, id: EntryId) -> Option<&Readable<'v>> {
        self.readable
            .iter()
            .find(|readable| readable.entry.id() == id)
    }

    /// The one Entry `name` names among those the pairing reads: by its UUID in hex, or else by
    /// its path.
    pub fn find(&self, name: &str) -> Result<&Readable<'v>, Lookup> {
        let by_uuid = EntryId::from_hex(name);
        let mut matches = self.readable.iter().filter(|readable| match by_uuid {
            Some(id) => readable.entry.id() == id,
            None => readable.path == name,
        });
        let found = matches.next().ok_or(Lookup::Unknown)?;
        if matches.next().is_some() {
            return Err(Lookup::Ambiguous);
        }
        Ok(found)
    }

    /// The Entry and value the Secret reference `reference` names: an Entry's UUID or path, then
    /// `#` and a field. The last `#` splits them, since titles may have one in them and field
    /// names rarely do. An empty field names no value.
    pub fn resolve(
        &self,
        reference: &str,
    ) -> Result<(&'v Entry, Zeroizing<String>), ReferenceError> {
        let (name, field) = reference
            .rsplit_once('#')
            .ok_or_else(|| ReferenceError::NoField(reference.to_owned()))?;
        let entry = match self.find(name) {
            Ok(found) => found.entry,
            Err(Lookup::Unknown) => return Err(ReferenceError::Unknown(reference.to_owned())),
            Err(Lookup::Ambiguous) => return Err(ReferenceError::Ambiguous(reference.to_owned())),
        };
        let value = match field {
            "password" => Some(entry.password().expose_secret()),
            "username" => Some(entry.username()),
            "url" => Some(entry.url()),
            _ => entry
                .custom_fields()
                .iter()
                .find(|custom| custom.name == field)
                .map(|custom| custom.value.expose_secret()),
        }
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ReferenceError::UnknownField(reference.to_owned()))?;
        Ok((entry, Zeroizing::new(value.to_owned())))
    }

    /// The path of the pairing's own subgroup below the Agent group, ending in `/` as Secret
    /// references go on from it; `None` once it is no longer in the Agent group, or is in the
    /// Recycle Bin.
    pub fn own_path(&self) -> Option<&str> {
        self.own_path.as_deref()
    }

    /// The title of a new Entry at `path`, if `path` is directly in the pairing's own subgroup:
    /// its own subgroup's path, then a title that isn't blank and has no `/`.
    pub fn new_title<'n>(&self, path: &'n str) -> Option<&'n str> {
        path.strip_prefix(self.own_path()?)
            .filter(|title| !title.contains('/') && !title.trim().is_empty())
    }
}

/// The walk down the Agent group that finds what a pairing reads, and where its own subgroup is.
struct Walk<'v, 'p> {
    vault: &'v Vault,
    pairing: &'p AgentPairing,
    readable: Vec<Readable<'v>>,
    own_path: Option<String>,
}

impl<'v> Walk<'v, '_> {
    fn group(&mut self, group: &'v Group, parent_path: &str, parent_read: bool) {
        if self.vault.recycle_bin() == Some(group.id()) {
            return;
        }
        let path = format!("{parent_path}{}/", group.name());
        let own = group.id() == self.pairing.group();
        if own {
            self.own_path = Some(path.clone());
        }
        let read = parent_read || own || self.pairing.readable().contains(&group.id());
        if read {
            self.readable
                .extend(group.entries().iter().map(|entry| Readable {
                    entry,
                    path: format!("{path}{}", entry.title()),
                    own,
                }));
        }
        for subgroup in group.groups() {
            self.group(subgroup, &path, read);
        }
    }
}
