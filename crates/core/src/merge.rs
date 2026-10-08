//! Merging another version of a Vault's Vault file into the Vault, such as one another device
//! saved, with KeePass's rules, so that every client Merging the same versions agrees:
//!
//! - Entries and Groups are matched by UUID, and the most recently modified version wins.
//! - The losing version of an Entry goes to its history, and the histories of both versions are
//!   united, with versions modified at the same time counted once.
//! - The most recent move of an Entry or Group (its `LocationChanged`) decides where it ends up.
//! - `DeletedObjects` deletes what the other version deleted, unless it was modified after the
//!   deletion. A deleted Group that still holds anything is kept.
//!
//! There is no conflict to resolve by hand. Like edits, a Merge changes the Vault's XML document
//! in place and keeps everything keefast doesn't model (ADR 0002).

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::edit::{HistoryLimits, Kind, child_or_insert, insert_entry, push, set_time};
use crate::kdbx;
use crate::vault::{decode_uuid, encode_uuid};
use crate::xml::{Element, Node};
use crate::{OpenError, Timestamp, Vault};

/// What a Merge changed in the Vault.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeSummary {
    pub entries_added: usize,
    pub entries_updated: usize,
    pub entries_moved: usize,
    pub entries_deleted: usize,
    pub groups_added: usize,
    pub groups_updated: usize,
    pub groups_moved: usize,
    pub groups_deleted: usize,
    changed: bool,
}

impl std::ops::AddAssign for MergeSummary {
    /// Adds up the changes of two Merges into one Vault.
    fn add_assign(&mut self, other: Self) {
        self.entries_added += other.entries_added;
        self.entries_updated += other.entries_updated;
        self.entries_moved += other.entries_moved;
        self.entries_deleted += other.entries_deleted;
        self.groups_added += other.groups_added;
        self.groups_updated += other.groups_updated;
        self.groups_moved += other.groups_moved;
        self.groups_deleted += other.groups_deleted;
        self.changed |= other.changed;
    }
}

impl MergeSummary {
    /// Whether the Merge changed the Vault at all, including what the counts leave out: Entry
    /// histories, deletion records, custom icons, Vault custom data and the Recycle Bin.
    pub fn changed(&self) -> bool {
        self.changed
    }
}

impl Vault {
    /// Decrypts another version of this Vault's Vault file, such as one saved by another
    /// device, with the key this Vault was opened with. Runs key derivation, so keep it off the
    /// UI thread.
    pub fn open_version(&self, bytes: &[u8]) -> Result<Vault, OpenError> {
        let file = kdbx::read(bytes, &self.key)?;
        Vault::from_file(file, self.key.clone())
    }

    /// Merges `other`, another version of this Vault, into this one with KeePass's rules (see
    /// the module docs). Merging the same version again changes nothing.
    pub fn merge(&mut self, other: &Vault) -> MergeSummary {
        let before = (self.file.document.clone(), self.file.binaries.len());
        // A Vault that was opened has a root Group with a valid UUID; with none there is
        // nothing to Merge into.
        let Some(root) = id(self.root_group()) else {
            return MergeSummary::default();
        };
        let mut merge = Merge {
            target: self,
            source: other,
            binaries: HashMap::new(),
            added: HashSet::new(),
            summary: MergeSummary::default(),
        };
        merge.group_properties(other.root_group(), root);
        merge.group(other.root_group(), root);
        merge.deletions();
        merge.meta();
        let mut summary = merge.summary;
        summary.changed = (&self.file.document, self.file.binaries.len()) != (&before.0, before.1);
        self.refresh();
        summary
    }

    /// The UUID of the Group holding the element at `path`.
    fn parent_of(&self, path: &[usize]) -> Option<Uuid> {
        let (_, parent) = path.split_last()?;
        id(self.at(parent))
    }
}

struct Merge<'a> {
    target: &'a mut Vault,
    source: &'a Vault,
    /// Where each of the source's attachments went in the target's attachment pool.
    binaries: HashMap<usize, usize>,
    /// The Entries and Groups this Merge added, so deleting one again isn't counted twice.
    added: HashSet<Uuid>,
    summary: MergeSummary,
}

