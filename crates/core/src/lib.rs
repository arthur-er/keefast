//! keefast's domain model and KDBX handling.
//!
//! This crate does no I/O: it turns Vault file bytes into a [`Vault`] and back, and knows nothing
//! about where those bytes came from or go to. KDBX reading and writing is keefast's own (see
//! ADR 0002), so that a save keeps everything in the Vault file.

mod appearance;
mod browser;
mod edit;
mod fuzzy;
mod generator;
mod kdbx;
mod key;
mod merge;
mod open;
mod save;
mod search;
mod site;
mod time;
mod totp;
mod vault;
mod xml;

pub use appearance::{
    CustomIcon, CustomIconId, EntryColor, EntryIcon, MAX_CUSTOM_ICON_BYTES, STANDARD_ICON_COUNT,
    parse_tags,
};
pub use browser::{
    AccessDecision, CapturedLogin, DEFAULT_BROWSER_GROUP, DefaultAccessPolicy, Pairing,
    RememberedAccess, SiteEntry,
};
pub use edit::{EditError, EntryFields};
pub use fuzzy::{FuzzyMatch, fuzzy_match};
pub use generator::{
    CharacterRecipe, GeneratorError, LOOKALIKES, PassphraseRecipe, PasswordRecipe, SYMBOLS,
};
pub use key::{CompositeKey, KeyFileError};
pub use merge::MergeSummary;
pub use open::OpenError;
pub use save::SaveError;
pub use search::{EntrySearch, EntrySort};
pub use site::Site;
pub use time::Timestamp;
pub use totp::{Totp, TotpAlgorithm, TotpCode, TotpError};
pub use vault::{CustomField, Entry, EntryId, Group, GroupId, Vault};
