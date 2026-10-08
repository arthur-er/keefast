//! Commands that change a Vault: creating, editing, moving and deleting Entries and Groups.
//!
//! Each command edits the Vault's XML document in place, as KeePass would, and touches nothing
//! it doesn't need to, so everything keefast doesn't model survives (ADR 0002).

use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::kdbx::random;
use crate::totp::{self, TOTP_FIELDS};
use crate::vault::{decode_uuid, encode_uuid};
use crate::xml::{Element, Node, Secret};
use crate::{Entry, EntryId, GroupId, Timestamp, Totp, Vault};

/// The fields of an Entry that keefast edits.
#[derive(Clone)]
pub struct EntryFields {
    pub title: String,
    pub username: String,
    pub password: SecretString,
    pub url: String,
    pub notes: String,
}

impl Default for EntryFields {
    fn default() -> Self {
        Self {
            title: String::new(),
            username: String::new(),
            password: SecretString::from(String::new()),
            url: String::new(),
            notes: String::new(),
        }
    }
}

/// Why an edit could not be made. The Vault is unchanged when one is returned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("the Entry is not in this Vault")]
    EntryNotFound,
    #[error("the Group is not in this Vault")]
    GroupNotFound,
    #[error("the root Group cannot be moved or deleted")]
    RootGroup,
    #[error("a Group cannot be moved into itself or one of its subgroups")]
    IntoItself,
    #[error("a Pairing needs a name")]
    PairingNameEmpty,
    #[error("the Vault already has a Pairing with this name")]
    PairingNameTaken,
}

/// The standard Entry fields as KeePass names them, with the `MemoryProtection` setting that
/// decides whether a new value is stored protected.
const FIELDS: [(&str, &str); 5] = [
    ("Title", "ProtectTitle"),
    ("UserName", "ProtectUserName"),
    ("Password", "ProtectPassword"),
    ("URL", "ProtectURL"),
    ("Notes", "ProtectNotes"),
];

/// KeePass's defaults, used when the Vault's `Meta` doesn't say.
const DEFAULT_HISTORY_MAX_ITEMS: i64 = 10;
const FOLDER_ICON: &str = "48";
const RECYCLE_BIN_ICON: &str = "43";
const RECYCLE_BIN_NAME: &str = "Recycle Bin";
const NO_UUID: Uuid = Uuid::nil();

impl From<&Entry> for EntryFields {
    fn from(entry: &Entry) -> Self {
        Self {
            title: entry.title().to_owned(),
            username: entry.username().to_owned(),
            password: entry.password().clone(),
            url: entry.url().to_owned(),
            notes: entry.notes().to_owned(),
        }
    }
}

impl EntryFields {
    fn values(&self) -> [&str; 5] {
        [
            &self.title,
            &self.username,
            self.password.expose_secret(),
            &self.url,
            &self.notes,
        ]
    }
}

impl Vault {
    /// The Group deleted Entries and Groups go to, if the Vault has one.
    pub fn recycle_bin(&self) -> Option<GroupId> {
        let id = self.recycle_bin_uuid()?;
        self.group(GroupId(id)).map(|group| group.id())
    }

    /// Whether the Group is the Recycle Bin or in it.
    pub fn is_group_in_recycle_bin(&self, group: GroupId) -> Result<bool, EditError> {
        let path = self.path_to(Kind::Group, group.0)?;
        Ok(self
            .recycle_bin_path()
            .is_some_and(|bin| path.starts_with(&bin)))
    }

