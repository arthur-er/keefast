//! Agents creating secrets (ADR 0012 "Agents create secrets"): `create_entry`, whose password
//! keefast can generate and never returns, and `store`, which sets one field from a value the
//! Agent piped to `keefast-agent store`, so a secret it mints never passes through its context.
//!
//! Only an Agent pairing with the create right writes, and only Entries directly in its own
//! subgroup: one it merely reads, even in a subgroup shared with it, stays as the user left it.
//! Agents never delete. A changed Entry goes through the Workspace's ordinary edit, so its
//! previous version stays in its history for the user to see and restore; nothing is written to
//! an Entry's custom data, whose writes skip history. Every write, and every refused one, is
//! audited with the Entry and the field's name, never its value.

use std::collections::BTreeMap;

use keefast_core::{CustomField, EntryFields, EntryId, GroupId, PasswordRecipe, Vault};
use keefast_workspace::{VaultId, Workspace};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{AgentError, AgentReply, AgentSession, SecretReference, VaultAccess, readable_entries};
use crate::audit::{AuditAction, AuditEntry, AuditOutcome};
use crate::scope::{Lookup, PairingScope};
use crate::{AgentPairing, AgentSettings, PairingKind};

/// What `create_entry` makes: an Entry with this title in the pairing's own subgroup.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CreateEntry {
    pub title: String,
    pub username: String,
    pub url: String,
    pub tags: Vec<String>,
    /// Custom fields by name. They aren't secret, and are stored unprotected: a secret goes in
    /// with a generated password or `keefast-agent store`, never through the Agent's context.
    pub fields: BTreeMap<String, String>,
    /// How keefast generates the password, if it does. It is never returned.
    #[serde(default, with = "recipe")]
    pub password: Option<PasswordRecipe>,
}

/// What `keefast-agent store` asks: to set the field `reference` names to `value`. Its `Debug`
/// hides the value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSecret {
    /// A Secret reference in the pairing's own subgroup, such as `claude/Fly.io#token`.
    pub reference: String,
    value: Zeroizing<String>,
}

impl StoreSecret {
    pub fn new(reference: String, value: Zeroizing<String>) -> Self {
        Self { reference, value }
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Debug for StoreSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreSecret")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}

/// The Entry a write made or changed: how to name it, and whether it is new.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrittenEntry {
    pub reference: SecretReference,
    pub created: bool,
}

/// A field `store` sets, by the name its Secret reference ends in.
enum Field {
    Password,
    Username,
    Url,
    Custom(String),
}

impl Field {
    /// The field `name` means. Notes and the title aren't fields a Secret reference reaches, so
    /// they can't be set either; KeePass's own names for the standard fields would shadow them.
    fn named(name: &str) -> Result<Self, AgentError> {
        let bad = |message: String| Err(AgentError::BadEntry(message));
        let lower = name.to_lowercase();
        match lower.as_str() {
            "notes" => bad("Agents can't set an Entry's notes".to_owned()),
            "title" => bad(
                "the title is the last step of the Entry's path, before #: store sets a field"
                    .to_owned(),
            ),
            "password" | "username" | "url" if name != lower => {
                bad(format!("write the field as #{lower}"))
            }
            "password" => Ok(Self::Password),
            "username" => Ok(Self::Username),
            "url" => Ok(Self::Url),
            _ if name.trim() != name => bad(format!(
                "the field name \"{name}\" starts or ends with spaces, which KeePass clients trim"
            )),
            _ => Ok(Self::Custom(name.to_owned())),
        }
    }

