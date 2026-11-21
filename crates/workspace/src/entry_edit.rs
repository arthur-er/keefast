//! Editing an Entry by a partial change, as the app's editor and its quick actions (adding tags,
//! adding and removing attachments) do. The change is applied to the Entry as it is when the edit
//! lands, under the same borrow that writes it, so a Merge can't land between reading the Entry
//! and writing it back. The rules for what an edit leaves alone live here: a secret the editor
//! never revealed, custom fields matched by where they were, an expiry's time of day.

use keefast_core::{
    Attachment, CustomField, EditError, Entry, EntryColor, EntryFields, EntryIcon, EntryId,
    Timestamp, parse_tags,
};
use secrecy::SecretString;

use crate::{EditVaultError, VaultId, Workspace};

/// A partial change to an Entry: each part left `None` (or, for attachments, empty) is left as
/// the Entry has it.
#[derive(Debug, Clone, Default)]
pub struct EntryEdit {
    pub title: Option<String>,
    pub username: Option<String>,
    /// `None` keeps the password, which an editor that never revealed it can't send back.
    pub password: Option<SecretString>,
    pub url: Option<String>,
    pub notes: Option<String>,
    /// The Entry's custom fields, in order; those it has but these don't are removed.
    pub custom_fields: Option<Vec<CustomFieldEdit>>,
    pub tags: Option<TagsEdit>,
    /// `Some(None)` takes the colour off.
    pub color: Option<Option<EntryColor>>,
    pub icon: Option<EntryIcon>,
    /// The expiry date as `YYYY-MM-DD`, the start of that day UTC, or `Some(None)` for none. A
    /// date the Entry's expiry already falls on keeps its time, so one set by another client to
    /// the second isn't moved.
    pub expires: Option<Option<String>>,
    /// Applied in order, after the other parts.
    pub attachments: Vec<AttachmentEdit>,
}

/// A custom field to give an Entry. Its name is trimmed.
#[derive(Debug, Clone)]
pub struct CustomFieldEdit {
    pub name: String,
    pub value: CustomFieldValue,
    pub protected: bool,
}

/// The value of a [`CustomFieldEdit`].
#[derive(Debug, Clone)]
pub enum CustomFieldValue {
    New(SecretString),
    /// The value of the Entry's custom field at this index, so a protected value never has to
    /// be revealed to be kept. Matched by index, not name, because the edit may rename it.
    Kept(usize),
    Empty,
}

/// A change to an Entry's tags. Each text is split on commas, semicolons and colons, as KeePass
/// splits them ([`parse_tags`]).
#[derive(Debug, Clone)]
pub enum TagsEdit {
    Replace(Vec<String>),
    /// Adds the tags the Entry hasn't got; those it has change nothing.
    Add(String),
}

/// A change to an Entry's attachments.
#[derive(Debug, Clone)]
pub enum AttachmentEdit {
    /// Added at the end, its name numbered if the Entry has one by that name already, as in
    /// `codes (2).txt`.
    Add(Attachment),
    /// Removes the attachment at this index, counted after the edits before it.
    Remove(usize),
}

/// Why an [`EntryEdit`] couldn't be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntryEditError {
    #[error("the Entry has no custom field at that index")]
    NoSuchField,
    #[error("the Entry has no attachment at that index")]
    NoSuchAttachment,
    #[error("the expiry is not a date written YYYY-MM-DD")]
    BadExpiry,
}

/// Why [`Workspace::edit_entry`] changed nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditEntryError {
    #[error("the Vault is not open")]
    VaultNotOpen,
    #[error("the Entry is not in this Vault")]
    NotFound,
    #[error("the Entry changed since the editor read it")]
    EntryChanged,
    #[error("the Entry has no custom field at that index")]
    NoSuchField,
    #[error("the Entry has no attachment at that index")]
    NoSuchAttachment,
    #[error("the expiry is not a date written YYYY-MM-DD")]
    BadExpiry,
    #[error(transparent)]
    Edit(EditError),
}

impl From<EntryEditError> for EditEntryError {
    fn from(error: EntryEditError) -> Self {
        match error {
            EntryEditError::NoSuchField => Self::NoSuchField,
            EntryEditError::NoSuchAttachment => Self::NoSuchAttachment,
            EntryEditError::BadExpiry => Self::BadExpiry,
        }
    }
}

