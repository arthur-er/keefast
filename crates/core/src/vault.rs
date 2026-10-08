use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::appearance::{CustomIcon, custom_icons, parse_tags};
use crate::attachment::{self, Attachment};
use crate::edit::{is_standard_field, string_value};
use crate::kdbx::{Binary, Key, VaultFile};
use crate::xml::Element;
use crate::{EntryColor, EntryIcon, OpenError, Timestamp, Totp, TotpError, totp};

/// An opened, decrypted Vault: a tree of Groups holding Entries.
///
/// The Vault keeps the whole Vault file it was opened from, so saving it writes back everything
/// in it, including data keefast doesn't model (ADR 0002). Edits change that document, and the
/// Groups and Entries are read back from it.
#[derive(Clone)]
pub struct Vault {
    root: Group,
    pub(crate) custom_icons: Vec<CustomIcon>,
    pub(crate) file: VaultFile,
    /// The hash of the composite key, which saving needs to re-encrypt the Vault file.
    pub(crate) key: Key,
    /// The key the Vault file had before the key was changed, until the change is saved.
    pub(crate) previous_key: Option<Key>,
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

    /// The Entry a browser names by its UUID in hex, as [`Self::to_hex`] writes it.
    pub fn from_hex(hex: &str) -> Option<Self> {
        Uuid::try_parse(hex.trim()).ok().map(Self)
    }
}

/// KeePassXC reads every field whose name starts with this as another URL for the Entry.
const ADDITIONAL_URL_PREFIX: &str = "KP2A_URL";

/// A field of an Entry besides its title, username, password, URL and notes.
#[derive(Clone)]
pub struct CustomField {
    pub name: String,
    pub value: SecretString,
    /// Whether the value is stored encrypted in the Vault file and hidden until revealed.
    pub protected: bool,
}

impl PartialEq for CustomField {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.protected == other.protected
            && self.value.expose_secret() == other.value.expose_secret()
    }
}

