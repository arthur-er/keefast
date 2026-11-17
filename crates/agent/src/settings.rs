//! The Agent settings a Vault keeps (ADR 0005 update 2026-11-16, ADR 0012): its Agent group,
//! its Default agent policy, its Agent pairings and its Agent access decisions.
//!
//! They live in the Vault's custom data under keys of keefast's own, one timed item per
//! setting, so that a Merge keeps the newest of each, as it does tag colours: a decision made on
//! one Approver and a pairing made on another desktop both survive. A Merge never removes an
//! item, so nothing here is ever removed: a forgotten decision is left empty, and a revoked
//! pairing is marked by an item of its own, which outlives any copy of the pairing a Merge
//! brings back.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keefast_core::{EntryId, GroupId, Vault};
use keefast_workspace::{EditVaultError, VaultId, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// The Agent group, as its UUID in hex.
const AGENT_GROUP_KEY: &str = "keefast_AGENT_GROUP";
/// The Default agent policy: `ask`, `allow` or `deny`.
const POLICY_KEY: &str = "keefast_DEFAULT_AGENT_POLICY";
/// Each Agent pairing, under this prefix plus its id, as JSON (see [`StoredPairing`]).
const PAIRING_PREFIX: &str = "keefast_AGENT_PAIRING_";
/// Each revoked Agent pairing, under this prefix plus its id, with its name as the value.
const REVOKED_PREFIX: &str = "keefast_AGENT_REVOKED_PAIRING_";
/// Each Agent access decision, under this prefix plus `<Entry UUID>_<pairing id>_<use>`, where
/// the use is `program:<name>` or `host:<name>`; the value is `allow`, `deny`, or empty once
/// forgotten. One item per decision rather than one per Entry, so that decisions remembered on
/// two Approvers at once both survive a Merge (ADR 0005 update 2026-11-17).
const DECISION_PREFIX: &str = "keefast_AGENT_DECISION_";

/// What an Agent uses an Entry with: the program `keefast-agent run` starts, by its
/// executable's name, or the host an `http_request` goes to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentUse {
    Program(String),
    Host(String),
}

impl AgentUse {
    fn value(&self) -> String {
        match self {
            Self::Program(program) => format!("program:{program}"),
            Self::Host(host) => format!("host:{host}"),
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        if let Some(program) = value.strip_prefix("program:") {
            Some(Self::Program(program.to_owned()))
        } else {
            value
                .strip_prefix("host:")
                .map(|host| Self::Host(host.to_owned()))
        }
    }
}

/// The user's answer to an Agent using an Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentDecision {
    Allow,
    Deny,
}

impl AgentDecision {
    fn value(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Self::Allow),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// An Agent access decision a Vault remembers: an Agent pairing using an Entry with a program
/// or host, allowed or denied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAccessDecision {
    pub entry: EntryId,
    pub pairing: AgentPairingId,
    pub agent_use: AgentUse,
    pub decision: AgentDecision,
}

impl AgentAccessDecision {
    /// The decision stored under `key` with `value`, unless it was forgotten or can't be read.
    fn read(key: &str, value: &str) -> Option<Self> {
        let rest = key.strip_prefix(DECISION_PREFIX)?;
        let mut parts = rest.splitn(3, '_');
        let entry = EntryId::from_hex(parts.next()?)?;
        let pairing = AgentPairingId(parts.next()?.to_owned());
        let agent_use = AgentUse::from_value(parts.next()?)?;
        Some(Self {
            entry,
            pairing,
            agent_use,
            decision: AgentDecision::from_value(value)?,
        })
    }
}

fn decision_key(entry: EntryId, pairing: &AgentPairingId, agent_use: &AgentUse) -> String {
    format!(
        "{DECISION_PREFIX}{}_{}_{}",
        entry.to_hex(),
        pairing.0,
        agent_use.value()
    )
}

/// Why Agent settings couldn't be changed. The Vault is unchanged when one is returned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentSettingsError {
    #[error("an Agent pairing needs a name")]
    PairingNameEmpty,
    #[error("an Agent pairing's name can't have a / in it: it names its subgroup")]
    PairingNameHasSlash,
    #[error("an Agent pairing called {0} exists already")]
    PairingNameTaken(String),
    #[error("the Vault has no such Agent pairing")]
    PairingNotFound,
    #[error("the Vault has no Agent group yet: choose one first")]
    NoAgentGroup,
    #[error(transparent)]
    Vault(#[from] EditVaultError),
}

