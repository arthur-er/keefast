//! `get-totp`: the current TOTP code of an Entry, for the extension to fill a 2FA field.

use serde_json::{Map, Value, json};

use super::{BrowserReply, BrowserSession, ProtocolError, candidates, text};
use crate::Workspace;

/// How long an Entry's UUID is in hex, as the extension sends it.
const UUID_HEX_LEN: usize = 32;

impl BrowserSession {
    /// Answers with the current TOTP code of the Entry the request names. The request names
    /// no site, so the code is given only for an Entry this extension was last given for a
    /// site, and only while that site is still offered the Entry. Any other Entry gets an empty
    /// code, as KeePassXC answers for an Entry without TOTP.
    pub(super) fn get_totp(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, message) = self.decrypt(request)?;
        if text(&message, "action") != "get-totp" {
            return Err(ProtocolError::IncorrectAction);
        }
        let uuid = text(&message, "uuid");
        if uuid.len() != UUID_HEX_LEN || !uuid.chars().all(|digit| digit.is_ascii_hexdigit()) {
            return Err(ProtocolError::NoValidUuidProvided);
        }
        let code = self
            .allowed
            .iter()
            .filter(|((_, entry), _)| entry.to_hex().eq_ignore_ascii_case(uuid))
            .find(|(id, query)| {
                candidates(workspace, query)
                    .iter()
                    .any(|candidate| candidate.id() == **id)
            })
            .and_then(|(&(vault, entry), _)| workspace.totp_code(vault, entry)?.ok())
            .map(|code| code.code().to_owned())
            .unwrap_or_default();
        Ok(BrowserReply::Send(
            self.encrypted_reply(&request, json!({ "totp": code })),
        ))
    }
}
