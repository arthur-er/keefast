//! Gateway pairings (ADR 0015): another program's vault, such as agentes-chat's, that lists a
//! pairing's Entries as its own items and resolves their values on its agents' behalf, through
//! the Headless host's `/gateway/v1/` API rather than MCP.
//!
//! [`GatewaySession`] decides what a Gateway pairing may see and do, once, for every host;
//! [`gateway_item`] is the Entry mapping it lists Entries by.

mod items;
mod session;

pub use items::{GatewayItem, ItemDetails, Matching, Website, gateway_item, reference};
pub use session::{GatewayError, GatewaySession, Listing, Whoami};