/// What a Vault answers an Agent using an Entry it has no Agent access decision for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DefaultAgentPolicy {
    /// Ask the user, through an Approver. Until one answers, the use is denied.
    #[default]
    Ask,
    Allow,
    Deny,
}

impl DefaultAgentPolicy {
    fn value(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    /// The policy stored as `value`. One keefast doesn't know, as from a later version, asks.
    fn from_value(value: &str) -> Self {
        match value {
            "allow" => Self::Allow,
            "deny" => Self::Deny,
            _ => Self::Ask,
        }
    }
}

/// An Agent pairing's id: random, never reused, and the same in every copy of the Vault, so a
/// pairing can be renamed and still be known.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentPairingId(String);

impl AgentPairingId {
    fn random() -> Self {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("the OS random number generator is available");
        Self(hex(&bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How an Agent pairing's Agent proves itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCredential {
    /// A bearer token, of which the Vault keeps only the SHA-256 hash, in hex: reading the
    /// Vault file gives no working token. Tokens are long and random, so an unsalted hash is
    /// enough.
    TokenHash(String),
    /// An OAuth client registered with a Headless host. Its tokens stay in that host's own
    /// state (ADR 0005 update 2026-11-16).
    OAuthClient(String),
}

impl AgentCredential {
    /// The credential for `token`, keeping only its hash.
    pub fn token(token: &str) -> Self {
        Self::TokenHash(token_hash(token))
    }

    /// Whether `token` is this credential's token.
    pub fn accepts_token(&self, token: &str) -> bool {
        match self {
            Self::TokenHash(hash) => token_hash(token).as_bytes().ct_eq(hash.as_bytes()).into(),
            Self::OAuthClient(_) => false,
        }
    }
}

fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// What the user chooses when creating an Agent pairing.
#[derive(Debug, Clone)]
pub struct NewAgentPairing {
    pub name: String,
    /// The pairing's own subgroup of the Agent group, which it reads and writes to.
    pub group: GroupId,
    /// Other subgroups of the Agent group it may read, such as a shared one.
    pub readable: Vec<GroupId>,
    /// Whether it may create Entries in its own subgroup.
    pub can_create: bool,
    pub credential: AgentCredential,
}

/// A pairing just created, with its token: the only time the token is known, since the Vault
/// keeps only its hash.
#[derive(Debug)]
pub struct CreatedAgentPairing {
    pub pairing: AgentPairing,
    pub token: Zeroizing<String>,
}

/// The prefix of every token keefast makes, so a leaked one is easy to recognise.
pub const TOKEN_PREFIX: &str = "kfa_";

/// A new random bearer token: 256 bits, in URL-safe base64.
fn new_token() -> Zeroizing<String> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    getrandom::fill(bytes.as_mut()).expect("the OS random number generator is available");
    Zeroizing::new(format!(
        "{TOKEN_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes.as_ref())
    ))
}

/// An Agent pairing a Vault keeps.
#[derive(Debug, Clone)]
pub struct AgentPairing {
    id: AgentPairingId,
    stored: StoredPairing,
}

impl AgentPairing {
    pub fn id(&self) -> &AgentPairingId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.stored.name
    }

    /// Its own subgroup of the Agent group.
    pub fn group(&self) -> GroupId {
        self.stored.group
    }

    /// The other subgroups of the Agent group it may read.
    pub fn readable(&self) -> &[GroupId] {
        &self.stored.readable
    }

    pub fn can_create(&self) -> bool {
        self.stored.create
    }

    pub fn credential(&self) -> AgentCredential {
        match (&self.stored.token_sha256, &self.stored.oauth_client) {
            (Some(hash), _) => AgentCredential::TokenHash(hash.clone()),
            (None, Some(client)) => AgentCredential::OAuthClient(client.clone()),
            // A pairing without either, as one edited by hand, accepts no token.
            (None, None) => AgentCredential::TokenHash(String::new()),
        }
    }
}

/// An Agent pairing as JSON in the Vault. Keys a later keefast adds are kept when this one
/// rewrites the pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPairing {
    name: String,
    #[serde(with = "group_hex")]
    group: GroupId,
    #[serde(default, with = "groups_hex")]
    readable: Vec<GroupId>,
    #[serde(default)]
    create: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oauth_client: Option<String>,
    #[serde(flatten)]
    unknown: Map<String, Value>,
}

impl StoredPairing {
    fn new(pairing: NewAgentPairing) -> Self {
        let (token_sha256, oauth_client) = match pairing.credential {
            AgentCredential::TokenHash(hash) => (Some(hash), None),
            AgentCredential::OAuthClient(client) => (None, Some(client)),
        };
        Self {
            name: pairing.name,
            group: pairing.group,
            readable: pairing.readable,
            create: pairing.can_create,
            token_sha256,
            oauth_client,
            unknown: Map::new(),
        }
    }
}

mod group_hex {
    use keefast_core::GroupId;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(group: &GroupId, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&group.to_hex())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<GroupId, D::Error> {
        let hex = String::deserialize(deserializer)?;
        GroupId::from_hex(&hex).ok_or_else(|| D::Error::custom("not a Group UUID"))
    }
}

mod groups_hex {
    use keefast_core::GroupId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(groups: &[GroupId], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(groups.iter().map(|group| group.to_hex()))
    }

