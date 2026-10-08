//! The Vault's side of browser integration: its identity in the KeePassXC-Browser protocol, the
//! Pairings stored in it, the Entries it offers a site, the Access decisions remembered on
//! them, its Default access policy, and the Browser group captured logins are saved to.
//!
//! All of it lives in custom data (ADR 0005), under KeePassXC's keys where KeePassXC has them,
//! so a Vault paired or decided on in KeePassXC works in keefast and the other way round.

use secrecy::SecretString;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::edit::{Kind, child_or_insert, push, set_time, text_element};
use crate::vault::{decode_uuid, encode_uuid};
use crate::xml::{Element, Node};
use crate::{EditError, Entry, EntryFields, EntryId, Group, GroupId, Site, Timestamp, Vault};

/// KeePassXC stores each Pairing's identification key under this prefix plus the Pairing's name.
const PAIRING_KEY_PREFIX: &str = "KPXC_BROWSER_";
/// KeePassXC records when a Pairing was made under this prefix plus the Pairing's name.
const PAIRING_CREATED_PREFIX: &str = "_CREATED_";
/// keefast records a revoked Pairing under this prefix plus the Pairing's key, timed, so that a
/// Merge with a version of the Vault file that still holds the Pairing doesn't bring it back.
/// KeePassXC has no such record, so the key is keefast's own.
const REVOKED_PAIRING_PREFIX: &str = "keefast_REVOKED_PAIRING_";
/// KeePassXC remembers an Entry's Access decisions in its custom data under this key, as JSON:
/// `{"Allow":[hosts],"Deny":[hosts],"Realm":""}`.
const ACCESS_KEY: &str = "KeePassXC-Browser Settings";
/// An Entry, or a Group's Entries and subgroups, are hidden from browsers when their custom data
/// has this key set to `true`. A Group with it set to anything else shows its subgroups again.
const HIDE_KEY: &str = "BrowserHideEntry";
/// The Browser group chosen for the Vault, as its UUID in the Vault's custom data. KeePassXC
/// has no such setting, so the key is keefast's own.
const BROWSER_GROUP_KEY: &str = "keefast_BROWSER_GROUP";
/// The Vault's Default access policy, in its custom data. KeePassXC has no such setting, so the
/// key is keefast's own.
const ACCESS_POLICY_KEY: &str = "keefast_DEFAULT_ACCESS_POLICY";
/// The name of the Browser group when none was chosen: a top Group of this name, created when
/// the first login is saved.
pub const DEFAULT_BROWSER_GROUP: &str = "Browser";

/// A login the user submitted in the browser, to save to a Vault.
#[derive(Debug, Clone)]
pub struct CapturedLogin {
    /// The page the login was submitted on.
    pub url: String,
    pub username: String,
    pub password: SecretString,
}

impl CapturedLogin {
    /// What the site is called: its host, or the URL as given when it has none.
    pub fn site_name(&self) -> String {
        Site::parse(&self.url).map_or_else(|| self.url.clone(), |site| site.host())
    }
}

/// A remembered answer to a site asking for an Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    Allow,
    Deny,
}

impl AccessDecision {
    /// The list of hosts KeePassXC keeps this decision in.
    fn list(self) -> &'static str {
        match self {
            AccessDecision::Allow => "Allow",
            AccessDecision::Deny => "Deny",
        }
    }

    fn opposite(self) -> Self {
        match self {
            AccessDecision::Allow => AccessDecision::Deny,
            AccessDecision::Deny => AccessDecision::Allow,
        }
    }
}

/// An Access decision remembered on an Entry, for one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberedAccess {
    pub host: String,
    pub decision: AccessDecision,
}

/// What a Vault answers a site asking for an Entry that has no Access decision for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DefaultAccessPolicy {
    /// Ask the user.
    #[default]
    Ask,
    AlwaysAllow,
    AlwaysDeny,
}

impl DefaultAccessPolicy {
    /// The decision the policy makes for the user, if it doesn't ask.
    pub fn decision(self) -> Option<AccessDecision> {
        match self {
            DefaultAccessPolicy::Ask => None,
            DefaultAccessPolicy::AlwaysAllow => Some(AccessDecision::Allow),
            DefaultAccessPolicy::AlwaysDeny => Some(AccessDecision::Deny),
        }
    }

    /// The policy written as `value`, asking for a value keefast doesn't know.
    fn from_value(value: &str) -> Self {
        match value {
            "allow" => DefaultAccessPolicy::AlwaysAllow,
            "deny" => DefaultAccessPolicy::AlwaysDeny,
            _ => DefaultAccessPolicy::Ask,
        }
    }

