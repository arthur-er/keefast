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

pub const SIDEBAR_ALL_ENTRIES: &str = "All Entries";
pub const LIST_EMPTY: &str = "No Entries";
pub const DETAIL_EMPTY: &str = "Select an Entry";
pub const DETAIL_USERNAME: &str = "Username";
pub const DETAIL_PASSWORD: &str = "Password";
pub const DETAIL_URL: &str = "URL";
pub const DETAIL_NOTES: &str = "Notes";
pub const UNTITLED: &str = "(untitled)";
pub const HIDDEN_PASSWORD: &str = "••••••••";
pub const PASSWORD_MASK_CHAR: &str = "•";
