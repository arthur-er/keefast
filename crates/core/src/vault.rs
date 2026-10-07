use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::SecretString;
use uuid::Uuid;

use crate::OpenError;
use crate::kdbx::{Key, VaultFile};
use crate::xml::Element;

/// An opened, decrypted Vault: a tree of Groups holding Entries.
///
/// The Vault keeps the whole Vault file it was opened from, so saving it writes back everything
/// in it, including data keefast doesn't model (ADR 0002).
pub struct Vault {
    root: Group,
    pub(crate) file: VaultFile,
    /// The hash of the composite key, which saving needs to re-encrypt the Vault file.
    pub(crate) key: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(Uuid);

#[derive(Debug)]
pub struct Group {
    id: GroupId,
    name: String,
    groups: Vec<Group>,
    entries: Vec<Entry>,
}

pub struct Entry {
    id: EntryId,
    title: String,
    username: String,
    password: SecretString,
    url: String,
    notes: String,
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
    BASE64
        .decode(text.trim())
        .ok()
        .and_then(|bytes| Uuid::from_slice(&bytes).ok())
        .ok_or_else(|| OpenError::Corrupt(format!("invalid UUID {text:?}")))
}

impl Group {
    fn from_element(group: &Element) -> Result<Self, OpenError> {
        Ok(Self {
            id: GroupId(uuid(group)?),
            name: group.child("Name").map(Element::text).unwrap_or_default(),
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
        let field = |name: &str| {
            entry
                .children_named("String")
                .find(|string| string.child("Key").is_some_and(|key| key.text() == name))
                .and_then(|string| string.child("Value"))
                .map(Element::text)
                .unwrap_or_default()
        };
        Ok(Self {
            id: EntryId(uuid(entry)?),
            title: field("Title"),
            username: field("UserName"),
            password: SecretString::from(field("Password")),
            url: field("URL"),
            notes: field("Notes"),
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
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("username", &self.username)
            .field("password", &"[hidden]")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}