    /// How the policy is written in the Vault's custom data.
    fn value(self) -> &'static str {
        match self {
            DefaultAccessPolicy::Ask => "ask",
            DefaultAccessPolicy::AlwaysAllow => "allow",
            DefaultAccessPolicy::AlwaysDeny => "deny",
        }
    }
}

/// An Entry offered to a site, with the Group it is in.
#[derive(Debug, Clone, Copy)]
pub struct SiteEntry<'a> {
    pub entry: &'a Entry,
    pub group: &'a Group,
}

/// A browser profile's Companion extension trusted by a Vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    /// The name the user gave the Pairing; the extension sends it as its `id`.
    pub name: String,
    /// The extension's identification public key, Base64-encoded.
    pub key: String,
}

impl Vault {
    /// The Vault's identity in the KeePassXC-Browser protocol, as KeePassXC computes it: the
    /// SHA-256, in hex, of the root Group's UUID in hex.
    pub fn browser_hash(&self) -> String {
        let uuid = self.root().id().0.simple().to_string();
        hex(&Sha256::digest(uuid.as_bytes()))
    }

    /// The Pairings stored in the Vault, in Vault file order.
    pub fn pairings(&self) -> Vec<Pairing> {
        self.custom_data()
            .filter_map(|(key, value)| {
                let name = key.strip_prefix(PAIRING_KEY_PREFIX)?;
                Some(Pairing {
                    name: name.to_owned(),
                    key: value,
                })
            })
            .collect()
    }

    /// Whether the Vault trusts the extension that identifies itself as `name` with `key`.
    pub fn is_paired_with(&self, name: &str, key: &str) -> bool {
        self.pairings()
            .iter()
            .any(|pairing| pairing.name == name && pairing.key == key)
    }

    /// Stores a new Pairing. Names are unique within a Vault, so a Pairing is never silently
    /// replaced.
    pub fn add_pairing(&mut self, name: &str, key: &str, now: Timestamp) -> Result<(), EditError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EditError::PairingNameEmpty);
        }
        if self.pairings().iter().any(|pairing| pairing.name == name) {
            return Err(EditError::PairingNameTaken);
        }
        self.set_vault_custom_data(&format!("{PAIRING_KEY_PREFIX}{name}"), key, now);
        self.set_vault_custom_data(
            &format!("{PAIRING_CREATED_PREFIX}{name}"),
            &now.to_iso_8601(),
            now,
        );
        Ok(())
    }

    /// Removes the Pairing named `name`: the extension holding its key gets nothing from the
    /// Vault any more, even after a Merge with a version of the Vault file made before.
    pub fn revoke_pairing(&mut self, name: &str, now: Timestamp) -> Result<(), EditError> {
        let pairing = self
            .pairings()
            .into_iter()
            .find(|pairing| pairing.name == name)
            .ok_or(EditError::PairingNotFound)?;
        self.remove_pairing(name);
        self.set_vault_custom_data(
            &format!("{REVOKED_PAIRING_PREFIX}{}", pairing.key),
            name,
            now,
        );
        Ok(())
    }

    /// Removes the Pairings revoked after they were made, which a Merge may have brought back.
    /// A Pairing without a time, as KeePassXC writes them, was made before any revocation.
    pub(crate) fn drop_revoked_pairings(&mut self) {
        let items: Vec<&Element> = self
            .file
            .document
            .child("Meta")
            .and_then(|meta| meta.child("CustomData"))
            .into_iter()
            .flat_map(|data| data.children_named("Item"))
            .collect();
        let text = |item: &Element, name| item.child(name).map(Element::text).unwrap_or_default();
        let modified = |item: &Element| {
            item.child("LastModificationTime")
                .and_then(|time| Timestamp::decode(&time.text()))
        };
        let revoked: Vec<String> = items
            .iter()
            .filter_map(|item| {
                let name = text(item, "Key")
                    .strip_prefix(PAIRING_KEY_PREFIX)?
                    .to_owned();
                let revocation = format!("{REVOKED_PAIRING_PREFIX}{}", text(item, "Value"));
                let revoked_at = items
                    .iter()
                    .find(|other| text(other, "Key") == revocation)
                    .map(|other| modified(other))?;
                (modified(item) <= revoked_at).then_some(name)
            })
            .collect();
        for name in revoked {
            self.remove_pairing(&name);
        }
    }

    fn remove_pairing(&mut self, name: &str) {
        let meta = child_or_insert(&mut self.file.document, "Meta", &[]);
        let custom_data = child_or_insert(meta, "CustomData", &[]);
        remove_custom_data(custom_data, &format!("{PAIRING_KEY_PREFIX}{name}"));
        remove_custom_data(custom_data, &format!("{PAIRING_CREATED_PREFIX}{name}"));
    }

    /// The Vault's custom data, `Meta/CustomData`, as key and value pairs.
    fn custom_data(&self) -> impl Iterator<Item = (String, String)> {
        self.file
            .document
            .child("Meta")
            .and_then(|meta| meta.child("CustomData"))
            .into_iter()
            .flat_map(|data| data.children_named("Item"))
            .map(|item| {
                let text = |name| item.child(name).map(Element::text).unwrap_or_default();
                (text("Key"), text("Value"))
            })
    }

    /// Sets an item of the Vault's custom data, with when it changed as KDBX 4.1 records it,
    /// so a Merge keeps the newest value.
    fn set_vault_custom_data(&mut self, key: &str, value: &str, now: Timestamp) {
        let meta = child_or_insert(&mut self.file.document, "Meta", &[]);
        let custom_data = child_or_insert(meta, "CustomData", &[]);
        let item = set_custom_data(custom_data, key, value);
        child_or_insert(item, "LastModificationTime", &["Value"]).set_text(now.encode());
    }
}