impl Merge<'_> {
    /// Merges what the source Group holds into the target Group with UUID `into`, parents
    /// before children, so every Group an Entry or Group is merged into exists by then.
    fn group(&mut self, source: &Element, into: Uuid) {
        for child in source.elements() {
            match child.name.as_str() {
                "Entry" => self.entry(child, into),
                "Group" => {
                    if let Some(id) = self.subgroup(child, into) {
                        self.group(child, id);
                    }
                }
                _ => {}
            }
        }
    }

    fn entry(&mut self, source: &Element, parent: Uuid) {
        let Some(id) = id(source) else {
            return;
        };
        let Ok(mut path) = self.target.path_to(Kind::Entry, id) else {
            let Some(group) = self.group_path(parent) else {
                return;
            };
            let entry = self.import(source);
            insert_entry(self.target.at_mut(&group), entry);
            self.added.insert(id);
            self.summary.entries_added += 1;
            return;
        };

        if self.moved_later(source, &path, parent)
            && let Some(group) = self.group_path(parent)
        {
            let entry = self.target.take(&path);
            // Taking the Entry out shifted what came after it.
            let group = self.group_path(parent).unwrap_or(group);
            insert_entry(self.target.at_mut(&group), entry);
            path = self.target.path_to(Kind::Entry, id).expect("just moved");
            self.summary.entries_moved += 1;
        }

        let target = self.target.at(&path);
        let location = time(source, "LocationChanged").max(time(target, "LocationChanged"));
        if modified(source) > modified(target) {
            // The source version wins; the target's goes to history.
            let mut loser = target.clone();
            let mut versions = history_of(&loser);
            loser.take_child("History");
            versions.push(loser);
            let mut current = self.import(source);
            let limits = self.target.history_limits();
            let history = child_or_insert(&mut current, "History", &[]);
            add_versions(history, versions, modified(source), &limits);
            *self.target.at_mut(&path) = current;
            self.summary.entries_updated += 1;
        } else {
            let mut versions = history_of(source);
            if modified(source) < modified(target) {
                let mut loser = source.clone();
                loser.take_child("History");
                versions.push(loser);
            }
            let current = modified(target);
            let known: HashSet<Option<Timestamp>> = target
                .child("History")
                .into_iter()
                .flat_map(|history| history.children_named("Entry"))
                .map(modified)
                .chain([current])
                .collect();
            versions.retain(|version| !known.contains(&modified(version)));
            if !versions.is_empty() {
                let versions = versions
                    .iter()
                    .map(|version| self.import(version))
                    .collect();
                let limits = self.target.history_limits();
                let target = self.target.at_mut(&path);
                let history = child_or_insert(target, "History", &[]);
                add_versions(history, versions, current, &limits);
            }
        }
        set_location(self.target.at_mut(&path), location);
    }

    /// Merges a Group's own properties, adding it under `parent` if the target doesn't have
    /// it, and returns its UUID.
    fn subgroup(&mut self, source: &Element, parent: Uuid) -> Option<Uuid> {
        let id = id(source)?;
        let Ok(path) = self.target.path_to(Kind::Group, id) else {
            let mut group = source.clone();
            group.children.retain(|node| !is_item(node));
            let to = self.group_path(parent)?;
            push(self.target.at_mut(&to), group);
            self.added.insert(id);
            self.summary.groups_added += 1;
            return Some(id);
        };
        if !path.is_empty()
            && self.moved_later(source, &path, parent)
            && let Some(to) = self.group_path(parent)
            // Moved into itself by the target meanwhile: the Group stays where it is.
            && !to.starts_with(&path)
        {
            let group = self.target.take(&path);
            if let Some(to) = self.group_path(parent) {
                push(self.target.at_mut(&to), group);
                self.summary.groups_moved += 1;
            }
        }
        self.group_properties(source, id);
        Some(id)
    }

    /// Takes the source Group's name, icon and other properties if it was modified later,
    /// keeping what the target Group holds.
    fn group_properties(&mut self, source: &Element, id: Uuid) {
        let Some(path) = self.group_path(id) else {
            return;
        };
        let target = self.target.at(&path);
        let location = time(source, "LocationChanged").max(time(target, "LocationChanged"));
        if modified(source) > modified(target) {
            let target = self.target.at_mut(&path);
            // The Group stays the Group it is. A Group matched by UUID has the same one in
            // both versions; only the root Group of an unrelated Vault differs, and taking
            // the other's UUID would lose it for everything that looks it up afterwards.
            let uuid = target.child("UUID").cloned();
            let items: Vec<Node> = target.children.drain(..).filter(is_item).collect();
            target.children = source
                .children
                .iter()
                .filter(|node| !is_item(node))
                .map(|node| match (node, &uuid) {
                    (Node::Element(element), Some(uuid)) if element.name == "UUID" => {
                        Node::Element(uuid.clone())
                    }
                    _ => node.clone(),
                })
                .chain(items)
                .collect();
            if !path.is_empty() {
                self.summary.groups_updated += 1;
            }
        }
        set_location(self.target.at_mut(&path), location);
    }

    /// Whether the source has the element at `path` in another Group, `parent`, and moved it
    /// there after the target last moved it.
    fn moved_later(&self, source: &Element, path: &[usize], parent: Uuid) -> bool {
        self.target.parent_of(path) != Some(parent)
            && time(source, "LocationChanged") > time(self.target.at(path), "LocationChanged")
    }

    /// Where the target's Group `id` is. Groups are merged before what they hold, so it is
    /// there; if it somehow isn't, whatever was to go in it is left out of the Merge rather
    /// than the Merge failing.
    fn group_path(&self, id: Uuid) -> Option<Vec<usize>> {
        self.target.path_to(Kind::Group, id).ok()
    }

    /// A copy of a source element for the target, its attachments added to the target's pool.
    fn import(&mut self, element: &Element) -> Element {
        let mut element = element.clone();
        self.import_binaries(&mut element);
        element
    }

    fn import_binaries(&mut self, element: &mut Element) {
        if element.name == "Binary"
            && let Some(value) = element.child_mut("Value")
            && let Some((_, reference)) = value.attributes.iter_mut().find(|(k, _)| k == "Ref")
            && let Ok(index) = reference.trim().parse::<usize>()
            && let Some(binary) = self.source.file.binaries.get(index)
        {
            let pool = &mut self.target.file.binaries;
            let moved = *self.binaries.entry(index).or_insert_with(|| {
                pool.iter()
                    .position(|kept| kept == binary)
                    .unwrap_or_else(|| {
                        pool.push(binary.clone());
                        pool.len() - 1
                    })
            });
            *reference = moved.to_string();
        }
        for child in element.elements_mut() {
            self.import_binaries(child);
        }
    }

    /// Unites both versions' `DeletedObjects`, and deletes what they list unless it was
    /// modified after its deletion, or is a Group that still holds something. The record of a
    /// deletion that didn't happen is dropped, so the other version keeps it too.
    fn deletions(&mut self) {
        let target_records = deletion_records(self.target);
        let mut records: Vec<(Uuid, Element)> = Vec::new();
        for (id, record) in target_records
            .iter()
            .cloned()
            .chain(deletion_records(self.source))
        {
            match records.iter_mut().find(|(known, _)| *known == id) {
                Some((_, kept)) if deleted_at(&record) > deleted_at(kept) => *kept = record,
                Some(_) => {}
                None => records.push((id, record)),
            }
        }

        let mut kept = Vec::new();
        let mut groups = Vec::new();
        for (id, record) in records {
            if let Ok(path) = self.target.path_to(Kind::Entry, id) {
                if modified(self.target.at(&path)) > deleted_at(&record) {
                    continue;
                }
                self.target.take(&path);
                self.count_deletion(id, Kind::Entry);
            } else if let Ok(path) = self.target.path_to(Kind::Group, id)
                && !path.is_empty()
            {
                groups.push((path.len(), id, record));
                continue;
            }
            kept.push(record);
        }
        // Subgroups first, so a Group whose subgroups all go is empty by its turn.
        groups.sort_by_key(|&(depth, ..)| std::cmp::Reverse(depth));
        for (_, id, record) in groups {
            let Some(path) = self.group_path(id) else {
                kept.push(record);
                continue;
            };
            let group = self.target.at(&path);
            if modified(group) > deleted_at(&record) || group.children.iter().any(is_item) {
                continue;
            }
            self.target.take(&path);
            self.count_deletion(id, Kind::Group);
            kept.push(record);
        }

        let unchanged = kept.len() == target_records.len()
            && kept
                .iter()
                .zip(&target_records)
                .all(|(kept, (_, record))| kept == record);
        if !unchanged {
            let root = child_or_insert(&mut self.target.file.document, "Root", &[]);
            let objects = child_or_insert(root, "DeletedObjects", &["Group"]);
            objects.children = kept.into_iter().map(Node::Element).collect();
        }
    }

    fn count_deletion(&mut self, id: Uuid, kind: Kind) {
        let summary = &mut self.summary;
        let (added, deleted) = match kind {
            Kind::Group => (&mut summary.groups_added, &mut summary.groups_deleted),
            Kind::Entry => (&mut summary.entries_added, &mut summary.entries_deleted),
        };
        if self.added.remove(&id) {
            *added -= 1;
        } else {
            *deleted += 1;
        }
    }

    /// Adds the source's custom icons and Vault custom data the target doesn't have, and its
    /// Recycle Bin if the target has none.
    fn meta(&mut self) {
        let Some(source) = self.source.meta() else {
            return;
        };
        let target = self.target.meta();
        let icon_ids: HashSet<Option<Uuid>> = target
            .and_then(|meta| meta.child("CustomIcons"))
            .into_iter()
            .flat_map(|icons| icons.children_named("Icon"))
            .map(id)
            .collect();
        let icons: Vec<Element> = source
            .child("CustomIcons")
            .into_iter()
            .flat_map(|icons| icons.children_named("Icon"))
            .filter(|icon| !icon_ids.contains(&id(icon)))
            .cloned()
            .collect();

        let target_items: Vec<&Element> = target
            .and_then(|meta| meta.child("CustomData"))
            .into_iter()
            .flat_map(|data| data.children_named("Item"))
            .collect();
        // Items the target lacks, and those changed later in the source (KDBX 4.1 times them).
        let items: Vec<(Option<usize>, Element)> = source
            .child("CustomData")
            .into_iter()
            .flat_map(|data| data.children_named("Item"))
            .filter_map(|item| {
                let index = target_items
                    .iter()
                    .position(|known| item_key(known) == item_key(item));
                let newer = index.is_none_or(|index| {
                    let known = target_items[index];
                    known != item && modified_item(item) > modified_item(known)
                });
                newer.then(|| (index, item.clone()))
            })
            .collect();

        let bin = (self.target.recycle_bin_uuid().is_none())
            .then(|| self.source.recycle_bin_uuid())
            .flatten()
            .filter(|bin| self.target.path_to(Kind::Group, *bin).is_ok())
            .map(|bin| {
                let changed = source.child("RecycleBinChanged").map(Element::text);
                (bin, changed)
            });

        let meta = child_or_insert(&mut self.target.file.document, "Meta", &[]);
        if !icons.is_empty() {
            let custom_icons = child_or_insert(meta, "CustomIcons", &["MemoryProtection"]);
            custom_icons
                .children
                .extend(icons.into_iter().map(Node::Element));
        }
        if !items.is_empty() {
            let data = child_or_insert(meta, "CustomData", &[]);
            for (index, item) in items {
                match index
                    .and_then(|index| data.elements_mut().filter(|e| e.name == "Item").nth(index))
                {
                    Some(known) => *known = item,
                    None => push(data, item),
                }
            }
        }
        if let Some((bin, changed)) = bin {
            child_or_insert(meta, "RecycleBinUUID", &["RecycleBinEnabled"])
                .set_text(encode_uuid(bin));
            if let Some(changed) = changed {
                child_or_insert(meta, "RecycleBinChanged", &["RecycleBinUUID"]).set_text(changed);
            }
        }
    }
}

