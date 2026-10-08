use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::SecretString;
use uuid::Uuid;

use crate::kdbx::{Key, VaultFile};
use crate::xml::Element;
use crate::{OpenError, Timestamp};

/// An opened, decrypted Vault: a tree of Groups holding Entries.
///
/// The Vault keeps the whole Vault file it was opened from, so saving it writes back everything
/// in it, including data keefast doesn't model (ADR 0002). Edits change that document, and the
/// Groups and Entries are read back from it.
#[derive(Clone)]
pub struct Vault {
    root: Group,
    pub(crate) file: VaultFile,
    /// The hash of the composite key, which saving needs to re-encrypt the Vault file.
    pub(crate) key: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(pub(crate) Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(pub(crate) Uuid);

impl EntryId {
    /// The UUID in lowercase hex without dashes, as KeePassXC shows it to browsers.
    pub fn to_hex(self) -> String {
        self.0.simple().to_string()
    }
}

/// KeePassXC reads every field whose name starts with this as another URL for the Entry.
const ADDITIONAL_URL_PREFIX: &str = "KP2A_URL";

#[derive(Debug, Clone)]
pub struct Group {
    id: GroupId,
    name: String,
    last_modified: Option<Timestamp>,
    pub(crate) custom_data: BTreeMap<String, String>,
    groups: Vec<Group>,
    entries: Vec<Entry>,
}

#[derive(Clone)]
pub struct Entry {
    id: EntryId,
    title: String,
    username: String,
    password: SecretString,
    url: String,
    notes: String,
    /// More URLs the Entry is for, as KeePassXC stores them: fields named `KP2A_URL…`.
    additional_urls: Vec<String>,
    pub(crate) custom_data: BTreeMap<String, String>,
    last_modified: Option<Timestamp>,
    /// Earlier versions, oldest first.
    history: Vec<Entry>,
}

impl Vault {
    pub(crate) fn from_file(file: VaultFile, key: Key) -> Result<Self, OpenError> {
        let group = file
            .document
            .child("Root")
            .and_then(|root| root.child("Group"))
            .ok_or_else(|| OpenError::Corrupt("the Vault has no root Group".into()))?;
        Ok(Self {
            root: Group::from_element(group)?,
            file,
            key,
        })
    }

    /// Reads the Groups and Entries back from the document after an edit.
    pub(crate) fn refresh(&mut self) {
        self.root = Group::from_element(self.root_group()).expect("edits keep every UUID valid");
    }

    /// The root Group's element in the document.
    pub(crate) fn root_group(&self) -> &Element {
        self.file
            .document
            .child("Root")
            .and_then(|root| root.child("Group"))
            .expect("a Vault always has a root Group")
    }

    pub fn root(&self) -> &Group {
        &self.root
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.root.find_group(id)
    }

    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.root.all_entries().find(|entry| entry.id == id)
    }
}

/// Two Vaults are equal when they hold the same content: the same document, protected values
/// and attachments, whatever keys and seeds they are encrypted with.
impl PartialEq for Vault {
    fn eq(&self, other: &Self) -> bool {
        self.file.document == other.file.document && self.file.binaries == other.file.binaries
    }
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

fn uuid(element: &Element) -> Result<Uuid, OpenError> {
    let text = element.child("UUID").map(Element::text).unwrap_or_default();
    decode_uuid(&text).ok_or_else(|| OpenError::Corrupt(format!("invalid UUID {text:?}")))
}

pub(crate) fn decode_uuid(text: &str) -> Option<Uuid> {
    BASE64
        .decode(text.trim())
        .ok()
        .and_then(|bytes| Uuid::from_slice(&bytes).ok())
}

pub(crate) fn encode_uuid(uuid: Uuid) -> String {
    BASE64.encode(uuid.as_bytes())
}

/// An Entry's or Group's `CustomData` items, by key.
fn custom_data(element: &Element) -> BTreeMap<String, String> {
    element
        .child("CustomData")
        .into_iter()
        .flat_map(|data| data.children_named("Item"))
        .map(|item| {
            let text = |name| item.child(name).map(Element::text).unwrap_or_default();
            (text("Key"), text("Value"))
        })
        .collect()
}

/// When an Entry or Group was last modified, from its `Times`.
fn last_modified(element: &Element) -> Option<Timestamp> {
    let time = element.child("Times")?.child("LastModificationTime")?;
    Timestamp::decode(&time.text())
}

impl Group {
    fn from_element(group: &Element) -> Result<Self, OpenError> {
        Ok(Self {
            id: GroupId(uuid(group)?),
            name: group.child("Name").map(Element::text).unwrap_or_default(),
            last_modified: last_modified(group),
            custom_data: custom_data(group),
            groups: group
                .children_named("Group")
                .map(Group::from_element)
                .collect::<Result<_, _>>()?,
            entries: group
                .children_named("Entry")
                .map(Entry::from_element)
                .collect::<Result<_, _>>()?,
        })
    }

    pub fn id(&self) -> GroupId {
        self.id
    }

    pub fn last_modified(&self) -> Option<Timestamp> {
        self.last_modified
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Direct child Groups, in Vault file order.
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Entries directly in this Group, in Vault file order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Entries in this Group and all its descendants, depth first.
    pub fn all_entries(&self) -> impl Iterator<Item = &Entry> {
        let nested: Box<dyn Iterator<Item = &Entry>> =
            Box::new(self.groups.iter().flat_map(Group::all_entries));
        self.entries.iter().chain(nested)
    }

    fn find_group(&self, id: GroupId) -> Option<&Group> {
        if self.id == id {
            return Some(self);
        }
        self.groups.iter().find_map(|child| child.find_group(id))
    }
}

impl Entry {
    fn from_element(entry: &Element) -> Result<Self, OpenError> {
        // Each field's name and value element, the value read only when wanted.
        let strings = || {
            entry.children_named("String").map(|string| {
                let key = string.child("Key").map(Element::text).unwrap_or_default();
                (key, string.child("Value"))
            })
        };
        let value = |value: Option<&Element>| value.map(Element::text).unwrap_or_default();
        let field = |name: &str| {
            strings()
                .find(|(key, _)| key == name)
                .map(|(_, element)| value(element))
                .unwrap_or_default()
        };
        Ok(Self {
            id: EntryId(uuid(entry)?),
            title: field("Title"),
            username: field("UserName"),
            password: SecretString::from(field("Password")),
            url: field("URL"),
            notes: field("Notes"),
            additional_urls: strings()
                .filter(|(key, _)| key.starts_with(ADDITIONAL_URL_PREFIX))
                .map(|(_, element)| value(element))
                .filter(|url| !url.is_empty())
                .collect(),
            custom_data: custom_data(entry),
            last_modified: last_modified(entry),
            history: entry
                .child("History")
                .map(|history| {
                    history
                        .children_named("Entry")
                        .map(Entry::from_element)
                        .collect::<Result<_, _>>()
                })
                .transpose()?
                .unwrap_or_default(),
        })
    }

    pub fn id(&self) -> EntryId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &SecretString {
        &self.password
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn notes(&self) -> &str {
        &self.notes
    }

    /// The other URLs the Entry is for, besides its URL.
    pub fn additional_urls(&self) -> &[String] {
        &self.additional_urls
    }

    pub fn last_modified(&self) -> Option<Timestamp> {
        self.last_modified
    }

    /// The Entry's earlier versions, oldest first.
    pub fn history(&self) -> &[Entry] {
        &self.history
    }
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("username", &self.username)
            .field("password", &"[hidden]")
            .field("url", &self.url)
            .field("history", &self.history)
            .finish_non_exhaustive()
    }
}
