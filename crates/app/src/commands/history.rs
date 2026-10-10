//! An Entry's earlier versions, and making one current again.

use keefast_core::Entry;
use secrecy::ExposeSecret;
use serde::Serialize;
use specta::Type;

use super::Core;
use super::entries::{CustomFieldView, custom_field_views, find};
use crate::dto::{Color, EntryId, VaultId, seconds, title_of};
use crate::error::{ApiError, ApiResult};

/// Whether a version has TOTP set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TotpState {
    None,
    Set,
    /// Set up, but its settings can't be read.
    Unreadable,
}

/// A version of an Entry, without its password and protected custom fields.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryVersion {
    /// Its index in the Entry's history, oldest first: what `restore_entry_version` takes.
    pub index: u32,
    /// When it was saved, in seconds since 1970, UTC.
    pub modified: Option<f64>,
    pub title: String,
    pub username: String,
    pub url: String,
    pub notes: String,
    pub has_password: bool,
    /// Whether its password is the Entry's current one: tells a version that changed the
    /// password without showing either.
    pub password_is_current: bool,
    /// Whether its password is the one of the version before it. False for the oldest version.
    pub password_is_previous: bool,
    pub custom_fields: Vec<CustomFieldView>,
    pub tags: Vec<String>,
    pub color: Option<Color>,
    pub totp: TotpState,
    pub attachments: Vec<String>,
    /// The custom fields, by name, it has like the version before it, protected in either, whose
    /// values differ: what a comparison can't tell from the values it is shown. Empty for the
    /// oldest version.
    pub protected_changed_from_previous: Vec<String>,
    /// The same, against the Entry as it is now.
    pub protected_changed_from_current: Vec<String>,
}

fn version(index: usize, history: &[Entry], current: &Entry) -> EntryVersion {
    let version = &history[index];
    let previous = index.checked_sub(1).map(|before| &history[before]);
    EntryVersion {
        index: index as u32,
        modified: version.last_modified().map(seconds),
        title: title_of(version),
        username: version.username().to_owned(),
        url: version.url().to_owned(),
        notes: version.notes().to_owned(),
        has_password: !version.password().expose_secret().is_empty(),
        password_is_current: same_password(version, current),
        password_is_previous: previous.is_some_and(|previous| same_password(version, previous)),
        custom_fields: custom_field_views(version),
        tags: version.tags().to_vec(),
        color: version.color().map(Into::into),
        totp: match version.totp() {
            None => TotpState::None,
            Some(Ok(_)) => TotpState::Set,
            Some(Err(_)) => TotpState::Unreadable,
        },
        attachments: version
            .attachments()
            .iter()
            .map(|attachment| attachment.name.clone())
            .collect(),
        protected_changed_from_previous: previous
            .map(|previous| protected_changes(version, previous))
            .unwrap_or_default(),
        protected_changed_from_current: protected_changes(version, current),
    }
}

fn same_password(a: &Entry, b: &Entry) -> bool {
    a.password().expose_secret() == b.password().expose_secret()
}

/// The custom fields both Entries have, protected in either, whose values differ.
fn protected_changes(a: &Entry, b: &Entry) -> Vec<String> {
    a.custom_fields()
        .iter()
        .filter_map(|field| {
            let other = b
                .custom_fields()
                .iter()
                .find(|other| other.name == field.name)?;
            let protected = field.protected || other.protected;
            let differs = field.value.expose_secret() != other.value.expose_secret();
            (protected && differs).then(|| field.name.clone())
        })
        .collect()
}

/// The Entry's earlier versions, oldest first. Each edit keeps the version it replaces here.
#[tauri::command]
#[specta::specta]
pub async fn get_entry_history(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
) -> ApiResult<Vec<EntryVersion>> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        let history = found.history();
        Ok((0..history.len())
            .map(|index| version(index, history, found))
            .collect())
    })
}

/// The password of a version of the Entry, for the user to see.
#[tauri::command]
#[specta::specta]
pub async fn reveal_version_password(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    version: u32,
) -> ApiResult<String> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        let old = found
            .history()
            .get(version as usize)
            .ok_or_else(ApiError::not_found)?;
        Ok(old.password().expose_secret().to_owned())
    })
}

/// A custom field's value in a version of the Entry, for the user to see.
#[tauri::command]
#[specta::specta]
pub async fn reveal_version_custom_field(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    version: u32,
    index: u32,
) -> ApiResult<String> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        found
            .history()
            .get(version as usize)
            .and_then(|old| old.custom_fields().get(index as usize))
            .map(|field| field.value.expose_secret().to_owned())
            .ok_or_else(ApiError::not_found)
    })
}

/// Makes an earlier version of the Entry current again. The version it replaces goes to the
/// history.
#[tauri::command]
#[specta::specta]
pub async fn restore_entry_version(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    version: u32,
) -> ApiResult<()> {
    let (vault, entry) = core
        .read(|workspace| Ok::<_, ApiError>((vault.resolve_open(workspace)?, entry.resolve()?)))?;
    core.edit(|workspace| workspace.restore_entry_version(vault, entry, version as usize))
}

#[cfg(test)]
mod tests {
    use keefast_core::{CompositeKey, CustomField, EntryFields, Timestamp, Vault};
    use secrecy::SecretString;

    use super::version;

    const KEEPASSXC_KDBX4: &[u8] = include_bytes!("../../../../fixtures/keepassxc-kdbx4.kdbx");

    fn fields(password: &str, pin: &str, note: &str) -> EntryFields {
        let field = |name: &str, value: &str, protected| CustomField {
            name: name.into(),
            value: SecretString::from(value),
            protected,
        };
        EntryFields {
            title: "Bank".into(),
            password: SecretString::from(password),
            custom_fields: vec![field("PIN", pin, true), field("Note", note, false)],
            ..EntryFields::default()
        }
    }

    #[test]
    fn versions_tell_which_secrets_changed_without_showing_them() {
        let mut vault = Vault::open(KEEPASSXC_KDBX4, &CompositeKey::password("fixture-password"))
            .expect("Vault file opens");
        let at = |minute: i64| Timestamp::from_unix_seconds(1_900_000_000 + minute * 60);
        let root = vault.root().id();
        let id = vault
            .create_entry(root, &fields("pw-1", "1111", "a"), at(0))
            .unwrap();
        // The PIN changes, then the password and the unprotected note, then the note alone.
        vault
            .update_entry(id, &fields("pw-1", "2222", "a"), at(1))
            .unwrap();
        vault
            .update_entry(id, &fields("pw-2", "2222", "b"), at(2))
            .unwrap();
        vault
            .update_entry(id, &fields("pw-2", "2222", "c"), at(3))
            .unwrap();
        let entry = vault.entry(id).unwrap();
        let history = entry.history();
        let versions: Vec<_> = (0..history.len())
            .map(|index| version(index, history, entry))
            .collect();
        assert_eq!(versions.len(), 3);

        assert!(!versions[0].password_is_previous);
        assert!(versions[0].protected_changed_from_previous.is_empty());
        assert!(!versions[0].password_is_current);
        assert_eq!(versions[0].protected_changed_from_current, ["PIN"]);

        assert!(versions[1].password_is_previous);
        assert_eq!(versions[1].protected_changed_from_previous, ["PIN"]);
        assert!(versions[1].protected_changed_from_current.is_empty());

        assert!(!versions[2].password_is_previous);
        assert!(versions[2].password_is_current);
        // The note isn't protected: its values show what changed.
        assert!(versions[2].protected_changed_from_previous.is_empty());
    }
}
