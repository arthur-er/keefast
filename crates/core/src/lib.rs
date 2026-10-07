//! keefast's domain model and KDBX handling.
//!
//! This crate does no I/O: it turns Vault file bytes into a [`Vault`] and knows nothing about
//! where those bytes came from. KDBX parsing is delegated to the `keepass` crate, which stays
//! hidden behind these domain types so it can be swapped (see ADR 0002).

mod key;
mod open;
mod vault;

pub use key::CompositeKey;
pub use open::OpenError;
pub use vault::{Entry, EntryId, Group, GroupId, Vault};