impl From<EditVaultError> for EditEntryError {
    fn from(error: EditVaultError) -> Self {
        match error {
            EditVaultError::Edit(EditError::EntryNotFound) => Self::NotFound,
            EditVaultError::Edit(error) => Self::Edit(error),
            EditVaultError::VaultNotOpen | EditVaultError::NotPaired => Self::VaultNotOpen,
        }
    }
}

impl EntryEdit {
    /// The Entry's fields after this edit, `current` being the Entry as it is, or `None` for a
    /// new Entry, whose fields come from the edit alone.
    pub fn apply(&self, current: Option<&Entry>) -> Result<EntryFields, EntryEditError> {
        let mut fields = current.map(EntryFields::from).unwrap_or_default();
        let set = |part: &Option<String>, field: &mut String| {
            if let Some(value) = part {
                field.clone_from(value);
            }
        };
        set(&self.title, &mut fields.title);
        set(&self.username, &mut fields.username);
        set(&self.url, &mut fields.url);
        set(&self.notes, &mut fields.notes);
        if let Some(password) = &self.password {
            fields.password = password.clone();
        }
        if let Some(custom_fields) = &self.custom_fields {
            fields.custom_fields = custom_fields
                .iter()
                .map(|field| field.apply(&fields.custom_fields))
                .collect::<Result<_, _>>()?;
        }
        match &self.tags {
            None => {}
            Some(TagsEdit::Replace(texts)) => {
                fields.tags = texts.iter().flat_map(|text| parse_tags(text)).collect();
            }
            Some(TagsEdit::Add(text)) => {
                for tag in parse_tags(text) {
                    if !fields.tags.contains(&tag) {
                        fields.tags.push(tag);
                    }
                }
            }
        }
        if let Some(color) = self.color {
            fields.color = color;
        }
        if let Some(icon) = self.icon {
            fields.icon = icon;
        }
        if let Some(expires) = &self.expires {
            fields.expires = match expires {
                None => None,
                Some(date) => Some(
                    Timestamp::expiry_on(date, current.and_then(Entry::expires))
                        .ok_or(EntryEditError::BadExpiry)?,
                ),
            };
        }
        for edit in &self.attachments {
            match edit {
                AttachmentEdit::Add(attachment) => {
                    attachment.clone().add_to(&mut fields.attachments)
                }
                AttachmentEdit::Remove(index) => {
                    if *index >= fields.attachments.len() {
                        return Err(EntryEditError::NoSuchAttachment);
                    }
                    fields.attachments.remove(*index);
                }
            }
        }
        Ok(fields)
    }
}

impl CustomFieldEdit {
    /// The field, `current` being the Entry's custom fields before the edit.
    fn apply(&self, current: &[CustomField]) -> Result<CustomField, EntryEditError> {
        let value = match &self.value {
            CustomFieldValue::New(value) => value.clone(),
            CustomFieldValue::Kept(index) => current
                .get(*index)
                .map(|kept| kept.value.clone())
                .ok_or(EntryEditError::NoSuchField)?,
            CustomFieldValue::Empty => SecretString::from(String::new()),
        };
        Ok(CustomField {
            name: self.name.trim().to_owned(),
            value,
            protected: self.protected,
        })
    }
}

impl Workspace {
    /// Applies `edit` to an Entry and writes it, in one go, so nothing changes the Entry in
    /// between. With `seen`, the Entry's last modification time as the editor read it, an Entry
    /// changed since (by a Merge or another edit) is left alone and the edit refused with
    /// [`EditEntryError::EntryChanged`], so an editor never silently overwrites a change it
    /// didn't show. A Vault waiting to lock is refused as not open, as it is hidden from the
    /// app.
    ///
    /// The version replaced goes to the Entry's history. `Ok(false)` when the edit left the
    /// Entry as it was: then it records no history, and the Vault is left with nothing new to
    /// save.
    pub fn edit_entry(
        &mut self,
        vault: VaultId,
        entry: EntryId,
        seen: Option<Timestamp>,
        edit: &EntryEdit,
    ) -> Result<bool, EditEntryError> {
        let current = self
            .readable_vault(vault)
            .ok_or(EditEntryError::VaultNotOpen)?
            .vault
            .entry(entry)
            .ok_or(EditEntryError::NotFound)?;
        if let Some(seen) = seen
            && current.last_modified() != Some(seen)
        {
            return Err(EditEntryError::EntryChanged);
        }
        let fields = edit.apply(Some(current))?;
        Ok(self.edit_if_changed(vault, |vault, now| vault.update_entry(entry, &fields, now))?)
    }
}