impl Vault {
    /// The Entries offered to `site`, in Vault file order: those whose URL or one of whose
    /// additional URLs is for it, leaving out Entries hidden from browsers and the Recycle Bin.
    pub fn entries_for_site(&self, site: &Site) -> Vec<SiteEntry<'_>> {
        let mut found = Vec::new();
        let bin = self.recycle_bin();
        let mut groups = vec![(self.root(), false)];
        while let Some((group, hidden)) = groups.pop() {
            if Some(group.id()) == bin {
                continue;
            }
            let hidden = group
                .custom_data
                .get(HIDE_KEY)
                .map_or(hidden, |value| value == "true");
            if !hidden {
                found.extend(
                    group
                        .entries()
                        .iter()
                        .filter(|entry| !entry.is_hidden_from_browser() && entry.is_for(site))
                        .map(|entry| SiteEntry { entry, group }),
                );
            }
            groups.extend(group.groups().iter().rev().map(|child| (child, hidden)));
        }
        found
    }

    /// Remembers `decision` on the Entry for each of `hosts`, replacing any earlier decision
    /// for them.
    pub fn remember_access(
        &mut self,
        entry: EntryId,
        hosts: &[String],
        decision: AccessDecision,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let path = self.path_to(Kind::Entry, entry.0)?;
        let mut settings = self
            .entry(entry)
            .map(Entry::access_settings)
            .unwrap_or_default();
        for host in hosts {
            settings.insert_host(decision, host);
            settings.remove_host(decision.opposite(), host);
        }
        let element = self.at_mut(&path);
        let custom_data = child_or_insert(
            element,
            "CustomData",
            &["AutoType", "Binary", "String", "Times"],
        );
        set_custom_data(custom_data, ACCESS_KEY, &settings.into_json());
        set_time(element, "LastModificationTime", now);
        self.refresh();
        Ok(())
    }

    /// Forgets the Access decision remembered on the Entry for `host`, or for every host when
    /// none is given, so the Default access policy applies again.
    pub fn forget_access(
        &mut self,
        entry: EntryId,
        host: Option<&str>,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let path = self.path_to(Kind::Entry, entry.0)?;
        let mut settings = self
            .entry(entry)
            .map(Entry::access_settings)
            .unwrap_or_default();
        for decision in [AccessDecision::Allow, AccessDecision::Deny] {
            match host {
                Some(host) => settings.remove_host(decision, host),
                None => settings.set_hosts(decision, Vec::new()),
            }
        }
        let element = self.at_mut(&path);
        let custom_data = child_or_insert(
            element,
            "CustomData",
            &["AutoType", "Binary", "String", "Times"],
        );
        set_custom_data(custom_data, ACCESS_KEY, &settings.into_json());
        set_time(element, "LastModificationTime", now);
        self.refresh();
        Ok(())
    }

    /// What the Vault answers a site asking for an Entry without an Access decision for it.
    /// A value keefast doesn't know reads as asking.
    pub fn default_access_policy(&self) -> DefaultAccessPolicy {
        self.custom_data()
            .find(|(key, _)| key == ACCESS_POLICY_KEY)
            .map_or_else(DefaultAccessPolicy::default, |(_, value)| {
                DefaultAccessPolicy::from_value(&value)
            })
    }

    /// Sets the Default access policy, timed so a Merge keeps the newest.
    pub fn set_default_access_policy(&mut self, policy: DefaultAccessPolicy, now: Timestamp) {
        self.set_vault_custom_data(ACCESS_POLICY_KEY, policy.value(), now);
    }

    /// The decision on giving the Entry to `hosts`: the Access decision remembered on it, or
    /// else the Default access policy's, if it doesn't ask.
    pub fn access_decision(&self, entry: &Entry, hosts: &[String]) -> Option<AccessDecision> {
        entry
            .access_decision(hosts)
            .or_else(|| self.default_access_policy().decision())
    }
}