/// Adds earlier versions of an Entry to its history, oldest first, leaving out those modified
/// when one already there or the current version was, and trims it to the Vault's limits. Versions
/// already there modified at the same time are counted once too.
fn add_versions(
    history: &mut Element,
    versions: Vec<Element>,
    current: Option<Timestamp>,
    limits: &HistoryLimits,
) {
    let mut entries: Vec<Element> = Vec::new();
    let mut others = Vec::new();
    for node in history.children.drain(..) {
        match node {
            Node::Element(entry) if entry.name == "Entry" => entries.push(entry),
            other => others.push(other),
        }
    }
    for version in versions {
        let time = modified(&version);
        if time != current && entries.iter().all(|kept| modified(kept) != time) {
            entries.push(version);
        }
    }
    // Versions modified at the same time count once, the later listed kept, as in KeePassXC.
    entries.sort_by_key(modified);
    entries.reverse();
    entries.dedup_by_key(|entry| modified(entry));
    entries.reverse();
    history.children = others
        .into_iter()
        .chain(entries.into_iter().map(Node::Element))
        .collect();
    limits.trim(history);
}

/// The earlier versions in an Entry's history.
fn history_of(entry: &Element) -> Vec<Element> {
    entry
        .child("History")
        .into_iter()
        .flat_map(|history| history.children_named("Entry"))
        .cloned()
        .collect()
}