    pub fn create_entry(
        &mut self,
        group: GroupId,
        fields: &EntryFields,
        now: Timestamp,
    ) -> Result<EntryId, EditError> {
        let path = self.path_to(Kind::Group, group.0)?;
        let id = new_uuid();
        let mut entry = Element::new("Entry");
        push(&mut entry, text_element("UUID", encode_uuid(id)));
        push(&mut entry, text_element("IconID", "0"));
        for name in ["ForegroundColor", "BackgroundColor", "OverrideURL", "Tags"] {
            push(&mut entry, Element::new(name));
        }
        push(&mut entry, times(now));
        for ((key, protect), value) in FIELDS.into_iter().zip(fields.values()) {
            push(&mut entry, new_string(key, value, self.protects(protect)));
        }
        let mut auto_type = Element::new("AutoType");
        push(&mut auto_type, text_element("Enabled", "True"));
        push(&mut auto_type, text_element("DataTransferObfuscation", "0"));
        push(&mut entry, auto_type);
        push(&mut entry, Element::new("History"));

        insert_entry(self.at_mut(&path), entry);
        self.refresh();
        Ok(EntryId(id))
    }

    /// Sets an Entry's fields. The version it replaces goes to the Entry's history, which is
    /// trimmed to the Vault's `HistoryMaxItems`. Setting the fields it already has changes
    /// nothing.
    pub fn update_entry(
        &mut self,
        entry: EntryId,
        fields: &EntryFields,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let path = self.path_to(Kind::Entry, entry.0)?;
        let unchanged = self
            .entry(entry)
            .is_some_and(|current| EntryFields::from(current).values() == fields.values());
        if unchanged {
            return Ok(());
        }
        let protection = FIELDS.map(|(_, protect)| self.protects(protect));
        let element = self.archive(&path);
        for (((key, _), protect), value) in FIELDS.into_iter().zip(protection).zip(fields.values())
        {
            set_string(element, key, value, protect);
        }
        set_time(element, "LastModificationTime", now);
        set_time(element, "LastAccessTime", now);
        self.refresh();
        Ok(())
    }

    /// Sets up TOTP on an Entry, or removes it with `None`. The settings go to the fields the
    /// Entry already keeps TOTP in, so every client keeps reading them, or to an `otpauth://`
    /// URI in `otp` as KeePassXC and KeeWeb write it. The version replaced goes to the Entry's
    /// history. Setting the TOTP it already has changes nothing.
    pub fn set_totp(
        &mut self,
        entry: EntryId,
        totp: Option<&Totp>,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let path = self.path_to(Kind::Entry, entry.0)?;
        let current = self.entry(entry).expect("the path leads to it");
        let unchanged = match (current.totp(), totp) {
            (None, None) => true,
            (Some(Ok(current)), Some(new)) => current == new,
            _ => false,
        };
        if unchanged {
            return Ok(());
        }
        let (title, username) = (current.title().to_owned(), current.username().to_owned());

        let element = self.archive(&path);
        let fields = totp.map(|totp| {
            let field = |name: &str| string_value(element, name);
            totp::write(totp, &field, &title, &username)
        });
        element.children.retain(|node| {
            let Node::Element(string) = node else {
                return true;
            };
            let key = string.child("Key").map(Element::text);
            !(string.name == "String" && key.is_some_and(|key| TOTP_FIELDS.contains(&&*key)))
        });
        for field in fields.into_iter().flatten() {
            set_string(element, field.name, &field.value, field.protect);
        }
        set_time(element, "LastModificationTime", now);
        set_time(element, "LastAccessTime", now);
        self.refresh();
        Ok(())
    }

    /// Adds the Entry at `path`, as it is now, to its history, which is trimmed to the Vault's
    /// `HistoryMaxItems`, and returns the Entry to change.
    fn archive(&mut self, path: &[usize]) -> &mut Element {
        let max_items = self.history_max_items();
        let element = self.at_mut(path);
        let mut previous = element.clone();
        previous.take_child("History");
        let history = child_or_insert(element, "History", &[]);
        history.children.push(Node::Element(previous));
        if let Some(max) = max_items {
            let mut excess = history.children_named("Entry").count().saturating_sub(max);
            history.children.retain(|node| {
                let is_entry = matches!(node, Node::Element(e) if e.name == "Entry");
                let drop = is_entry && excess > 0;
                excess -= usize::from(drop);
                !drop
            });
        }
        element
    }