impl Vault {
    /// The Group captured logins are saved to: the Group chosen for it, or else a top Group
    /// named [`DEFAULT_BROWSER_GROUP`]. None when there is neither, or the one chosen is in the
    /// Recycle Bin; saving a login then makes one.
    pub fn browser_group(&self) -> Option<GroupId> {
        let usable = |group: GroupId| {
            let binned = self.is_group_in_recycle_bin(group).ok()?;
            (!binned).then_some(group)
        };
        let chosen = self
            .custom_data()
            .find(|(key, _)| key == BROWSER_GROUP_KEY)
            .and_then(|(_, value)| decode_uuid(&value))
            .map(GroupId);
        match chosen.and_then(usable) {
            Some(group) => Some(group),
            None => self
                .root()
                .groups()
                .iter()
                .find(|group| group.name() == DEFAULT_BROWSER_GROUP)
                .and_then(|group| usable(group.id())),
        }
    }

    /// Makes `group` the Browser group. It stays so when renamed or moved. A Group in the
    /// Recycle Bin can't be.
    pub fn set_browser_group(&mut self, group: GroupId, now: Timestamp) -> Result<(), EditError> {
        if self.is_group_in_recycle_bin(group)? {
            return Err(EditError::InRecycleBin);
        }
        self.set_vault_custom_data(BROWSER_GROUP_KEY, &encode_uuid(group.0), now);
        Ok(())
    }

    /// Saves a captured login as a new Entry in `group`, or else in the Browser group, made
    /// if the Vault has none. The Entry is titled with the site's host. A Group in the
    /// Recycle Bin is refused.
    pub fn add_login(
        &mut self,
        group: Option<GroupId>,
        login: &CapturedLogin,
        now: Timestamp,
    ) -> Result<EntryId, EditError> {
        let group = match group {
            Some(group) if self.is_group_in_recycle_bin(group)? => {
                return Err(EditError::InRecycleBin);
            }
            Some(group) => group,
            None => self.ensure_browser_group(now)?,
        };
        let fields = EntryFields {
            title: login.site_name(),
            username: login.username.clone(),
            password: login.password.clone(),
            url: login.url.clone(),
            ..EntryFields::default()
        };
        self.create_entry(group, &fields, now)
    }

    /// Updates an Entry with a captured login's username and password, keeping the version it
    /// replaces in the Entry's history. A login without a username keeps the Entry's, as
    /// KeePassXC does.
    pub fn update_login(
        &mut self,
        entry: EntryId,
        login: &CapturedLogin,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let current = self.entry(entry).ok_or(EditError::EntryNotFound)?;
        let mut fields = EntryFields {
            password: login.password.clone(),
            ..EntryFields::from(current)
        };
        if !login.username.is_empty() {
            fields.username = login.username.clone();
        }
        self.update_entry(entry, &fields, now)
    }

    /// The Browser group, made at the top of the Vault and chosen if there is none.
    fn ensure_browser_group(&mut self, now: Timestamp) -> Result<GroupId, EditError> {
        if let Some(group) = self.browser_group() {
            return Ok(group);
        }
        let group = self.create_group(self.root().id(), DEFAULT_BROWSER_GROUP, now)?;
        self.set_browser_group(group, now)?;
        Ok(group)
    }
}

impl Entry {
    /// Whether the Entry itself is marked to be hidden from browsers.
    pub fn is_hidden_from_browser(&self) -> bool {
        self.custom_data
            .get(HIDE_KEY)
            .is_some_and(|value| value == "true")
    }