    /// `fields` with this field set to `value`. A custom field keeps its place, and is protected
    /// from now on, as a value an Agent stores is a secret.
    fn set(&self, fields: &mut EntryFields, value: &str) {
        match self {
            Self::Password => fields.password = SecretString::from(value.to_owned()),
            Self::Username => fields.username = value.to_owned(),
            Self::Url => fields.url = value.to_owned(),
            Self::Custom(name) => {
                let value = SecretString::from(value.to_owned());
                match fields.custom_fields.iter_mut().find(|f| &f.name == name) {
                    Some(field) => {
                        field.value = value;
                        field.protected = true;
                    }
                    None => fields.custom_fields.push(CustomField {
                        name: name.clone(),
                        value,
                        protected: true,
                    }),
                }
            }
        }
    }
}

/// Where a `store` writes, as one look at the Vault found it.
enum Target {
    Existing {
        id: EntryId,
        path: String,
        fields: EntryFields,
    },
    New {
        group: GroupId,
        title: String,
        path: String,
    },
}

/// What an Agent's write is audited as: its action, and for a `store`, the field.
struct Write<'a> {
    action: AuditAction,
    field: Option<&'a str>,
}

impl AgentSession {
    pub(super) fn create_entry(
        &self,
        vault: &impl VaultAccess,
        request: CreateEntry,
    ) -> Result<AgentReply, AgentError> {
        vault
            .edit(|workspace, id| self.create_in(workspace, id, request))
            .unwrap_or(Err(AgentError::Locked))
    }

    pub(super) fn store(
        &self,
        vault: &impl VaultAccess,
        request: StoreSecret,
    ) -> Result<AgentReply, AgentError> {
        vault
            .edit(|workspace, id| self.store_in(workspace, id, request))
            .unwrap_or(Err(AgentError::Locked))
    }

    fn create_in(
        &self,
        workspace: &mut Workspace,
        id: VaultId,
        request: CreateEntry,
    ) -> Result<AgentReply, AgentError> {
        let write = Write {
            action: AuditAction::Create,
            field: None,
        };
        let (pairing, group, path) = {
            let vault = workspace
                .readable_vault(id)
                .ok_or(AgentError::Locked)?
                .vault();
            let settings = AgentSettings::of(vault);
            let pairing = self.writer(workspace, &settings, &write)?;
            let title = request.title.trim();
            if title.is_empty() {
                return Err(AgentError::BadEntry("the Entry needs a title".to_owned()));
            }
            if title.contains('/') {
                return Err(AgentError::BadEntry(
                    "a title can't have / in it: / separates the steps of a Secret reference's \
                     path, and Agents create Entries only directly in their own subgroup"
                        .to_owned(),
                ));
            }
            for name in request.fields.keys() {
                check_custom_field(name)?;
            }
            let own = own_path(vault, &settings, &pairing)
                .ok_or_else(|| AgentError::NotOwnEntry(format!("{}/{title}", pairing.name())))?;
            let path = format!("{own}{title}");
            if readable_entries(vault, &settings, &pairing)
                .iter()
                .any(|(_, known)| *known == path)
            {
                return Err(AgentError::EntryExists(path));
            }
            let group = pairing.group();
            (pairing, group, path)
        };
        let password = match &request.password {
            Some(recipe) => recipe
                .generate()
                .map_err(|error| AgentError::BadEntry(error.to_string()))?,
            None => SecretString::from(String::new()),
        };
        let fields = EntryFields {
            title: request.title.trim().to_owned(),
            username: request.username,
            password,
            url: request.url,
            tags: request.tags,
            custom_fields: request
                .fields
                .into_iter()
                .map(|(name, value)| CustomField {
                    name,
                    value: SecretString::from(value),
                    protected: false,
                })
                .collect(),
            ..EntryFields::default()
        };
        let entry = workspace
            .create_entry(id, group, &fields)
            .map_err(|error| AgentError::BadEntry(error.to_string()))?;
        self.audit_write(
            workspace,
            &pairing,
            &write,
            Some((entry, &fields.title)),
            None,
        );
        Ok(AgentReply::Written(WrittenEntry {
            reference: SecretReference {
                uuid: entry.to_hex(),
                path,
            },
            created: true,
        }))
    }

