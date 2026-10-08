//! The keyboard actions and the keys bound to them. Each is a GPUI action, so the command
//! palette lists every one the focused view handles, by a name made from the action's: an action
//! appears there once a view registers it with `on_action`.

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
        CopyTotp,
        /// Copies the focused custom field of the selected Entry.
        CopyCustomField,
        SaveAll,
        OpenVault,
        /// Locks every open Vault.
        LockAll,
    ]
);

actions!(
    command_palette,
    [
        /// Opens the command palette, or closes it.
        ToggleCommandPalette,
        SelectNextCommand,
        SelectPreviousCommand,
    ]
);

/// The key context of the three panes.
pub const CONTEXT: &str = "VaultBrowser";
/// The key context of a text field, where keys type rather than act on the Entry list.
pub const TEXT_FIELD_CONTEXT: &str = "TextField";
/// The key context of a custom field in the Entry detail, which takes Enter and Ctrl+C for
/// itself.
pub const CUSTOM_FIELD_CONTEXT: &str = "CustomField";
/// The key context of the command palette.
pub const COMMAND_PALETTE_CONTEXT: &str = "CommandPalette";

pub fn bind_keys(cx: &mut App) {
    let list = format!("{CONTEXT} && !{TEXT_FIELD_CONTEXT}");
    let list = Some(list.as_str());
    // Enter and Ctrl+C belong to a focused custom field, not to the Entry.
    let list_without_fields =
        format!("{CONTEXT} && !{TEXT_FIELD_CONTEXT} && !{CUSTOM_FIELD_CONTEXT}");
    let list_without_fields = Some(list_without_fields.as_str());
    let custom_field = format!("{CONTEXT} && {CUSTOM_FIELD_CONTEXT}");
    let custom_field = Some(custom_field.as_str());
    let anywhere = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("down", SelectNextEntry, list),
        KeyBinding::new("up", SelectPreviousEntry, list),
        KeyBinding::new("home", SelectFirstEntry, list),
        KeyBinding::new("end", SelectLastEntry, list),
        KeyBinding::new("enter", OpenEntry, list_without_fields),
        KeyBinding::new("secondary-b", CopyUsername, list),
        KeyBinding::new("secondary-c", CopyPassword, list_without_fields),
        KeyBinding::new("secondary-t", CopyTotp, list),
        KeyBinding::new("enter", CopyCustomField, custom_field),
        KeyBinding::new("secondary-c", CopyCustomField, custom_field),
        KeyBinding::new("secondary-s", SaveAll, anywhere),
        KeyBinding::new("secondary-o", OpenVault, anywhere),
        KeyBinding::new("secondary-shift-p", ToggleCommandPalette, None),
        KeyBinding::new("down", SelectNextCommand, Some(COMMAND_PALETTE_CONTEXT)),
        KeyBinding::new("up", SelectPreviousCommand, Some(COMMAND_PALETTE_CONTEXT)),
        KeyBinding::new("secondary-l", LockAll, anywhere),
    ]);
}