impl fmt::Debug for CustomField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomField")
            .field("name", &self.name)
            .field("value", &"[hidden]")
            .field("protected", &self.protected)
            .finish()
    }
}

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
    /// The Entry's TOTP settings, from whichever client's fields hold them.
    totp: Option<Result<Totp, TotpError>>,
    /// In Vault file order.
    custom_fields: Vec<CustomField>,
    /// In Vault file order.
    attachments: Vec<Attachment>,
    pub(crate) custom_data: BTreeMap<String, String>,
    tags: Vec<String>,
    created: Option<Timestamp>,
    color: Option<EntryColor>,
    icon: EntryIcon,
    /// When the Entry expires, if it does.
    expires: Option<Timestamp>,
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
            root: Group::from_element(group, &file.binaries)?,
            custom_icons: custom_icons(&file.document),
            file,
            key,
            previous_key: None,
        })
    }

    /// Reads the Groups, Entries and custom icons back from the document after an edit.
    pub(crate) fn refresh(&mut self) {
        self.root = Group::from_element(self.root_group(), &self.file.binaries)
            .expect("edits keep every UUID valid");
        self.custom_icons = custom_icons(&self.file.document);
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

/// The custom fields among an Entry's `String`s. Of fields sharing a name, which no client
/// writes, the first is the one read, as for the standard fields.
fn custom_fields<'a>(
    strings: impl Iterator<Item = (String, Option<&'a Element>)>,
) -> Vec<CustomField> {
    let mut fields: Vec<CustomField> = Vec::new();
    for (name, value) in strings {
        if is_standard_field(&name) || fields.iter().any(|field| field.name == name) {
            continue;
        }
        fields.push(CustomField {
            value: SecretString::from(value.map(Element::text).unwrap_or_default()),
            protected: value.is_some_and(Element::is_protected),
            name,
        });
    }
    fields
}

/// When an Entry or Group was last modified, from its `Times`.
fn last_modified(element: &Element) -> Option<Timestamp> {
    time(element, "LastModificationTime")
}

/// One of an Entry's or Group's `Times`.
fn time(element: &Element, name: &str) -> Option<Timestamp> {
    Timestamp::decode(&element.child("Times")?.child(name)?.text())
}

/// When an Entry or Group expires, if its `Times` say it does.
fn expires(element: &Element) -> Option<Timestamp> {
    let times = element.child("Times")?;
    let expires = times.child("Expires")?.text();
    if !expires.trim().eq_ignore_ascii_case("true") {
        return None;
    }
    Timestamp::decode(&times.child("ExpiryTime")?.text())
}

impl Group {
    fn from_element(group: &Element, pool: &[Binary]) -> Result<Self, OpenError> {
        Ok(Self {
            id: GroupId(uuid(group)?),
            name: group.child("Name").map(Element::text).unwrap_or_default(),
            last_modified: last_modified(group),
            custom_data: custom_data(group),
            groups: group
                .children_named("Group")
                .map(|group| Group::from_element(group, pool))
                .collect::<Result<_, _>>()?,
            entries: group
                .children_named("Entry")
                .map(|entry| Entry::from_element(entry, pool))
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
    fn from_element(entry: &Element, pool: &[Binary]) -> Result<Self, OpenError> {
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
            totp: totp::read(&|name| string_value(entry, name)),
            custom_fields: custom_fields(strings()),
            attachments: attachment::read(entry, pool),
            custom_data: custom_data(entry),
            tags: parse_tags(&entry.child("Tags").map(Element::text).unwrap_or_default()),
            created: time(entry, "CreationTime"),
            // KeePassXC draws the background colour behind the row, the foreground one as the
            // text; KeeWeb marks an Entry by either.
            color: ["BackgroundColor", "ForegroundColor"]
                .into_iter()
                .find_map(|name| EntryColor::nearest(&entry.child(name)?.text())),
            icon: EntryIcon::read(entry),
            expires: expires(entry),
            last_modified: last_modified(entry),
            history: entry
                .child("History")
                .map(|history| {
                    history
                        .children_named("Entry")
                        .map(|entry| Entry::from_element(entry, pool))
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

    /// The Entry's TOTP settings, if it has any, or why they can't be read.
    pub fn totp(&self) -> Option<Result<&Totp, TotpError>> {
        self.totp
            .as_ref()
            .map(|totp| totp.as_ref().map_err(|error| *error))
    }

    /// The Entry's other fields, as KeePassXC lists them: everything but the standard fields,
    /// including those other features keep their settings in, such as `KP2A_URL` and `otp`.
    pub fn custom_fields(&self) -> &[CustomField] {
        &self.custom_fields
    }

    /// The files kept with the Entry, in Vault file order.
    pub fn attachments(&self) -> &[Attachment] {
        &self.attachments
    }

    pub fn created(&self) -> Option<Timestamp> {
        self.created
    }

    pub fn last_modified(&self) -> Option<Timestamp> {
        self.last_modified
    }

    /// The Entry's tags, in Vault file order.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// The colour the Entry is marked with, if it is.
    pub fn color(&self) -> Option<EntryColor> {
        self.color
    }

    /// The icon the Entry is shown with.
    pub fn icon(&self) -> EntryIcon {
        self.icon
    }

    /// When the Entry expires, if it does.
    pub fn expires(&self) -> Option<Timestamp> {
        self.expires
    }

    /// Whether the Entry's expiry time has come by `now`.
    pub fn is_expired(&self, now: Timestamp) -> bool {
        self.expires.is_some_and(|expires| expires <= now)
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
            .field("custom_fields", &self.custom_fields)
            .field("attachments", &self.attachments)
            .field("tags", &self.tags)
            .field("color", &self.color)
            .field("icon", &self.icon)
            .field("expires", &self.expires)
            .field("history", &self.history)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn entry(marks: &str) -> Entry {
        let xml = format!(
            "<Entry><UUID>AAAAAAAAAAAAAAAAAAAAAA==</UUID>{marks}<Times>\
             <ExpiryTime>islY4g4AAAA=</ExpiryTime><Expires>False</Expires></Times></Entry>"
        );
        Entry::from_element(&xml::parse(&xml).unwrap(), &[]).unwrap()
    }

    #[test]
    fn a_text_colour_marks_an_entry_without_a_background_colour() {
        let coloured = |marks| entry(marks).color();
        assert_eq!(
            coloured("<ForegroundColor>#0000FF</ForegroundColor><BackgroundColor/>"),
            Some(EntryColor::Blue)
        );
        assert_eq!(
            coloured(
                "<ForegroundColor>#0000FF</ForegroundColor>\
                 <BackgroundColor>#FF0000</BackgroundColor>"
            ),
            Some(EntryColor::Red)
        );
        assert_eq!(coloured("<ForegroundColor>#000000</ForegroundColor>"), None);
    }

    #[test]
    fn an_entry_expires_only_when_it_says_so() {
        assert_eq!(entry("").expires(), None);
        let expiring = Entry::from_element(
            &xml::parse(
                "<Entry><UUID>AAAAAAAAAAAAAAAAAAAAAA==</UUID><Times>\
                 <ExpiryTime>islY4g4AAAA=</ExpiryTime><Expires>True</Expires></Times></Entry>",
            )
            .unwrap(),
            &[],
        )
        .unwrap();
        assert_eq!(
            expiring.expires(),
            Some(Timestamp::from_unix_seconds(1_791_414_922))
        );
    }

    #[test]
    fn an_unknown_standard_icon_reads_as_the_key() {
        assert_eq!(entry("<IconID>68</IconID>").icon(), EntryIcon::Standard(68));
        assert_eq!(entry("<IconID>69</IconID>").icon(), EntryIcon::Standard(0));
        assert_eq!(entry("<IconID>x</IconID>").icon(), EntryIcon::Standard(0));
        let nil = "<IconID>2</IconID><CustomIconUUID>AAAAAAAAAAAAAAAAAAAAAA==</CustomIconUUID>";
        assert_eq!(entry(nil).icon(), EntryIcon::Standard(2));
    }
}