    /// Group UUIDs that can't be read are left out.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<GroupId>, D::Error> {
        let hexes = Vec::<String>::deserialize(deserializer)?;
        Ok(hexes
            .iter()
            .filter_map(|hex| GroupId::from_hex(hex))
            .collect())
    }
}

/// A Vault's Agent settings, as read from it at one moment.
#[derive(Debug, Clone)]
pub struct AgentSettings {
    agent_group: Option<GroupId>,
    default_agent_policy: DefaultAgentPolicy,
    pairings: Vec<AgentPairing>,
    decisions: Vec<AgentAccessDecision>,
}

impl AgentSettings {
    /// The Agent settings `vault` keeps.
    pub fn of(vault: &Vault) -> Self {
        let items: BTreeMap<String, String> = vault.custom_data().collect();
        let get = |key: &str| items.get(key).map(String::as_str).unwrap_or_default();
        let agent_group = GroupId::from_hex(get(AGENT_GROUP_KEY))
            .filter(|group| matches!(vault.is_group_in_recycle_bin(*group), Ok(false)));
        let mut pairings: Vec<AgentPairing> = items
            .iter()
            .filter_map(|(key, value)| {
                let id = key.strip_prefix(PAIRING_PREFIX)?;
                if items.contains_key(&format!("{REVOKED_PREFIX}{id}")) {
                    return None;
                }
                // A pairing that can't be read, as one edited by hand, is left out.
                let stored = serde_json::from_str(value).ok()?;
                Some(AgentPairing {
                    id: AgentPairingId(id.to_owned()),
                    stored,
                })
            })
            .collect();
        pairings.sort_by(|a, b| a.name().cmp(b.name()).then_with(|| a.id.cmp(&b.id)));
        Self {
            agent_group,
            default_agent_policy: DefaultAgentPolicy::from_value(get(POLICY_KEY)),
            pairings,
            decisions: items
                .iter()
                .filter_map(|(key, value)| AgentAccessDecision::read(key, value))
                .collect(),
        }
    }

    /// The decision remembered for `pairing` using `entry` with `agent_use`, if there is one.
    pub fn decision(
        &self,
        entry: EntryId,
        pairing: &AgentPairingId,
        agent_use: &AgentUse,
    ) -> Option<AgentDecision> {
        self.decisions
            .iter()
            .find(|known| {
                known.entry == entry && &known.pairing == pairing && &known.agent_use == agent_use
            })
            .map(|known| known.decision)
    }

    /// The decisions remembered on `entry`, by pairing, then use.
    pub fn decisions_on(&self, entry: EntryId) -> impl Iterator<Item = &AgentAccessDecision> {
        self.decisions
            .iter()
            .filter(move |decision| decision.entry == entry)
    }

    /// The Agent pairings, revoked ones left out, by name.
    pub fn pairings(&self) -> &[AgentPairing] {
        &self.pairings
    }

    pub fn pairing(&self, id: &AgentPairingId) -> Option<&AgentPairing> {
        self.pairings.iter().find(|pairing| &pairing.id == id)
    }

