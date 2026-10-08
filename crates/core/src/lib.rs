//! keefast's domain model and KDBX handling.
//!
//! This crate does no I/O: it turns Vault file bytes into a [`Vault`] and back, and knows nothing
//! about where those bytes came from or go to. KDBX reading and writing is keefast's own (see
//! ADR 0002), so that a save keeps everything in the Vault file.

mod browser;
mod edit;
mod generator;
mod kdbx;
mod key;
mod open;
mod save;
mod site;
mod time;
mod totp;
mod vault;
mod xml;

pub use browser::{AccessDecision, CapturedLogin, DEFAULT_BROWSER_GROUP, Pairing, SiteEntry};
pub use edit::{EditError, EntryFields};
pub use generator::{
    CharacterRecipe, GeneratorError, LOOKALIKES, PassphraseRecipe, PasswordRecipe, SYMBOLS,
};
pub use key::{CompositeKey, KeyFileError};
pub use open::OpenError;
pub use save::SaveError;
pub use site::Site;
pub use time::Timestamp;
pub use totp::{Totp, TotpAlgorithm, TotpCode, TotpError};
pub use vault::{CustomField, Entry, EntryId, Group, GroupId, Vault};
