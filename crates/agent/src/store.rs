//! `keefast-agent store <Secret reference>#<field>`: saves a value read from standard input
//! into a field of an Entry in the Agent pairing's own subgroup, creating the Entry if needed
//! (ADR 0012 "Agents create secrets").
//!
//! It is how an Agent with a shell keeps a secret it mints, such as a new API token, without
//! seeing it: `mint-token | keefast-agent store claude/new#token`. The value goes from the pipe
//! to keefast's endpoint at [`STORE_PATH`], beside `/mcp` and behind the same bearer check,
//! never through MCP or the model, and keefast answers only with the Entry's Secret reference.
//! It reaches keefast as `run` does, through `KEEFAST_AGENT_URL` and `KEEFAST_AGENT_TOKEN`.

use std::io::Read;

use zeroize::Zeroizing;

use crate::run::Connection;
use crate::{StoreSecret, WrittenEntry};

/// Where the Headless host answers `store`, beside its MCP endpoint at `/mcp`.
pub const STORE_PATH: &str = "/agent/store";

/// The most `store` reads from its input: plenty for a token or a key, and a bound on a pipe
/// left open by mistake.
pub const MAX_VALUE: usize = 64 * 1024;

/// Reads `store`'s one argument, the Secret reference of the field to set.
pub fn parse(args: &[String]) -> Result<String, String> {
    match args {
        [reference] if !reference.starts_with('-') => Ok(reference.clone()),
        [] => Err(
            "store needs the Secret reference to save to, such as claude/new#token, and \
                   the value on its input"
                .to_owned(),
        ),
        _ => Err(
            "store takes one Secret reference, such as claude/new#token, and reads the \
                  value from its input"
                .to_owned(),
        ),
    }
}

/// The value on `input`, without the one line break that ends it, as `echo` and most minting
/// commands end their output with one. Anything else, spaces included, is kept as it is.
pub fn read_value(mut input: impl Read) -> Result<Zeroizing<String>, String> {
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .by_ref()
        .take(MAX_VALUE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("can't read the value from the input: {error}"))?;
    if bytes.len() > MAX_VALUE {
        return Err(format!(
            "the value is longer than {MAX_VALUE} bytes, which keefast doesn't store"
        ));
    }
    let mut value = Zeroizing::new(
        String::from_utf8(std::mem::take(&mut *bytes))
            .map_err(|_| "the value isn't UTF-8 text, which KeePass fields hold".to_owned())?,
    );
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    if value.is_empty() {
        return Err(
            "there is nothing to store: pipe the value in, such as mint-token | keefast-agent \
             store claude/new#token"
                .to_owned(),
        );
    }
    Ok(value)
}

/// Saves `value` to the field `reference` names, through `connection`.
pub fn store(
    connection: &Connection,
    reference: &str,
    value: Zeroizing<String>,
) -> Result<WrittenEntry, String> {
    let request = StoreSecret::new(reference.to_owned(), value);
    let body = Zeroizing::new(serde_json::to_string(&request).expect("a request is plain JSON"));
    connection.post(STORE_PATH, &body)
}
