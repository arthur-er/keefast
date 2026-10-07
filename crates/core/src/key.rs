use secrecy::{ExposeSecret, SecretString};

/// The secrets that unlock a Vault file. Only a master password for now; key files come later.
#[derive(Clone)]
pub struct CompositeKey {
    password: SecretString,
}

impl CompositeKey {
    pub fn password(password: impl Into<String>) -> Self {
        Self {
            password: SecretString::from(password.into()),
        }
    }

    pub(crate) fn to_database_key(&self) -> keepass::DatabaseKey {
        keepass::DatabaseKey::new().with_password(self.password.expose_secret())
    }
}