    /// The Agent pairing whose token is `token`, if one is.
    pub fn pairing_with_token(&self, token: &str) -> Option<&AgentPairing> {
        self.pairings
            .iter()
            .find(|pairing| pairing.credential().accepts_token(token))
    }

    /// The Group whose Entries Agents may see; `None` when none was chosen, or the one chosen
    /// is gone or in the Recycle Bin, and then Agents see nothing.
    pub fn agent_group(&self) -> Option<GroupId> {
        self.agent_group
    }

    pub fn default_agent_policy(&self) -> DefaultAgentPolicy {
        self.default_agent_policy
    }
}

/// Reading and changing a Vault's Agent settings through the Workspace that holds it unlocked.
/// Each change leaves the Vault with unsaved changes, unless it changes nothing.
pub trait AgentSettingsExt {
    /// The Agent settings of an unlocked Vault; `None` when it is locked or not open.
    fn agent_settings(&self, vault: VaultId) -> Option<AgentSettings>;

    /// Makes `group` the Vault's Agent group.
    fn set_agent_group(&mut self, vault: VaultId, group: GroupId)
    -> Result<(), AgentSettingsError>;

    fn set_default_agent_policy(
        &mut self,
        vault: VaultId,
        policy: DefaultAgentPolicy,
    ) -> Result<(), AgentSettingsError>;

    /// Adds an Agent pairing with a new id. Its name must not be empty.
    fn add_agent_pairing(
        &mut self,
        vault: VaultId,
        pairing: NewAgentPairing,
    ) -> Result<AgentPairing, AgentSettingsError>;

    /// Creates an Agent pairing called `name` with a new bearer token, and its own subgroup of
    /// the Agent group by the same name, unless one is there already, such as one a revoked
    /// pairing left. It reads `readable` too, and creates Entries if `can_create`.
    fn create_agent_pairing(
        &mut self,
        vault: VaultId,
        name: &str,
        readable: Vec<GroupId>,
        can_create: bool,
    ) -> Result<CreatedAgentPairing, AgentSettingsError>;

    /// Sets which other subgroups an Agent pairing reads, and whether it creates Entries.
    fn set_agent_pairing_access(
        &mut self,
        vault: VaultId,
        pairing: &AgentPairingId,
        readable: Vec<GroupId>,
        can_create: bool,
    ) -> Result<(), AgentSettingsError>;

    /// Revokes an Agent pairing for good: no copy of the Vault Merged in later brings it back.
    /// The Entries it created stay where they are.
    fn revoke_agent_pairing(
        &mut self,
        vault: VaultId,
        pairing: &AgentPairingId,
    ) -> Result<(), AgentSettingsError>;

    /// Remembers `decision` for `pairing` using `entry` with `agent_use`.
    fn remember_agent_decision(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        pairing: &AgentPairingId,
        agent_use: &AgentUse,
        decision: AgentDecision,
    ) -> Result<(), AgentSettingsError>;

    /// Forgets the decision for `pairing` using `entry` with `agent_use`, so it asks again,
    /// or follows the Default agent policy.
    fn forget_agent_decision(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        pairing: &AgentPairingId,
        agent_use: &AgentUse,
    ) -> Result<(), AgentSettingsError>;
}

impl AgentSettingsExt for Workspace {
    fn agent_settings(&self, vault: VaultId) -> Option<AgentSettings> {
        Some(AgentSettings::of(self.readable_vault(vault)?.vault()))
    }

    fn set_agent_group(
        &mut self,
        vault: VaultId,
        group: GroupId,
    ) -> Result<(), AgentSettingsError> {
        Ok(self.set_vault_custom_data(vault, &[item(AGENT_GROUP_KEY, group.to_hex())])?)
    }

    fn set_default_agent_policy(
        &mut self,
        vault: VaultId,
        policy: DefaultAgentPolicy,
    ) -> Result<(), AgentSettingsError> {
        Ok(self.set_vault_custom_data(vault, &[item(POLICY_KEY, policy.value())])?)
    }