    pub fn move_entry(
        &mut self,
        entry: EntryId,
        to: GroupId,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let from = self.path_to(Kind::Entry, entry.0)?;
        self.path_to(Kind::Group, to.0)?;
        let mut element = self.take(&from);
        set_time(&mut element, "LocationChanged", now);
        let to = self.path_to(Kind::Group, to.0)?;
        insert_entry(self.at_mut(&to), element);
        self.refresh();
        Ok(())
    }

    /// Whether [`Self::delete_entry`] would delete the Entry for good rather than move it to
    /// the Recycle Bin.
    pub fn deletes_entry_for_good(&self, entry: EntryId) -> Result<bool, EditError> {
        let path = self.path_to(Kind::Entry, entry.0)?;
        Ok(!self.recycle_bin_enabled() || self.in_recycle_bin(&path))
    }

    /// Moves the Entry to the Recycle Bin when the Vault has one enabled, and deletes it for
    /// good otherwise, or when it is already in the Recycle Bin.
    pub fn delete_entry(&mut self, entry: EntryId, now: Timestamp) -> Result<(), EditError> {
        if !self.deletes_entry_for_good(entry)? {
            let bin = self.ensure_recycle_bin(now);
            return self.move_entry(entry, bin, now);
        }
        let path = self.path_to(Kind::Entry, entry.0)?;
        let element = self.take(&path);
        self.record_deletion(&element, now);
        self.refresh();
        Ok(())
    }

    pub fn create_group(
        &mut self,
        parent: GroupId,
        name: &str,
        now: Timestamp,
    ) -> Result<GroupId, EditError> {
        let path = self.path_to(Kind::Group, parent.0)?;
        let id = new_uuid();
        push(self.at_mut(&path), new_group(id, name, FOLDER_ICON, now));
        self.refresh();
        Ok(GroupId(id))
    }

    pub fn rename_group(
        &mut self,
        group: GroupId,
        name: &str,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let path = self.path_to(Kind::Group, group.0)?;
        let element = self.at_mut(&path);
        set_value(child_or_insert(element, "Name", &["UUID"]), name, false);
        set_time(element, "LastModificationTime", now);
        set_time(element, "LastAccessTime", now);
        self.refresh();
        Ok(())
    }

    /// Moves a Group, with everything in it, into another Group.
    pub fn move_group(
        &mut self,
        group: GroupId,
        to: GroupId,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let from = self.path_to(Kind::Group, group.0)?;
        let target = self.path_to(Kind::Group, to.0)?;
        if from.is_empty() {
            return Err(EditError::RootGroup);
        }
        if target.starts_with(&from) {
            return Err(EditError::IntoItself);
        }
        let mut element = self.take(&from);
        set_time(&mut element, "LocationChanged", now);
        let target = self.path_to(Kind::Group, to.0)?;
        push(self.at_mut(&target), element);
        self.refresh();
        Ok(())
    }

    /// Whether [`Self::delete_group`] would delete the Group for good rather than move it to
    /// the Recycle Bin.
    pub fn deletes_group_for_good(&self, group: GroupId) -> Result<bool, EditError> {
        let path = self.path_to(Kind::Group, group.0)?;
        if path.is_empty() {
            return Err(EditError::RootGroup);
        }
        let holds_bin = self
            .recycle_bin_path()
            .is_some_and(|bin| bin.starts_with(&path));
        Ok(!self.recycle_bin_enabled() || holds_bin || self.in_recycle_bin(&path))
    }

    /// Moves the Group to the Recycle Bin when the Vault has one enabled, and deletes it and
    /// everything in it for good otherwise, or when it is in the Recycle Bin, is the Recycle
    /// Bin, or holds it.
    pub fn delete_group(&mut self, group: GroupId, now: Timestamp) -> Result<(), EditError> {
        if !self.deletes_group_for_good(group)? {
            let bin = self.ensure_recycle_bin(now);
            return self.move_group(group, bin, now);
        }
        let path = self.path_to(Kind::Group, group.0)?;
        let element = self.take(&path);
        self.record_deletion(&element, now);
        self.refresh();
        Ok(())
    }

