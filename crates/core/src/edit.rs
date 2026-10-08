//! Commands that change a Vault: creating, editing, moving and deleting Entries and Groups.
//!
//! Each command edits the Vault's XML document in place, as KeePass would, and touches nothing
//! it doesn't need to, so everything keefast doesn't model survives (ADR 0002).

use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::appearance::{STANDARD_ICON_COUNT, join_tags, parse_tags};
use crate::attachment;
use crate::kdbx::random;
use crate::totp::{self, TOTP_FIELDS};
use crate::vault::{decode_uuid, encode_uuid};
use crate::xml::{Element, Node, Secret};
use crate::{
    Attachment, CustomField, Entry, EntryColor, EntryIcon, EntryId, GroupId, Timestamp, Totp, Vault,
};

/// The fields of an Entry that keefast edits.
#[derive(Clone)]
pub struct EntryFields {
    pub title: String,
    pub username: String,
    pub password: SecretString,
    pub url: String,
    pub notes: String,
    /// In order; fields the Entry has but these don't are removed.
    pub custom_fields: Vec<CustomField>,
    /// Each split further as [`parse_tags`] splits text, so no tag holds a separator.
    pub tags: Vec<String>,
    pub color: Option<EntryColor>,
    pub icon: EntryIcon,
    /// When the Entry expires, if it does.
    pub expires: Option<Timestamp>,
    /// In order; attachments the Entry has but these don't are removed.
    pub attachments: Vec<Attachment>,
}

