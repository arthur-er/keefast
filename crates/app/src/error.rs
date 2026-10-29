//! What a command tells the web UI when it fails: a kind to branch on, and a sentence to show,
//! in the words the GPUI app used.

use keefast_core::{
    EditError, EncryptionError, GeneratorError, KeyFileError, OpenError, TotpError,
};
use keefast_workspace::{
    ChangeKeyError, CopyError, CreateVaultError, DropboxError, EditVaultError, ImportIconError,
    NewKeyError, OpenVaultError, SaveVaultError, StorageError,
};
use serde::Serialize;
use specta::Type;

/// A failed command. `message` is a whole sentence for the user; `detail` is the technical
/// reason, when there is one worth showing under it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub kind: ErrorKind,
    pub message: String,
    pub detail: Option<String>,
}

/// What went wrong, for the UI to branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The Vault, Entry, Group, attachment or request is gone, or the Vault is locked.
    NotFound,
    /// The master password or Key file doesn't unlock the Vault file.
    WrongKey,
    /// What the user typed can't be used; `message` says why.
    Invalid,
    /// A Vault from that Vault file is open already.
    AlreadyOpen,
    /// The field to copy is empty, so nothing was copied.
    Empty,
    /// A file couldn't be read or written.
    Io,
    /// Dropbox failed; `message` says how.
    Dropbox,
    /// The Vault file changed elsewhere in a way that can't be Merged, or is being saved.
    Conflict,
    /// Something keefast can't do in this build or on this machine.
    Unavailable,
    /// The user stopped it.
    Cancelled,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn not_found() -> Self {
        Self::new(ErrorKind::NotFound, ERROR_NOT_FOUND)
    }

    pub fn vault_not_found() -> Self {
        Self::new(ErrorKind::NotFound, "That Vault is no longer open.")
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }

    pub fn picker() -> Self {
        Self::new(
            ErrorKind::Unavailable,
            "The file picker could not be opened.",
        )
    }
}

const ERROR_NOT_FOUND: &str = "That Entry or Group no longer exists.";

impl From<EditVaultError> for ApiError {
    fn from(error: EditVaultError) -> Self {
        let message = match &error {
            EditVaultError::Edit(EditError::IntoItself) => {
                "A Group cannot be moved into itself or its subgroups."
            }
            EditVaultError::Edit(EditError::RootGroup) => {
                "The top Group of a Vault cannot be moved or deleted."
            }
            EditVaultError::Edit(EditError::EntryNotFound | EditError::GroupNotFound)
            | EditVaultError::VaultNotOpen => return Self::not_found(),
            EditVaultError::Edit(EditError::PairingNameEmpty) => "Give this browser a name.",
            EditVaultError::Edit(EditError::PairingNameTaken) => {
                "This Vault already has a browser with this name."
            }
            EditVaultError::Edit(EditError::PairingNotFound) => {
                "This browser is no longer paired with the Vault."
            }
            EditVaultError::NotPaired => "The browser is not paired with this Vault.",
            EditVaultError::Edit(EditError::InRecycleBin) => {
                "Logins can't be saved in the Recycle Bin."
            }
            EditVaultError::Edit(EditError::FieldNameEmpty) => "Give every custom field a name.",
            EditVaultError::Edit(EditError::FieldNameTaken) => {
                "Each field needs a name of its own, other than Title, UserName, Password, URL or \
                 Notes."
            }
            EditVaultError::Edit(EditError::IconNotFound) => {
                "That icon is no longer in this Vault."
            }
            EditVaultError::Edit(EditError::IconNotPng) => "An icon must be a PNG image.",
            EditVaultError::Edit(EditError::IconTooLarge) => "That image is too large for an icon.",
            EditVaultError::Edit(EditError::VersionNotFound) => {
                "That version is no longer in the Entry's history."
            }
            EditVaultError::Edit(EditError::AttachmentNameEmpty) => "Give every attachment a name.",
            EditVaultError::Edit(EditError::AttachmentNameTaken) => {
                "Each attachment of an Entry needs a name of its own."
            }
        };
        Self::invalid(message)
    }
}