    fn add_agent_pairing(
        &mut self,
        vault: VaultId,
        pairing: NewAgentPairing,
    ) -> Result<AgentPairing, AgentSettingsError> {
        if pairing.name.trim().is_empty() {
            return Err(AgentSettingsError::PairingNameEmpty);
        }
        let id = AgentPairingId::random();
        let stored = StoredPairing::new(pairing);
        let json = serde_json::to_string(&stored).expect("a pairing is plain JSON");
        self.set_vault_custom_data(vault, &[item(format!("{PAIRING_PREFIX}{}", id.0), json)])?;
        Ok(AgentPairing { id, stored })
    }

    fn create_agent_pairing(
        &mut self,
        vault: VaultId,
        name: &str,
        readable: Vec<GroupId>,
        can_create: bool,
    ) -> Result<CreatedAgentPairing, AgentSettingsError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AgentSettingsError::PairingNameEmpty);
        }
        if name.contains('/') {
            return Err(AgentSettingsError::PairingNameHasSlash);
        }
        let settings = self
            .agent_settings(vault)
            .ok_or(EditVaultError::VaultNotOpen)?;
        if settings
            .pairings()
            .iter()
            .any(|pairing| pairing.name() == name)
        {
            return Err(AgentSettingsError::PairingNameTaken(name.to_owned()));
        }
        let agent_group = settings
            .agent_group()
            .ok_or(AgentSettingsError::NoAgentGroup)?;
        let existing = self
            .readable_vault(vault)
            .and_then(|open| open.vault().group(agent_group))
            .and_then(|group| group.groups().iter().find(|child| child.name() == name))
            .map(|group| group.id());
        let group = match existing {
            Some(group) => group,
            None => self.create_group(vault, agent_group, name)?,
        };
        let token = new_token();
        let pairing = self.add_agent_pairing(
            vault,
            NewAgentPairing {
                name: name.to_owned(),
                group,
                readable,
                can_create,
                credential: AgentCredential::token(&token),
            },
        )?;
        Ok(CreatedAgentPairing { pairing, token })
    }

    fn set_agent_pairing_access(
        &mut self,
        vault: VaultId,
        pairing: &AgentPairingId,
        readable: Vec<GroupId>,
        can_create: bool,
    ) -> Result<(), AgentSettingsError> {
        let settings = self
            .agent_settings(vault)
            .ok_or(EditVaultError::VaultNotOpen)?;
        let pairing = settings
            .pairing(pairing)
            .ok_or(AgentSettingsError::PairingNotFound)?;
        let mut stored = pairing.stored.clone();
        stored.readable = readable;
        stored.create = can_create;
        let json = serde_json::to_string(&stored).expect("a pairing is plain JSON");
        let key = format!("{PAIRING_PREFIX}{}", pairing.id.0);
        Ok(self.set_vault_custom_data(vault, &[item(key, json)])?)
    }

    fn revoke_agent_pairing(
        &mut self,
        vault: VaultId,
        pairing: &AgentPairingId,
    ) -> Result<(), AgentSettingsError> {
        let settings = self
            .agent_settings(vault)
            .ok_or(EditVaultError::VaultNotOpen)?;
        let pairing = settings
            .pairing(pairing)
            .ok_or(AgentSettingsError::PairingNotFound)?;
        let key = format!("{REVOKED_PREFIX}{}", pairing.id.0);
        Ok(self.set_vault_custom_data(vault, &[item(key, pairing.name())])?)
    }

    fn remember_agent_decision(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        pairing: &AgentPairingId,
        agent_use: &AgentUse,
        decision: AgentDecision,
    ) -> Result<(), AgentSettingsError> {
        let key = decision_key(entry, pairing, agent_use);
        Ok(self.set_vault_custom_data(vault, &[item(key, decision.value())])?)
    }

    fn forget_agent_decision(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        pairing: &AgentPairingId,
        agent_use: &AgentUse,
    ) -> Result<(), AgentSettingsError> {
        let settings = self
            .agent_settings(vault)
            .ok_or(EditVaultError::VaultNotOpen)?;
        // Never remembered: nothing to leave empty.
        if settings.decision(entry, pairing, agent_use).is_none() {
            return Ok(());
        }
        let key = decision_key(entry, pairing, agent_use);
        Ok(self.set_vault_custom_data(vault, &[item(key, "")])?)
    }
}

fn item(key: impl Into<String>, value: impl Into<String>) -> (String, String) {
    (key.into(), value.into())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