impl Default for EntryFields {
    fn default() -> Self {
        Self {
            title: String::new(),
            username: String::new(),
            password: SecretString::from(String::new()),
            url: String::new(),
            notes: String::new(),
            custom_fields: Vec::new(),
            tags: Vec::new(),
            color: None,
            icon: EntryIcon::default(),
            expires: None,
            attachments: Vec::new(),
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
    #[error("the Group is in the Recycle Bin")]
    InRecycleBin,
    #[error("a custom field needs a name")]
    FieldNameEmpty,
    #[error("the Entry already has a field with this name")]
    FieldNameTaken,
    #[error("the Entry has no such version in its history")]
    VersionNotFound,
    #[error("an attachment needs a name")]
    AttachmentNameEmpty,
    #[error("the Entry already has an attachment with this name")]
    AttachmentNameTaken,
    #[error("a Pairing needs a name")]
    PairingNameEmpty,
    #[error("the Vault already has a Pairing with this name")]
    PairingNameTaken,
    #[error("the icon is not in this Vault")]
    IconNotFound,
    #[error("an icon must be a PNG image")]
    IconNotPng,
    #[error("the icon image is too large")]
    IconTooLarge,
    #[error("the Vault has no Pairing with this name")]
    PairingNotFound,
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
const DEFAULT_HISTORY_MAX_SIZE: i64 = 6 * 1024 * 1024;
/// What KeePass counts for an Entry version besides its fields and attachments.
const VERSION_BASE_SIZE: u64 = 128;
pub(crate) const FOLDER_ICON: &str = "48";
const RECYCLE_BIN_ICON: &str = "43";
const RECYCLE_BIN_NAME: &str = "Recycle Bin";
pub(crate) const NO_UUID: Uuid = Uuid::nil();

impl From<&Entry> for EntryFields {
    fn from(entry: &Entry) -> Self {
        Self {
            title: entry.title().to_owned(),
            username: entry.username().to_owned(),
            password: entry.password().clone(),
            url: entry.url().to_owned(),
            notes: entry.notes().to_owned(),
            custom_fields: entry.custom_fields().to_vec(),
            tags: entry.tags().to_vec(),
            color: entry.color(),
            icon: entry.icon(),
            expires: entry.expires(),
            attachments: entry.attachments().to_vec(),
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

    /// The tags as they are written: each split as typed tags are, without repeats.
    fn tags(&self) -> Vec<String> {
        parse_tags(&join_tags(&self.tags))
    }

    /// Whether these fields are the Entry's, as far as keefast edits them.
    fn match_entry(&self, entry: &Entry) -> bool {
        let current = EntryFields::from(entry);
        current.values() == self.values()
            && current.custom_fields == self.custom_fields
            && current.tags == self.tags()
            && current.color == self.color
            && current.icon == self.icon
            && current.expires == self.expires
    }

    /// Every custom field needs a name, and no two fields of an Entry can share one, even
    /// told apart only by spaces around it, which KeePassXC trims away. The icon must be one
    /// the Vault has.
    fn check(&self, vault: &Vault) -> Result<(), EditError> {
        match self.icon {
            EntryIcon::Standard(id) if id >= STANDARD_ICON_COUNT => {
                return Err(EditError::IconNotFound);
            }
            EntryIcon::Custom(id) if vault.custom_icon(id).is_none() => {
                return Err(EditError::IconNotFound);
            }
            _ => {}
        }
        for (index, field) in self.custom_fields.iter().enumerate() {
            let name = field.name.trim();
            if name.is_empty() {
                return Err(EditError::FieldNameEmpty);
            }
            let earlier = &self.custom_fields[..index];
            if is_standard_field(name) || earlier.iter().any(|f| f.name.trim() == name) {
                return Err(EditError::FieldNameTaken);
            }
        }
        Ok(())
    }

    /// Every attachment needs a name of its own. Only checked for attachments being set, so an
    /// Entry another client gave clashing names can still be edited otherwise.
    fn check_attachments(&self) -> Result<(), EditError> {
        for (index, attachment) in self.attachments.iter().enumerate() {
            if attachment.name.trim().is_empty() {
                return Err(EditError::AttachmentNameEmpty);
            }
            if self.attachments[..index]
                .iter()
                .any(|earlier| earlier.name == attachment.name)
            {
                return Err(EditError::AttachmentNameTaken);
            }
        }
        Ok(())
    }
}

/// Whether `name` is one of the standard fields every Entry has, rather than a custom field.
pub(crate) fn is_standard_field(name: &str) -> bool {
    FIELDS.iter().any(|(key, _)| *key == name)
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
        fields.check(self)?;
        fields.check_attachments()?;
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
        for field in &fields.custom_fields {
            let value = field.value.expose_secret();
            push(&mut entry, new_string(&field.name, value, field.protected));
        }
        // References that point past the pool go first, so none lands on contents added here.
        self.drop_unused_attachments();
        attachment::write(&mut entry, &fields.attachments, &mut self.file.binaries);
        let mut auto_type = Element::new("AutoType");
        push(&mut auto_type, text_element("Enabled", "True"));
        push(&mut auto_type, text_element("DataTransferObfuscation", "0"));
        push(&mut entry, auto_type);
        push(&mut entry, Element::new("History"));
        // The new Entry has no tags or colour, the default icon, and doesn't expire.
        update_marks_and_expiry(&mut entry, &EntryFields::default(), fields);

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
        fields.check(self)?;
        let current = self.entry(entry).expect("the path leads to it");
        let attachments_changed = current.attachments() != fields.attachments.as_slice();
        if fields.match_entry(current) && !attachments_changed {
            return Ok(());
        }
        let current = EntryFields::from(current);
        if attachments_changed {
            fields.check_attachments()?;
            // References that point past the pool go first, so none lands on contents added
            // here.
            self.drop_unused_attachments();
        }
        let protection = FIELDS.map(|(_, protect)| self.protects(protect));
        // The history is trimmed by what its attachments weigh, so archive before the pool of
        // attachment contents is taken out to be written to.
        self.archive(&path);
        let mut pool = std::mem::take(&mut self.file.binaries);
        let element = self.at_mut(&path);
        for (((key, _), protect), value) in FIELDS.into_iter().zip(protection).zip(fields.values())
        {
            set_string(element, key, value, protect);
        }
        set_custom_fields(element, &fields.custom_fields);
        update_marks_and_expiry(element, &current, fields);
        if attachments_changed {
            attachment::write(element, &fields.attachments, &mut pool);
        }
        set_time(element, "LastModificationTime", now);
        set_time(element, "LastAccessTime", now);
        self.file.binaries = pool;
        self.drop_unused_attachments();
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
        self.drop_unused_attachments();
        self.refresh();
        Ok(())
    }

    /// Makes the version at `version` in the Entry's history, oldest first, current again. The
    /// version it replaces goes to the history, which is trimmed to the Vault's limits; the
    /// Entry stays where it is.
    pub fn restore_entry_version(
        &mut self,
        entry: EntryId,
        version: usize,
        now: Timestamp,
    ) -> Result<(), EditError> {
        let path = self.path_to(Kind::Entry, entry.0)?;
        let current = self.at(&path);
        let mut restored = current
            .child("History")
            .and_then(|history| history.children_named("Entry").nth(version))
            .cloned()
            .ok_or(EditError::VersionNotFound)?;
        let location = current
            .child("Times")
            .and_then(|times| times.child("LocationChanged"))
            .and_then(|time| Timestamp::decode(&time.text()));

        let element = self.archive(&path);
        restored.take_child("History");
        if let Some(history) = element.take_child("History") {
            push(&mut restored, history);
        }
        *element = restored;
        if let Some(location) = location {
            set_time(element, "LocationChanged", location);
        }
        set_time(element, "LastModificationTime", now);
        set_time(element, "LastAccessTime", now);
        self.drop_unused_attachments();
        self.refresh();
        Ok(())
    }

    /// Drops attachment contents that no Entry or history version refers to any more, as
    /// KeePass does, so removed attachments don't stay in the Vault file.
    fn drop_unused_attachments(&mut self) {
        attachment::drop_unused(&mut self.file.document, &mut self.file.binaries);
    }

    /// Adds the Entry at `path`, as it is now, to its history, which is trimmed to the Vault's
    /// limits, and returns the Entry to change.
    fn archive(&mut self, path: &[usize]) -> &mut Element {
        let limits = self.history_limits();
        let element = self.at_mut(path);
        let mut previous = element.clone();
        previous.take_child("History");
        let history = child_or_insert(element, "History", &[]);
        history.children.push(Node::Element(previous));
        limits.trim(history);
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
        self.drop_unused_attachments();
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
        self.drop_unused_attachments();
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

    pub(crate) fn at(&self, path: &[usize]) -> &Element {
        let mut element = self.root_group();
        for &index in path {
            element = match &element.children[index] {
                Node::Element(child) => child,
                _ => unreachable!("paths lead through elements"),
            };
        }
        element
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

    pub(crate) fn take(&mut self, path: &[usize]) -> Element {
        let (&last, parent) = path.split_last().expect("the root Group is never taken");
        match self.at_mut(parent).children.remove(last) {
            Node::Element(element) => element,
            _ => unreachable!("paths lead to elements"),
        }
    }

    pub(crate) fn meta(&self) -> Option<&Element> {
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

    /// The Vault's `HistoryMaxItems` and `HistoryMaxSize`, with what its attachments weigh.
    pub(crate) fn history_limits(&self) -> HistoryLimits {
        let setting = |name: &str, default: i64| {
            let max = self
                .meta_text(name)
                .and_then(|text| text.trim().parse::<i64>().ok())
                .unwrap_or(default);
            // Negative means unlimited.
            u64::try_from(max).ok()
        };
        HistoryLimits {
            max_items: setting("HistoryMaxItems", DEFAULT_HISTORY_MAX_ITEMS)
                .map(|max| usize::try_from(max).unwrap_or(usize::MAX)),
            max_size: setting("HistoryMaxSize", DEFAULT_HISTORY_MAX_SIZE),
            binary_sizes: self
                .file
                .binaries
                .iter()
                .map(|binary| binary.data.bytes().len() as u64)
                .collect(),
        }
    }

    fn recycle_bin_enabled(&self) -> bool {
        self.meta_text("RecycleBinEnabled")
            .is_none_or(|text| is_true(&text))
    }

    pub(crate) fn recycle_bin_uuid(&self) -> Option<Uuid> {
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

/// How many earlier versions of an Entry, and how much of them, the Vault keeps.
pub(crate) struct HistoryLimits {
    /// `None` when unlimited.
    max_items: Option<usize>,
    /// In bytes, as KeePass weighs Entries; `None` when unlimited.
    max_size: Option<u64>,
    /// The size of each attachment in the Vault's pool, by index.
    binary_sizes: Vec<u64>,
}

impl HistoryLimits {
    /// Drops the oldest versions, first listed, from an Entry's `History` until what is left
    /// is within the limits, as KeePass does.
    pub(crate) fn trim(&self, history: &mut Element) {
        let sizes: Vec<u64> = history
            .children_named("Entry")
            .map(|version| self.size(version))
            .collect();
        let mut excess = self
            .max_items
            .map_or(0, |max| sizes.len().saturating_sub(max));
        if let Some(max) = self.max_size {
            let mut total: u64 = sizes[excess..].iter().sum();
            while total > max {
                total -= sizes[excess];
                excess += 1;
            }
        }
        history.children.retain(|node| {
            let is_version = matches!(node, Node::Element(e) if e.name == "Entry");
            let drop = is_version && excess > 0;
            excess -= usize::from(drop);
            !drop
        });
    }

    /// Roughly what KeePass's `PwEntry.GetSize` counts: the fields and attachments, by name
    /// and value, and the tags and URL override.
    fn size(&self, version: &Element) -> u64 {
        let length = |element: Option<&Element>| element.map_or(0, |e| e.text_len() as u64);
        let strings: u64 = version
            .children_named("String")
            .map(|string| length(string.child("Key")) + length(string.child("Value")))
            .sum();
        let binaries: u64 = version
            .children_named("Binary")
            .map(|binary| {
                let value = binary.child("Value");
                let data = match value.and_then(|value| value.attribute("Ref")) {
                    Some(index) => index
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| self.binary_sizes.get(index).copied())
                        .unwrap_or(0),
                    None => length(value),
                };
                length(binary.child("Key")) + data
            })
            .sum();
        let other = length(version.child("Tags")) + length(version.child("OverrideURL"));
        VERSION_BASE_SIZE + strings + binaries + other
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

pub(crate) fn has_uuid(element: &Element, id: Uuid) -> bool {
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

pub(crate) fn new_uuid() -> Uuid {
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
pub(crate) fn set_value(element: &mut Element, value: &str, protect: bool) {
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

/// Changes an Entry element's marks (tags, colour and icon) and expiry from `current` to `new`, leaving
/// alone those that don't change, so they stay as the client that wrote them put them.
fn update_marks_and_expiry(entry: &mut Element, current: &EntryFields, new: &EntryFields) {
    let tags = new.tags();
    if current.tags != tags {
        set_value(
            child_or_insert(
                entry,
                "Tags",
                &[
                    "OverrideURL",
                    "BackgroundColor",
                    "ForegroundColor",
                    "Times",
                    "IconID",
                    "UUID",
                ],
            ),
            &join_tags(&tags),
            false,
        );
    }
    if current.color != new.color {
        let background = new.color.map_or("", EntryColor::hex);
        set_value(
            child_or_insert(
                entry,
                "BackgroundColor",
                &["ForegroundColor", "CustomIconUUID", "IconID", "UUID"],
            ),
            background,
            false,
        );
        // A foreground colour would mark the Entry when the background no longer does.
        if new.color.is_none()
            && let Some(foreground) = entry.child_mut("ForegroundColor")
        {
            set_value(foreground, "", false);
        }
    }
    if current.icon != new.icon {
        new.icon.write(entry);
    }
    if current.expires != new.expires {
        let times = child_or_insert(entry, "Times", &["Tags", "IconID", "Name", "UUID"]);
        if let Some(expires) = new.expires {
            set_value(
                child_or_insert(times, "ExpiryTime", &["LastAccessTime"]),
                &expires.encode(),
                false,
            );
        }
        let flag = if new.expires.is_some() {
            "True"
        } else {
            "False"
        };
        set_value(
            child_or_insert(times, "Expires", &["ExpiryTime"]),
            flag,
            false,
        );
    }
}

/// Sets a standard field on an Entry element. A field that exists keeps its own protection
/// if it is protected; otherwise the Vault's `MemoryProtection` decides.
fn set_string(entry: &mut Element, key: &str, value: &str, protect: bool) {
    let protect = protect || string_mut(entry, key).is_some_and(|v| v.is_protected());
    set_field(entry, key, value, protect);
}

/// Makes an Entry element's custom fields `fields`, in that order, where its first custom
/// field was (or after its other fields). Those it has keep their element, with anything in it
/// keefast doesn't model, and those not in `fields` are removed.
fn set_custom_fields(entry: &mut Element, fields: &[CustomField]) {
    let key = |string: &Element| string.child("Key").map(Element::text).unwrap_or_default();
    let mut existing = Vec::new();
    let mut at = None;
    for node in std::mem::take(&mut entry.children) {
        match node {
            Node::Element(string)
                if string.name == "String" && !is_standard_field(&key(&string)) =>
            {
                at.get_or_insert(entry.children.len());
                existing.push(string);
            }
            node => entry.children.push(node),
        }
    }
    let mut at = at.unwrap_or_else(|| insertion_index(entry, STRING_AFTER));
    for field in fields {
        let mut string = match existing.iter().position(|string| key(string) == field.name) {
            Some(index) => existing.remove(index),
            None => new_string(&field.name, "", false),
        };
        let value = child_or_insert(&mut string, "Value", &["Key"]);
        set_value(value, field.value.expose_secret(), field.protected);
        entry.children.insert(at, Node::Element(string));
        at += 1;
    }
    // A second field with a kept name, which no client writes, stays as it was.
    for string in existing {
        if fields.iter().any(|field| field.name == key(&string)) {
            entry.children.insert(at, Node::Element(string));
            at += 1;
        }
    }
}

/// Sets a field of an Entry element, protected or not, adding it if the Entry hasn't got it.
fn set_field(entry: &mut Element, key: &str, value: &str, protect: bool) {
    match string_mut(entry, key) {
        Some(value_element) => set_value(value_element, value, protect),
        None => insert_after_last(entry, new_string(key, value, protect), STRING_AFTER),
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

/// What a new `<String>` goes after in an Entry, the first of these it has.
const STRING_AFTER: &[&str] = &["String", "Times", "Tags", "UUID"];

/// The `Value` element of an Entry element's field called `key`, if it has that field.
fn string_mut<'a>(entry: &'a mut Element, key: &str) -> Option<&'a mut Element> {
    let string = entry.elements_mut().find(|string| {
        string.name == "String" && string.child("Key").is_some_and(|k| k.text() == key)
    })?;
    Some(child_or_insert(string, "Value", &["Key"]))
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
    let index = insertion_index(parent, after);
    parent.children.insert(index, Node::Element(child));
}

/// The index just after the last child element named in `after`, trying the names in order,
/// or the end.
fn insertion_index(parent: &Element, after: &[&str]) -> usize {
    after
        .iter()
        .find_map(|name| {
            parent
                .children
                .iter()
                .rposition(|node| matches!(node, Node::Element(e) if e.name == *name))
        })
        .map_or(parent.children.len(), |index| index + 1)
}

/// KeePass lists a Group's Entries before its subgroups.
pub(crate) fn insert_entry(group: &mut Element, entry: Element) {
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

pub(crate) fn new_group(id: Uuid, name: &str, icon: &str, now: Timestamp) -> Element {
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