    fn store_in(
        &self,
        workspace: &mut Workspace,
        id: VaultId,
        request: StoreSecret,
    ) -> Result<AgentReply, AgentError> {
        let reference = request.reference.as_str();
        let (name, field_name) = reference
            .rsplit_once('#')
            .filter(|(_, field)| !field.is_empty())
            .ok_or_else(|| AgentError::NoField(reference.to_owned()))?;
        let write = Write {
            action: AuditAction::Store,
            field: Some(field_name),
        };
        let (pairing, target) = {
            let scope = PairingScope::open(workspace, id, self.pairing(), PairingKind::Agent)?;
            let pairing = self.writer(workspace, scope.settings(), &write)?;
            let target = self.target(workspace, &scope, reference, name, &write)?;
            (pairing, target)
        };
        let field = Field::named(field_name)?;
        if request.value().is_empty() {
            return Err(AgentError::BadEntry(
                "there is nothing to store: the value is empty".to_owned(),
            ));
        }
        let (entry, title, path, created) = match target {
            Target::Existing {
                id: entry,
                path,
                mut fields,
            } => {
                field.set(&mut fields, request.value());
                workspace
                    .update_entry(id, entry, &fields)
                    .map_err(|error| AgentError::BadEntry(error.to_string()))?;
                (entry, fields.title, path, false)
            }
            Target::New { group, title, path } => {
                let mut fields = EntryFields {
                    title,
                    ..EntryFields::default()
                };
                field.set(&mut fields, request.value());
                let entry = workspace
                    .create_entry(id, group, &fields)
                    .map_err(|error| AgentError::BadEntry(error.to_string()))?;
                (entry, fields.title, path, true)
            }
        };
        self.audit_write(workspace, &pairing, &write, Some((entry, &title)), None);
        Ok(AgentReply::Written(WrittenEntry {
            reference: SecretReference {
                uuid: entry.to_hex(),
                path,
            },
            created,
        }))
    }

    /// The Entry a `store` writes: the one `name` matches among those the pairing reads, if it
    /// is directly in its own subgroup, or else a new one there, if `name` is a path directly in
    /// it.
    fn target(
        &self,
        workspace: &Workspace,
        scope: &PairingScope<'_>,
        reference: &str,
        name: &str,
        write: &Write<'_>,
    ) -> Result<Target, AgentError> {
        let pairing = scope.pairing();
        let refused = |entry: Option<(EntryId, &str)>| {
            self.audit_write(
                workspace,
                pairing,
                write,
                entry,
                Some("outside the pairing's own subgroup"),
            );
            AgentError::NotOwnEntry(reference.to_owned())
        };
        match scope.find(name) {
            Ok(found) if found.own => Ok(Target::Existing {
                id: found.entry.id(),
                path: found.path.clone(),
                fields: EntryFields::from(found.entry),
            }),
            Ok(found) => Err(refused(Some((found.entry.id(), found.entry.title())))),
            Err(Lookup::Ambiguous) => Err(AgentError::AmbiguousReference(reference.to_owned())),
            Err(Lookup::Unknown) if EntryId::from_hex(name).is_some() => {
                Err(AgentError::UnknownReference(reference.to_owned()))
            }
            Err(Lookup::Unknown) => {
                let title = scope.new_title(name).ok_or_else(|| refused(None))?;
                Ok(Target::New {
                    group: pairing.group(),
                    title: title.to_owned(),
                    path: name.to_owned(),
                })
            }
        }
    }

    /// The session's pairing, if it may write; a pairing without the create right is refused,
    /// and audited.
    fn writer(
        &self,
        workspace: &Workspace,
        settings: &AgentSettings,
        write: &Write<'_>,
    ) -> Result<AgentPairing, AgentError> {
        let pairing = self.own_pairing(settings)?.clone();
        if !pairing.can_create() {
            self.audit_write(
                workspace,
                &pairing,
                write,
                None,
                Some("the pairing may not create Entries"),
            );
            return Err(AgentError::NoCreateRight);
        }
        Ok(pairing)
    }

