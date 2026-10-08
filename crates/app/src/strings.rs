//! Every user-facing string, in one place so the UI is ready for translation.

pub const APP_NAME: &str = "keefast";

pub const UNLOCK_TITLE: &str = "Open a Vault";
pub const UNLOCK_NO_FILE: &str = "No Vault file selected";
pub const UNLOCK_CHOOSE_FILE: &str = "Choose Vault file…";
pub const UNLOCK_CHOOSE_FILE_PROMPT: &str = "Open Vault file";
pub const UNLOCK_PASSWORD_LABEL: &str = "Master password";
pub const UNLOCK_PASSWORD_PLACEHOLDER: &str = "Type your master password";
pub const UNLOCK_BUTTON: &str = "Unlock";
pub const UNLOCK_IN_PROGRESS: &str = "Unlocking…";
pub const UNLOCK_ERROR_WRONG_KEY: &str = "Wrong master password.";
pub const UNLOCK_ERROR_CORRUPT: &str = "This Vault file is damaged or is not a Vault file.";
pub const UNLOCK_ERROR_UNSUPPORTED: &str = "This Vault file format is not supported.";
pub const UNLOCK_ERROR_NOT_FOUND: &str = "The Vault file could not be found.";
pub const UNLOCK_ERROR_READ: &str = "The Vault file could not be read.";
pub const UNLOCK_ERROR_PICKER: &str = "The file picker could not be opened.";
pub const UNLOCK_ERROR_ALREADY_OPEN: &str = "A Vault from this Vault file is already open.";
pub const RECENT_VAULTS: &str = "Recent Vault files";
pub const RECENT_FORGET: &str = "×";
pub const CONFIG_ERROR: &str =
    "keefast's settings file could not be read or saved, so recent Vault files may be forgotten.";

pub const SIDEBAR_VAULTS: &str = "Vaults";
pub const SIDEBAR_ALL_VAULTS: &str = "All Vaults";
pub const OPEN_VAULT: &str = "+ Vault";

pub const LIST_EMPTY: &str = "No Entries";
pub const DETAIL_EMPTY: &str = "Select an Entry";
pub const DETAIL_USERNAME: &str = "Username";
pub const DETAIL_PASSWORD: &str = "Password";
pub const DETAIL_URL: &str = "URL";
pub const DETAIL_NOTES: &str = "Notes";
pub const UNTITLED: &str = "(untitled)";
pub const HIDDEN_PASSWORD: &str = "••••••••";
pub const PASSWORD_MASK_CHAR: &str = "•";

pub const CARET: &str = "▏";
pub const CANCEL: &str = "Cancel";
pub const FIELD_TITLE: &str = "Title";
pub const EDITOR_DONE: &str = "Done";
pub const REVEAL_PASSWORD: &str = "Show";
pub const HIDE_PASSWORD: &str = "Hide";

pub const NEW_ENTRY: &str = "+ Entry";
pub const NEW_GROUP: &str = "+ Group";
pub const NEW_GROUP_NAME: &str = "New Group";
pub const GROUP_NAME_PLACEHOLDER: &str = "Group name";
pub const EDIT: &str = "Edit";
pub const RENAME: &str = "Rename";
pub const MOVE: &str = "Move…";
pub const DELETE: &str = "Delete";
pub const MOVE_TO: &str = "Move to:";

pub const SAVE: &str = "Save";
pub const UNSAVED_MARKER: &str = " •";
pub const CHECKBOX_ON: &str = "☑";
pub const CHECKBOX_OFF: &str = "☐";
pub const AUTOSAVE: &str = "Autosave";
pub const STATUS_SAVED: &str = "All changes saved";
pub const STATUS_UNSAVED: &str = "Unsaved changes";
pub const STATUS_SAVING: &str = "Saving…";
pub const STATUS_SAVE_FAILED: &str = "Save failed";

pub const CONFIRM_DELETE_ENTRY: &str = "Delete this Entry for good?";
pub const CONFIRM_DELETE_GROUP: &str = "Delete this Group and everything in it for good?";
pub const CONFIRM_DELETE_DETAIL: &str = "This cannot be undone.";

pub const ERROR_MOVE_INTO_ITSELF: &str = "A Group cannot be moved into itself or its subgroups.";
pub const ERROR_ROOT_GROUP: &str = "The top Group of a Vault cannot be moved or deleted.";
pub const ERROR_NOT_FOUND: &str = "That Entry or Group no longer exists.";

pub const BROWSER_INTEGRATION: &str = "Browser integration";
pub const BROWSER_HOSTS_FOR: &str = "Set up for ";
pub const BROWSER_HOSTS_NONE: &str =
    "No supported browser found (Chrome, Chromium, Brave, Edge, Vivaldi).";
pub const BROWSER_HOSTS_FAILED: &str = "Browser integration could not be set up.";
pub const BROWSER_ALREADY_SERVED: &str = "Another keefast is already serving the browser.";

pub const PAIRING_TITLE: &str = "Pair a browser";
pub const PAIRING_PROMPT: &str = "A browser extension asks to pair with the Vault “{vault}”. If \
this was you, name this browser so you can recognise it later.";
pub const PAIRING_NAME_LABEL: &str = "Browser name";
pub const PAIRING_NAME_PLACEHOLDER: &str = "e.g. Chrome on my laptop";
pub const PAIRING_APPROVE: &str = "Pair";
pub const PAIRING_DENY: &str = "Deny";
pub const ERROR_PAIRING_NAME_EMPTY: &str = "Give this browser a name.";
pub const ERROR_PAIRING_NAME_TAKEN: &str = "This Vault already has a browser with this name.";

pub const ACCESS_TITLE: &str = "Allow access?";
pub const ACCESS_PROMPT: &str = "{site} asks for these Entries. Allow only if you are signing in \
there.";
pub const ACCESS_REMEMBER: &str = "Remember this decision for {site}";
pub const ACCESS_ALLOW: &str = "Allow";
pub const ACCESS_DENY: &str = "Deny";