impl From<OpenVaultError> for ApiError {
    /// Tells a wrong key, a damaged or unsupported Vault file and Key file problems apart.
    fn from(error: OpenVaultError) -> Self {
        use ErrorKind::*;
        let (kind, message, detail) = match error {
            OpenVaultError::AlreadyOpen(_) => (
                AlreadyOpen,
                "A Vault from this Vault file is already open.",
                None,
            ),
            OpenVaultError::Vault(OpenError::WrongKey) => {
                (WrongKey, "Wrong master password or Key file.", None)
            }
            OpenVaultError::Vault(OpenError::Corrupt(reason)) => (
                Invalid,
                "This Vault file is damaged or is not a Vault file.",
                Some(reason),
            ),
            OpenVaultError::Vault(OpenError::Unsupported(reason)) => (
                Invalid,
                "This Vault file format is not supported.",
                Some(reason),
            ),
            OpenVaultError::Vault(OpenError::KdfOverLimit(error)) => (
                Invalid,
                "This Vault file's key derivation settings are too high for keefast to unlock \
                 it.",
                Some(describe_encryption(error).to_owned()),
            ),
            OpenVaultError::Storage(StorageError::NotFound(_)) => {
                (NotFound, "The Vault file could not be found.", None)
            }
            OpenVaultError::Storage(StorageError::Io(error)) => (
                Io,
                "The Vault file could not be read.",
                Some(error.to_string()),
            ),
            OpenVaultError::Storage(StorageError::Conflict) => {
                (Io, "The Vault file could not be read.", None)
            }
            OpenVaultError::Storage(StorageError::Unavailable(_)) => (
                Unavailable,
                "This Vault file is in Dropbox, which this build of keefast can't reach.",
                None,
            ),
            OpenVaultError::Storage(StorageError::Offline(reason)) => (
                Dropbox,
                "Dropbox can't be reached, and this Vault file has not been opened on this \
                 machine before.",
                Some(reason),
            ),
            OpenVaultError::Storage(StorageError::Dropbox(error)) => return error.into(),
            OpenVaultError::KeyFileNotFound(_) => {
                (NotFound, "The Key file could not be found.", None)
            }
            OpenVaultError::KeyFileUnreadable(error) => (
                Io,
                "The Key file could not be read.",
                Some(error.to_string()),
            ),
            OpenVaultError::KeyFile(KeyFileError::Corrupt(reason)) => {
                (Invalid, "This Key file is damaged.", Some(reason))
            }
            OpenVaultError::KeyFile(KeyFileError::Unsupported(reason)) => (
                Invalid,
                "This Key file format is not supported.",
                Some(reason),
            ),
        };
        Self {
            kind,
            message: message.to_owned(),
            detail,
        }
    }
}

impl From<DropboxError> for ApiError {
    fn from(error: DropboxError) -> Self {
        let message = match &error {
            DropboxError::NotConfigured => {
                return Self::new(
                    ErrorKind::Unavailable,
                    "This build of keefast has no Dropbox app key.",
                );
            }
            DropboxError::Cancelled => {
                return Self::new(ErrorKind::Cancelled, "Connecting Dropbox was stopped.");
            }
            DropboxError::NotConnected | DropboxError::AccessRevoked => {
                "Dropbox no longer lets keefast in. Connect your account again."
            }
            DropboxError::Refused(_) => "Dropbox was not connected.",
            DropboxError::Callback(_) => {
                "keefast could not wait for Dropbox's answer. Is another keefast connecting \
                 Dropbox?"
            }
            DropboxError::Network(_) => "Dropbox can't be reached. Check your connection.",
            DropboxError::NotFound => "That is no longer in Dropbox.",
            DropboxError::Conflict => "The Vault file changed in Dropbox since keefast read it.",
            DropboxError::Api(_) => "Dropbox reported an error.",
            DropboxError::SecretStore(_) => {
                "The OS secret store isn't available, so keefast can't keep your Dropbox account \
                 connected."
            }
        };
        Self::new(ErrorKind::Dropbox, message).with_detail(error.to_string())
    }
}

fn describe_encryption(error: EncryptionError) -> &'static str {
    match error {
        EncryptionError::Iterations => "Argon2 needs 1 to 4294967295 iterations.",
        EncryptionError::Parallelism => "Argon2's parallelism must be between 1 and 16777215.",
        EncryptionError::Memory => {
            "Argon2's memory must be at least 8 KiB per unit of parallelism, and at most 4 GiB."
        }
        EncryptionError::Cost => {
            "Argon2's memory times its iterations can't be more than 128 GiB. Lower one of them."
        }
        EncryptionError::Rounds => "AES-KDF needs 1 to 10000000000 rounds.",
    }
}

impl From<EncryptionError> for ApiError {
    fn from(error: EncryptionError) -> Self {
        Self::invalid(describe_encryption(error))
    }
}

impl From<NewKeyError> for ApiError {
    fn from(error: NewKeyError) -> Self {
        match error {
            NewKeyError::NoKey => Self::invalid("Set a master password, a Key file, or both."),
            NewKeyError::KeyFileNotFound(_) => {
                Self::new(ErrorKind::NotFound, "The Key file could not be found.")
            }
            NewKeyError::KeyFileUnreadable(error) => {
                Self::new(ErrorKind::Io, "The Key file could not be read.")
                    .with_detail(error.to_string())
            }
            NewKeyError::KeyFile(KeyFileError::Corrupt(reason)) => {
                Self::invalid("This Key file is damaged.").with_detail(reason)
            }
            NewKeyError::KeyFile(KeyFileError::Unsupported(reason)) => {
                Self::invalid("This Key file format is not supported.").with_detail(reason)
            }
        }
    }
}

