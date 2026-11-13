//! `generate-password`: a new password for a sign-up form, made with the user's generator
//! settings.

use secrecy::ExposeSecret;
use serde_json::{Map, Value, json};

use super::{BrowserReply, BrowserSession, ProtocolError};
use crate::Workspace;

impl BrowserSession {
    /// Answers with a password made with the user's generator settings. KeePassXC shows its
    /// generator and waits for the user to apply a password; keefast answers at once, since
    /// the settings are the user's choice already. Settings that can't make a password turn
    /// the request down.
    pub(super) fn generate_password(
        &mut self,
        workspace: &Workspace,
        request: &Map<String, Value>,
    ) -> Result<BrowserReply, ProtocolError> {
        let (request, _) = self.decrypt(request)?;
        let password = workspace
            .generate_password()
            .map_err(|_| ProtocolError::ActionCancelledOrDenied)?;
        Ok(BrowserReply::Send(self.encrypted_reply(
            &request,
            json!({ "password": password.expose_secret() }),
        )))
    }
}