    /// Audits a write to `entry`: allowed without a `refusal`, denied with one.
    fn audit_write(
        &self,
        workspace: &Workspace,
        pairing: &AgentPairing,
        write: &Write<'_>,
        entry: Option<(EntryId, &str)>,
        refusal: Option<&str>,
    ) {
        let outcome = if refusal.is_some() {
            AuditOutcome::Denied
        } else {
            AuditOutcome::Allowed
        };
        let mut record = self.record(workspace, pairing, write.action, outcome);
        record.entry = entry.map(|(id, title)| AuditEntry {
            uuid: id.to_hex(),
            title: title.to_owned(),
        });
        record.field = write.field.map(str::to_owned);
        record.reason = refusal.map(str::to_owned);
        self.audit.record(record);
    }
}

/// A custom field `create_entry` may set: one a Secret reference can reach, by a name that
/// isn't a standard field's.
fn check_custom_field(name: &str) -> Result<(), AgentError> {
    match Field::named(name) {
        Ok(Field::Custom(_)) if !name.is_empty() => Ok(()),
        Ok(_) => Err(AgentError::BadEntry(format!(
            "{name} is one of the Entry's own fields, not a custom field: set it by its own name"
        ))),
        Err(error) => Err(error),
    }
}

/// The path of the pairing's own subgroup below the Agent group, ending in `/`, as Secret
/// references start with it; `None` if it is no longer in the Agent group.
pub(crate) fn own_path(
    vault: &Vault,
    settings: &AgentSettings,
    pairing: &AgentPairing,
) -> Option<String> {
    let agent_group = vault.group(settings.agent_group()?)?;
    let mut stack: Vec<(&keefast_core::Group, String)> = agent_group
        .groups()
        .iter()
        .map(|group| (group, String::new()))
        .collect();
    while let Some((group, parent)) = stack.pop() {
        if vault.recycle_bin() == Some(group.id()) {
            continue;
        }
        let path = format!("{parent}{}/", group.name());
        if group.id() == pairing.group() {
            return Some(path);
        }
        stack.extend(group.groups().iter().map(|child| (child, path.clone())));
    }
    None
}

/// A [`PasswordRecipe`] as JSON, for a `create_entry` the desktop app runs: `core` has no
/// serde of its own.
mod recipe {
    use keefast_core::{CharacterRecipe, PassphraseRecipe, PasswordRecipe};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Recipe {
        Characters {
            length: u32,
            lowercase: bool,
            uppercase: bool,
            digits: bool,
            symbols: bool,
            exclude_lookalikes: bool,
        },
        Passphrase {
            words: u32,
            separator: String,
        },
    }

    pub fn serialize<S: Serializer>(
        recipe: &Option<PasswordRecipe>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        recipe
            .as_ref()
            .map(|recipe| match recipe {
                PasswordRecipe::Characters(c) => Recipe::Characters {
                    length: c.length,
                    lowercase: c.lowercase,
                    uppercase: c.uppercase,
                    digits: c.digits,
                    symbols: c.symbols,
                    exclude_lookalikes: c.exclude_lookalikes,
                },
                PasswordRecipe::Passphrase(p) => Recipe::Passphrase {
                    words: p.words,
                    separator: p.separator.clone(),
                },
            })
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PasswordRecipe>, D::Error> {
        Ok(
            Option::<Recipe>::deserialize(deserializer)?.map(|recipe| match recipe {
                Recipe::Characters {
                    length,
                    lowercase,
                    uppercase,
                    digits,
                    symbols,
                    exclude_lookalikes,
                } => PasswordRecipe::Characters(CharacterRecipe {
                    length,
                    lowercase,
                    uppercase,
                    digits,
                    symbols,
                    exclude_lookalikes,
                }),
                Recipe::Passphrase { words, separator } => {
                    PasswordRecipe::Passphrase(PassphraseRecipe { words, separator })
                }
            }),
        )
    }
}
