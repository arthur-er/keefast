//! The Vault's side of browser integration: its identity in the KeePassXC-Browser protocol, the
//! Pairings stored in it, the Entries it offers a site, and the Access decisions remembered on
//! them.
//!
//! All of it lives in custom data under KeePassXC's keys (ADR 0005), so a Vault paired or
//! decided on in KeePassXC works in keefast and the other way round.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::edit::{Kind, child_or_insert, push, set_time, text_element};
use crate::xml::Element;
use crate::{EditError, Entry, EntryId, Group, Site, Timestamp, Vault};

/// KeePassXC stores each Pairing's identification key under this prefix plus the Pairing's name.
const PAIRING_KEY_PREFIX: &str = "KPXC_BROWSER_";
/// KeePassXC records when a Pairing was made under this prefix plus the Pairing's name.
const PAIRING_CREATED_PREFIX: &str = "_CREATED_";
/// KeePassXC remembers an Entry's Access decisions in its custom data under this key, as JSON:
/// `{"Allow":[hosts],"Deny":[hosts],"Realm":""}`.
const ACCESS_KEY: &str = "KeePassXC-Browser Settings";
/// An Entry, or a Group's Entries and subgroups, are hidden from browsers when their custom data
/// has this key set to `true`. A Group with it set to anything else shows its subgroups again.
const HIDE_KEY: &str = "BrowserHideEntry";

/// A remembered answer to a site asking for an Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    Allow,
    Deny,
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
        let custom_data = self.custom_data_mut();
        push(
            custom_data,
            custom_data_item(&format!("{PAIRING_KEY_PREFIX}{name}"), key),
        );
        push(
            custom_data,
            custom_data_item(
                &format!("{PAIRING_CREATED_PREFIX}{name}"),
                &now.to_iso_8601(),
            ),
        );
        Ok(())
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

    fn custom_data_mut(&mut self) -> &mut Element {
        let meta = child_or_insert(&mut self.file.document, "Meta", &[]);
        child_or_insert(meta, "CustomData", &[])
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
        let (add, remove) = match decision {
            AccessDecision::Allow => ("Allow", "Deny"),
            AccessDecision::Deny => ("Deny", "Allow"),
        };
        for host in hosts {
            settings.insert_host(add, host);
            settings.remove_host(remove, host);
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
        if !hosts.is_empty() && hosts.iter().all(|host| settings.has_host("Allow", host)) {
            Some(AccessDecision::Allow)
        } else if hosts.iter().any(|host| settings.has_host("Deny", host)) {
            Some(AccessDecision::Deny)
        } else {
            None
        }
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
    fn hosts(&self, list: &str) -> impl Iterator<Item = &str> {
        self.0
            .get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
    }

    fn has_host(&self, list: &str, host: &str) -> bool {
        self.hosts(list).any(|listed| listed == host)
    }

    fn insert_host(&mut self, list: &str, host: &str) {
        if !self.has_host(list, host) {
            let mut hosts: Vec<&str> = self.hosts(list).collect();
            hosts.push(host);
            let hosts = hosts.into_iter().map(Value::from).collect();
            self.0.insert(list.to_owned(), Value::Array(hosts));
        }
    }

    fn remove_host(&mut self, list: &str, host: &str) {
        let hosts = self
            .hosts(list)
            .filter(|listed| *listed != host)
            .map(Value::from)
            .collect();
        self.0.insert(list.to_owned(), Value::Array(hosts));
    }

    /// The settings as KeePassXC writes them: compact JSON with every one of its keys.
    fn into_json(mut self) -> String {
        for list in ["Allow", "Deny"] {
            self.0
                .entry(list)
                .or_insert_with(|| Value::Array(Vec::new()));
        }
        self.0.entry("Realm").or_insert_with(|| Value::from(""));
        Value::Object(self.0).to_string()
    }
}

/// Sets the custom data item `key` to `value`, adding it if there is none.
fn set_custom_data(custom_data: &mut Element, key: &str, value: &str) {
    let existing = custom_data
        .elements_mut()
        .find(|item| item.name == "Item" && item.child("Key").is_some_and(|k| k.text() == key));
    match existing {
        Some(item) => child_or_insert(item, "Value", &["Key"]).set_text(value),
        None => push(custom_data, custom_data_item(key, value)),
    }
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
