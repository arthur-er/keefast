//! The system clipboard, as the Workspace sees it. The Workspace decides what to copy and when
//! to clear it (ADR 0004); this only reads and writes text.
//!
//! The clipboard is opened on first use and kept for as long as the app runs: on X11 what was
//! copied is served by the program that copied it, so it would vanish with a short-lived handle.

use std::cell::RefCell;

use keefast_workspace::Clipboard;

#[derive(Default)]
pub struct SystemClipboard {
    /// `None` until first used, and again after the clipboard couldn't be opened, to try again
    /// next time. In a cell because the Workspace reads it through `&self`.
    inner: RefCell<Option<arboard::Clipboard>>,
}

impl SystemClipboard {
    fn with<R>(&self, use_it: impl FnOnce(&mut arboard::Clipboard) -> R) -> Option<R> {
        let mut inner = self.inner.borrow_mut();
        if inner.is_none() {
            match arboard::Clipboard::new() {
                Ok(clipboard) => *inner = Some(clipboard),
                Err(error) => eprintln!("keefast: can't open the clipboard: {error}"),
            }
        }
        inner.as_mut().map(use_it)
    }
}

impl Clipboard for SystemClipboard {
    fn read_text(&self) -> Option<String> {
        self.with(|clipboard| clipboard.get_text().ok()).flatten()
    }

    fn write_text(&mut self, text: &str) {
        let written = self.with(|clipboard| clipboard.set_text(text.to_owned()));
        if let Some(Err(error)) = written {
            eprintln!("keefast: can't write to the clipboard: {error}");
        }
    }
}
