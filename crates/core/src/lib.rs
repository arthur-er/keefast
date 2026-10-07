//! keefast's domain model and KDBX handling.
//!
//! This crate does no I/O: it turns Vault file bytes into a [`Vault`] and back, and knows nothing
//! about where those bytes came from or go to. KDBX reading and writing is keefast's own (see
//! ADR 0002), so that a save keeps everything in the Vault file.

mod kdbx;
mod key;
mod open;
mod save;
mod vault;
mod xml;

pub use key::CompositeKey;
pub use open::OpenError;
pub use save::SaveError;
pub use vault::{Entry, EntryId, Group, GroupId, Vault};