    /// Whether the Entry's URL or one of its additional URLs is for `site`.
    pub fn is_for(&self, site: &Site) -> bool {
        site.matches(self.url()) || self.additional_urls().iter().any(|url| site.matches(url))
    }

    /// The Access decision remembered for `hosts`, a site's host and the host its form submits
    /// to: allowed when every one of them is allowed, denied when any is denied, and otherwise
    /// none.
    pub fn access_decision(&self, hosts: &[String]) -> Option<AccessDecision> {
        let settings = self.access_settings();
        let listed = |decision, host: &String| settings.has_host(decision, host);
        if !hosts.is_empty() && hosts.iter().all(|host| listed(AccessDecision::Allow, host)) {
            Some(AccessDecision::Allow)
        } else if hosts.iter().any(|host| listed(AccessDecision::Deny, host)) {
            Some(AccessDecision::Deny)
        } else {
            None
        }
    }

    /// The Access decisions remembered on the Entry: the hosts allowed, then those denied.
    pub fn remembered_access(&self) -> Vec<RememberedAccess> {
        let settings = self.access_settings();
        [AccessDecision::Allow, AccessDecision::Deny]
            .into_iter()
            .flat_map(|decision| {
                settings.hosts(decision).map(move |host| RememberedAccess {
                    host: host.to_owned(),
                    decision,
                })
            })
            .collect()
    }

    fn access_settings(&self) -> AccessSettings {
        let settings = self
            .custom_data
            .get(ACCESS_KEY)
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        AccessSettings(settings)
    }
}

/// An Entry's KeePassXC-Browser settings: its allowed and denied hosts, and whatever else
/// KeePassXC keeps there, which is written back untouched.
#[derive(Default)]
struct AccessSettings(Map<String, Value>);

impl AccessSettings {
    fn hosts(&self, decision: AccessDecision) -> impl Iterator<Item = &str> {
        self.0
            .get(decision.list())
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
    }

    fn has_host(&self, decision: AccessDecision, host: &str) -> bool {
        self.hosts(decision).any(|listed| listed == host)
    }

    fn insert_host(&mut self, decision: AccessDecision, host: &str) {
        if !self.has_host(decision, host) {
            let mut hosts: Vec<Value> = self.hosts(decision).map(Value::from).collect();
            hosts.push(host.into());
            self.set_hosts(decision, hosts);
        }
    }

    fn remove_host(&mut self, decision: AccessDecision, host: &str) {
        let hosts = self
            .hosts(decision)
            .filter(|listed| *listed != host)
            .map(Value::from)
            .collect();
        self.set_hosts(decision, hosts);
    }

    fn set_hosts(&mut self, decision: AccessDecision, hosts: Vec<Value>) {
        self.0
            .insert(decision.list().to_owned(), Value::Array(hosts));
    }

    /// The settings as KeePassXC writes them: compact JSON with every one of its keys.
    fn into_json(mut self) -> String {
        for decision in [AccessDecision::Allow, AccessDecision::Deny] {
            if !self.0.contains_key(decision.list()) {
                self.set_hosts(decision, Vec::new());
            }
        }
        self.0.entry("Realm").or_insert_with(|| Value::from(""));
        Value::Object(self.0).to_string()
    }
}

/// Sets the custom data item `key` to `value`, adding it if there is none.
/// Sets the custom data item with `key`, adding it if there is none, and returns it.
fn set_custom_data<'a>(custom_data: &'a mut Element, key: &str, value: &str) -> &'a mut Element {
    let index = custom_data
        .children
        .iter()
        .position(|node| is_item(node, key));
    let index = index.unwrap_or_else(|| {
        push(custom_data, custom_data_item(key, value));
        custom_data.children.len() - 1
    });
    let Node::Element(item) = &mut custom_data.children[index] else {
        unreachable!("found or pushed an element")
    };
    child_or_insert(item, "Value", &["Key"]).set_text(value);
    item
}

/// Removes the custom data item `key`, if there is one.
fn remove_custom_data(custom_data: &mut Element, key: &str) {
    custom_data.children.retain(|node| !is_item(node, key));
}

/// Whether `node` is the custom data item `key`.
fn is_item(node: &Node, key: &str) -> bool {
    matches!(node, Node::Element(item)
        if item.name == "Item" && item.child("Key").is_some_and(|k| k.text() == key))
}

fn custom_data_item(key: &str, value: &str) -> Element {
    let mut item = Element::new("Item");
    push(&mut item, text_element("Key", key));
    push(&mut item, text_element("Value", value));
    item
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