    // Paths are the indices into `children`, from the root Group down to an element.

    fn root_group_mut(&mut self) -> &mut Element {
        self.file
            .document
            .child_mut("Root")
            .and_then(|root| root.child_mut("Group"))
            .expect("a Vault always has a root Group")
    }

    pub(crate) fn path_to(&self, kind: Kind, id: Uuid) -> Result<Vec<usize>, EditError> {
        let mut path = Vec::new();
        if find(self.root_group(), kind, id, &mut path) {
            Ok(path)
        } else {
            Err(kind.not_found())
        }
    }

    pub(crate) fn at_mut(&mut self, path: &[usize]) -> &mut Element {
        let mut element = self.root_group_mut();
        for &index in path {
            element = match &mut element.children[index] {
                Node::Element(child) => child,
                _ => unreachable!("paths lead through elements"),
            };
        }
        element
    }

    fn take(&mut self, path: &[usize]) -> Element {
        let (&last, parent) = path.split_last().expect("the root Group is never taken");
        match self.at_mut(parent).children.remove(last) {
            Node::Element(element) => element,
            _ => unreachable!("paths lead to elements"),
        }
    }

    fn meta(&self) -> Option<&Element> {
        self.file.document.child("Meta")
    }

    fn meta_text(&self, name: &str) -> Option<String> {
        Some(self.meta()?.child(name)?.text())
    }

    fn protects(&self, setting: &str) -> bool {
        let protection = self
            .meta()
            .and_then(|meta| meta.child("MemoryProtection"))
            .and_then(|protection| protection.child(setting))
            .map(Element::text);
        match protection {
            Some(value) => is_true(&value),
            // KeePass protects only passwords by default.
            None => setting == "ProtectPassword",
        }
    }

    /// `None` when the history is unlimited.
    fn history_max_items(&self) -> Option<usize> {
        let max = self
            .meta_text("HistoryMaxItems")
            .and_then(|text| text.trim().parse::<i64>().ok())
            .unwrap_or(DEFAULT_HISTORY_MAX_ITEMS);
        usize::try_from(max).ok()
    }

    fn recycle_bin_enabled(&self) -> bool {
        self.meta_text("RecycleBinEnabled")
            .is_none_or(|text| is_true(&text))
    }

    fn recycle_bin_uuid(&self) -> Option<Uuid> {
        decode_uuid(&self.meta_text("RecycleBinUUID")?).filter(|id| *id != NO_UUID)
    }

    fn recycle_bin_path(&self) -> Option<Vec<usize>> {
        self.path_to(Kind::Group, self.recycle_bin_uuid()?).ok()
    }

    fn in_recycle_bin(&self, path: &[usize]) -> bool {
        self.recycle_bin_path()
            .is_some_and(|bin| path.len() > bin.len() && path.starts_with(&bin))
    }

    /// The Recycle Bin, created at the top of the Vault if it doesn't exist yet.
    fn ensure_recycle_bin(&mut self, now: Timestamp) -> GroupId {
        if let Some(id) = self.recycle_bin_path().and(self.recycle_bin_uuid()) {
            return GroupId(id);
        }
        let id = new_uuid();
        let mut bin = new_group(id, RECYCLE_BIN_NAME, RECYCLE_BIN_ICON, now);
        for setting in ["EnableAutoType", "EnableSearching"] {
            if let Some(element) = bin.child_mut(setting) {
                element.set_text("false");
            }
        }
        push(self.root_group_mut(), bin);
        let meta = child_or_insert(&mut self.file.document, "Meta", &[]);
        set_value(
            child_or_insert(meta, "RecycleBinUUID", &["RecycleBinEnabled"]),
            &encode_uuid(id),
            false,
        );
        set_value(
            child_or_insert(meta, "RecycleBinChanged", &["RecycleBinUUID"]),
            &now.encode(),
            false,
        );
        self.refresh();
        GroupId(id)
    }