impl From<CreateVaultError> for ApiError {
    fn from(error: CreateVaultError) -> Self {
        match error {
            CreateVaultError::AlreadyOpen(_) => Self::new(
                ErrorKind::AlreadyOpen,
                "A Vault from this Vault file is open. Choose another file, or lock and reopen \
                 that Vault.",
            ),
            CreateVaultError::FileExists => Self::new(
                ErrorKind::Conflict,
                "A file is already there. keefast never replaces a file with a new Vault.",
            ),
            CreateVaultError::Key(error) => error.into(),
            CreateVaultError::Encryption(error) => error.into(),
            CreateVaultError::Save(error) => {
                Self::invalid("The Vault could not be encrypted with these settings.")
                    .with_detail(error.to_string())
            }
            CreateVaultError::Storage(StorageError::Unavailable(_)) => Self::new(
                ErrorKind::Unavailable,
                "This Vault file is in Dropbox, which this build of keefast can't reach.",
            ),
            CreateVaultError::Storage(error) => {
                Self::new(ErrorKind::Io, "The Vault file could not be written there.")
                    .with_detail(error.to_string())
            }
        }
    }
}

impl From<ChangeKeyError> for ApiError {
    fn from(error: ChangeKeyError) -> Self {
        match error {
            ChangeKeyError::VaultNotOpen => Self::vault_not_found(),
            ChangeKeyError::Key(error) => error.into(),
            ChangeKeyError::Encryption(error) => error.into(),
        }
    }
}

impl From<SaveVaultError> for ApiError {
    fn from(error: SaveVaultError) -> Self {
        let detail = error.to_string();
        let (kind, message) = match &error {
            SaveVaultError::VaultNotOpen => return Self::vault_not_found(),
            SaveVaultError::InProgress => (
                ErrorKind::Conflict,
                "The Vault is being saved; it is saved again once that is done.",
            ),
            SaveVaultError::Merge(_) => (
                ErrorKind::Conflict,
                "The Vault file was changed elsewhere with another master key, so its changes \
                 can't be Merged. Nothing was overwritten.",
            ),
            SaveVaultError::Storage(StorageError::Offline(_)) => (
                ErrorKind::Dropbox,
                "Dropbox can't be reached. Check your connection.",
            ),
            SaveVaultError::Storage(StorageError::Conflict) => (
                ErrorKind::Conflict,
                "The Vault file kept changing while keefast saved it. Try again.",
            ),
            SaveVaultError::Vault(_) | SaveVaultError::Storage(_) => {
                (ErrorKind::Io, "The Vault could not be saved.")
            }
        };
        Self::new(kind, message).with_detail(detail)
    }
}

impl From<CopyError> for ApiError {
    fn from(error: CopyError) -> Self {
        match error {
            CopyError::NotFound => Self::not_found(),
            CopyError::Empty => Self::new(
                ErrorKind::Empty,
                "That field is empty, so nothing was copied.",
            ),
        }
    }
}

impl From<ImportIconError> for ApiError {
    fn from(error: ImportIconError) -> Self {
        match error {
            ImportIconError::Unreadable(error) => {
                Self::new(ErrorKind::Io, "The image could not be read.")
                    .with_detail(error.to_string())
            }
            ImportIconError::Edit(error) => error.into(),
        }
    }
}

impl From<GeneratorError> for ApiError {
    fn from(error: GeneratorError) -> Self {
        Self::invalid(match error {
            GeneratorError::NoCharacterClasses => "Choose at least one kind of character.".into(),
            GeneratorError::LengthOutOfRange { min, max } => {
                format!("The length must be between {min} and {max}.")
            }
            GeneratorError::WordCountOutOfRange { min, max } => {
                format!("The number of words must be between {min} and {max}.")
            }
        })
    }
}

/// Why TOTP settings can't be read, for the setup form and for an Entry whose settings are
/// broken.
pub fn describe_totp(error: TotpError) -> &'static str {
    match error {
        TotpError::MissingSecret => "Enter the secret key or otpauth:// URI.",
        TotpError::InvalidSecret => {
            "The secret key isn't valid: use the letters A–Z and digits 2–7."
        }
        TotpError::InvalidDigits => "A code has 1 to 10 digits.",
        TotpError::InvalidPeriod => "The period is 1 to 86400 seconds.",
        TotpError::NotTotp => "This isn't an otpauth://totp/ URI.",
    }
}

impl From<TotpError> for ApiError {
    fn from(error: TotpError) -> Self {
        Self::invalid(describe_totp(error))
    }
}

impl From<tauri::Error> for ApiError {
    fn from(error: tauri::Error) -> Self {
        Self::new(ErrorKind::Unavailable, "keefast could not do that.")
            .with_detail(error.to_string())
    }
}
