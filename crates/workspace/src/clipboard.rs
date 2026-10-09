//! Copying Entry fields to the clipboard, and clearing them again so they don't linger
//! (ADR 0004).

use std::time::{Duration, SystemTime};

use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

/// The system clipboard. Passed in with each call, as the app's clipboard lives in its UI
/// context, and so tests can fake it.
pub trait Clipboard {
    fn read_text(&self) -> Option<String>;
    fn write_text(&mut self, text: &str);
}

/// An Entry field that can be copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyField {
    Username,
    Password,
    /// The Entry's TOTP code now. Empty when the Entry has no TOTP, or its settings can't be
    /// read.
    TotpCode,
    /// One of the Entry's custom fields, by its position in `Entry::custom_fields`.
    CustomField(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CopyError {
    #[error("that Entry no longer exists")]
    NotFound,
    #[error("that field of the Entry is empty")]
    Empty,
}

/// What keefast last put on the clipboard and when to clear it, until it is cleared.
#[derive(Default)]
pub(crate) struct PendingClear(Option<(SecretString, SystemTime)>);

impl PendingClear {
    /// Puts `text` on the clipboard, to be cleared at `clear_at`, or never with `None`.
    pub fn copy(
        &mut self,
        clipboard: &mut dyn Clipboard,
        text: &str,
        clear_at: Option<SystemTime>,
    ) {
        clipboard.write_text(text);
        self.0 = clear_at.map(|at| (SecretString::from(text), at));
    }

    pub fn clears_in(&self, now: SystemTime) -> Option<Duration> {
        let (_, at) = self.0.as_ref()?;
        Some(at.duration_since(now).unwrap_or_default())
    }

    /// Clears the clipboard once it is time, but only if it still holds what keefast put
    /// there: whatever the user copied since is theirs.
    pub fn clear_if_due(&mut self, clipboard: &mut dyn Clipboard, now: SystemTime) {
        let Some((_, at)) = &self.0 else {
            return;
        };
        if now >= *at {
            self.clear_now(clipboard);
        }
    }

    /// Clears the clipboard now if it still holds what keefast put there, as when quitting.
    pub fn clear_now(&mut self, clipboard: &mut dyn Clipboard) {
        let Some((copied, _)) = &self.0 else {
            return;
        };
        let current = clipboard.read_text().map(Zeroizing::new);
        if current.as_deref().map(String::as_str) == Some(copied.expose_secret()) {
            clipboard.write_text("");
        }
        self.0 = None;
    }
}