    /// Lists a permanently deleted Entry or Group, and everything in it, in `DeletedObjects`,
    /// so a Merge with another copy of the Vault file deletes it there too.
    fn record_deletion(&mut self, element: &Element, now: Timestamp) {
        let mut deleted = Vec::new();
        collect_uuids(element, &mut deleted);
        let root = child_or_insert(&mut self.file.document, "Root", &[]);
        let objects = child_or_insert(root, "DeletedObjects", &["Group"]);
        for uuid in deleted {
            let mut object = Element::new("DeletedObject");
            push(&mut object, text_element("UUID", uuid));
            push(&mut object, text_element("DeletionTime", now.encode()));
            push(objects, object);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Entry,
    Group,
}

impl Kind {
    fn not_found(self) -> EditError {
        match self {
            Kind::Entry => EditError::EntryNotFound,
            Kind::Group => EditError::GroupNotFound,
        }
    }
}

/// Searches `group` and its descendants (not Entry histories), pushing the path on success.
fn find(group: &Element, kind: Kind, id: Uuid, path: &mut Vec<usize>) -> bool {
    if matches!(kind, Kind::Group) && has_uuid(group, id) {
        return true;
    }
    for (index, node) in group.children.iter().enumerate() {
        let Node::Element(child) = node else {
            continue;
        };
        path.push(index);
        let found = match child.name.as_str() {
            "Entry" => matches!(kind, Kind::Entry) && has_uuid(child, id),
            "Group" => find(child, kind, id, path),
            _ => false,
        };
        if found {
            return true;
        }
        path.pop();
    }
    false
}

fn has_uuid(element: &Element, id: Uuid) -> bool {
    element
        .child("UUID")
        .and_then(|uuid| decode_uuid(&uuid.text()))
        == Some(id)
}

fn collect_uuids(element: &Element, out: &mut Vec<String>) {
    if let Some(uuid) = element.child("UUID") {
        out.push(uuid.text());
    }
    for child in element.elements() {
        if child.name == "Entry" || child.name == "Group" {
            collect_uuids(child, out);
        }
    }
}

fn new_uuid() -> Uuid {
    uuid::Builder::from_random_bytes(random()).into_uuid()
}

fn is_true(text: &str) -> bool {
    text.trim().eq_ignore_ascii_case("true")
}

pub(crate) fn push(parent: &mut Element, child: Element) {
    parent.children.push(Node::Element(child));
}

pub(crate) fn text_element(name: &str, text: impl Into<String>) -> Element {
    let mut element = Element::new(name);
    set_value(&mut element, &text.into(), false);
    element
}

/// Sets an element's content, protected or not. An empty unprotected value has no content at
/// all, as when read back from `<Value/>`.
fn set_value(element: &mut Element, value: &str, protect: bool) {
    element.attributes.retain(|(name, _)| name != "Protected");
    element.children.clear();
    if protect {
        element.attributes.push(("Protected".into(), "True".into()));
        element
            .children
            .push(Node::Protected(Secret::new(value.as_bytes().to_vec())));
    } else if !value.is_empty() {
        element.children.push(Node::Text(value.into()));
    }
}

/// Sets a standard field on an Entry element. A field that exists keeps its own protection
/// if it is protected; otherwise the Vault's `MemoryProtection` decides.
fn set_string(entry: &mut Element, key: &str, value: &str, protect: bool) {
    let existing = entry.elements_mut().find(|string| {
        string.name == "String" && string.child("Key").is_some_and(|k| k.text() == key)
    });
    match existing {
        Some(string) => {
            let value_element = child_or_insert(string, "Value", &["Key"]);
            let protect = protect || value_element.is_protected();
            set_value(value_element, value, protect);
        }
        None => insert_after_last(
            entry,
            new_string(key, value, protect),
            &["String", "Times", "Tags", "UUID"],
        ),
    }
}

/// The value of an Entry element's field, if it has one that isn't blank.
pub(crate) fn string_value(entry: &Element, key: &str) -> Option<Zeroizing<String>> {
    entry
        .children_named("String")
        .find(|string| string.child("Key").is_some_and(|k| k.text() == key))
        .and_then(|string| string.child("Value"))
        .map(|value| Zeroizing::new(value.text()))
        .filter(|value| !value.trim().is_empty())
}

/// A `<String>` field of an Entry.
fn new_string(key: &str, value: &str, protect: bool) -> Element {
    let mut string = Element::new("String");
    push(&mut string, text_element("Key", key));
    let mut element = Element::new("Value");
    set_value(&mut element, value, protect);
    push(&mut string, element);
    string
}

pub(crate) fn set_time(element: &mut Element, name: &str, time: Timestamp) {
    let times = child_or_insert(element, "Times", &["Tags", "IconID", "Name", "UUID"]);
    set_value(child_or_insert(times, name, &[]), &time.encode(), false);
}

/// The first child called `name`, inserted after the last of the `after` elements present
/// (or at the end) if there is none.
pub(crate) fn child_or_insert<'a>(
    parent: &'a mut Element,
    name: &str,
    after: &[&str],
) -> &'a mut Element {
    if parent.child(name).is_none() {
        insert_after_last(parent, Element::new(name), after);
    }
    parent.child_mut(name).expect("just inserted")
}

