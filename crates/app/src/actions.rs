//! The keyboard actions of the three panes and the keys bound to them. Each is a GPUI action,
//! so a command palette can list the ones available with `Window::available_actions` and show
//! their keys with `Window::bindings_for_action`.

use gpui::{App, KeyBinding, actions};

actions!(
    vault_browser,
    [
        SelectNextEntry,
        SelectPreviousEntry,
        SelectFirstEntry,
        SelectLastEntry,
        /// Opens the selected Entry for editing.
        OpenEntry,
        CopyUsername,
        CopyPassword,
        SaveAll,
        OpenVault,
    ]
);

/// The key context of the three panes.
pub const CONTEXT: &str = "VaultBrowser";
/// The key context of a text field, where keys type rather than act on the Entry list.
pub const TEXT_FIELD_CONTEXT: &str = "TextField";

pub fn bind_keys(cx: &mut App) {
    let list = Some("VaultBrowser && !TextField");
    let anywhere = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("down", SelectNextEntry, list),
        KeyBinding::new("up", SelectPreviousEntry, list),
        KeyBinding::new("home", SelectFirstEntry, list),
        KeyBinding::new("end", SelectLastEntry, list),
        KeyBinding::new("enter", OpenEntry, list),
        KeyBinding::new("secondary-b", CopyUsername, list),
        KeyBinding::new("secondary-c", CopyPassword, list),
        KeyBinding::new("secondary-s", SaveAll, anywhere),
        KeyBinding::new("secondary-o", OpenVault, anywhere),
    ]);
}