fn deletion_records(vault: &Vault) -> Vec<(Uuid, Element)> {
    vault
        .file
        .document
        .child("Root")
        .and_then(|root| root.child("DeletedObjects"))
        .into_iter()
        .flat_map(|objects| objects.children_named("DeletedObject"))
        .filter_map(|record| Some((id(record)?, record.clone())))
        .collect()
}

fn deleted_at(record: &Element) -> Option<Timestamp> {
    Timestamp::decode(&record.child("DeletionTime")?.text())
}

/// Entries and subgroups, as opposed to a Group's own properties.
fn is_item(node: &Node) -> bool {
    matches!(node, Node::Element(element) if element.name == "Entry" || element.name == "Group")
}

fn id(element: &Element) -> Option<Uuid> {
    decode_uuid(&element.child("UUID")?.text())
}

fn time(element: &Element, name: &str) -> Option<Timestamp> {
    Timestamp::decode(&element.child("Times")?.child(name)?.text())
}

fn modified(element: &Element) -> Option<Timestamp> {
    time(element, "LastModificationTime")
}

fn set_location(element: &mut Element, location: Option<Timestamp>) {
    if let Some(location) = location
        && time(element, "LocationChanged") != Some(location)
    {
        set_time(element, "LocationChanged", location);
    }
}

fn item_key(item: &Element) -> Option<String> {
    item.child("Key").map(Element::text)
}

fn modified_item(item: &Element) -> Option<Timestamp> {
    Timestamp::decode(&item.child("LastModificationTime")?.text())
}