/// Inserts `child` after the last child element named in `after`, trying the names in order,
/// or at the end.
fn insert_after_last(parent: &mut Element, child: Element, after: &[&str]) {
    let index = after
        .iter()
        .find_map(|name| {
            parent
                .children
                .iter()
                .rposition(|node| matches!(node, Node::Element(e) if e.name == *name))
        })
        .map_or(parent.children.len(), |index| index + 1);
    parent.children.insert(index, Node::Element(child));
}

/// KeePass lists a Group's Entries before its subgroups.
fn insert_entry(group: &mut Element, entry: Element) {
    let index = group
        .children
        .iter()
        .rposition(|node| matches!(node, Node::Element(e) if e.name == "Entry"))
        .map(|index| index + 1)
        .or_else(|| {
            group
                .children
                .iter()
                .position(|node| matches!(node, Node::Element(e) if e.name == "Group"))
        })
        .unwrap_or(group.children.len());
    group.children.insert(index, Node::Element(entry));
}

fn times(now: Timestamp) -> Element {
    let mut times = Element::new("Times");
    for name in [
        "LastModificationTime",
        "CreationTime",
        "LastAccessTime",
        "ExpiryTime",
    ] {
        push(&mut times, text_element(name, now.encode()));
    }
    push(&mut times, text_element("Expires", "False"));
    push(&mut times, text_element("UsageCount", "0"));
    push(&mut times, text_element("LocationChanged", now.encode()));
    times
}

fn new_group(id: Uuid, name: &str, icon: &str, now: Timestamp) -> Element {
    let mut group = Element::new("Group");
    push(&mut group, text_element("UUID", encode_uuid(id)));
    push(&mut group, text_element("Name", name));
    push(&mut group, Element::new("Notes"));
    push(&mut group, text_element("IconID", icon));
    push(&mut group, times(now));
    push(&mut group, text_element("IsExpanded", "True"));
    push(&mut group, Element::new("DefaultAutoTypeSequence"));
    push(&mut group, text_element("EnableAutoType", "null"));
    push(&mut group, text_element("EnableSearching", "null"));
    push(
        &mut group,
        text_element("LastTopVisibleEntry", encode_uuid(NO_UUID)),
    );
    group
}
