use gpui::{App, ClipboardItem};
use keefast_workspace::Clipboard;

/// The system clipboard, as the Workspace sees it.
pub struct AppClipboard<'a>(pub &'a mut App);

impl Clipboard for AppClipboard<'_> {
    fn read_text(&self) -> Option<String> {
        self.0.read_from_clipboard()?.text()
    }

    fn write_text(&mut self, text: &str) {
        self.0
            .write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    }
}
