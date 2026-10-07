use std::fmt;

use keepass::db::{GroupRef, fields};
use secrecy::SecretString;
use uuid::Uuid;

/// An opened, decrypted Vault: a tree of Groups holding Entries.
#[derive(Debug)]
pub struct Vault {
    root: Group,
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
    pub(crate) fn from_database(database: &keepass::Database) -> Self {
        Self {
            root: Group::from_ref(&database.root()),
        }
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

impl Group {
    fn from_ref(group: &GroupRef<'_>) -> Self {
        Self {
            id: GroupId(group.id().uuid()),
            name: group.name.clone(),
            groups: group
                .groups()
                .map(|child| Group::from_ref(&child))
                .collect(),
            entries: group
                .entries()
                .map(|entry| {
                    let field = |name| entry.get(name).unwrap_or_default().to_owned();
                    Entry {
                        id: EntryId(entry.id().uuid()),
                        title: field(fields::TITLE),
                        username: field(fields::USERNAME),
                        password: SecretString::from(field(fields::PASSWORD)),
                        url: field(fields::URL),
                        notes: field(fields::NOTES),
                    }
                })
                .collect(),
        }
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
