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
pub const COPIED_USERNAME: &str = "Username copied.";
pub const COPIED_PASSWORD: &str = "Password copied.";
pub const NOTHING_TO_COPY: &str = "That field is empty, so nothing was copied.";
pub const COPIED_CLEARS_IN: &str = "The clipboard clears in {seconds} s.";
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

pub const GENERATE_PASSWORD: &str = "Generate";
pub const GENERATOR_CHARACTERS: &str = "Characters";
pub const GENERATOR_PASSPHRASE: &str = "Passphrase";
pub const GENERATOR_LENGTH: &str = "Length";
pub const GENERATOR_LOWERCASE: &str = "a–z";
pub const GENERATOR_UPPERCASE: &str = "A–Z";
pub const GENERATOR_DIGITS: &str = "0–9";
pub const GENERATOR_SYMBOLS: &str = "!@#$";
pub const GENERATOR_EXCLUDE_LOOKALIKES: &str = "Exclude look-alikes ({lookalikes})";
pub const GENERATOR_WORDS: &str = "Words";
pub const GENERATOR_SEPARATOR: &str = "Separator";
pub const GENERATOR_AGAIN: &str = "Regenerate";
pub const GENERATOR_USE: &str = "Use password";
pub const GENERATOR_ERROR_NO_CLASSES: &str = "Choose at least one kind of character.";
pub const GENERATOR_ERROR_LENGTH: &str = "The length must be between {min} and {max}.";
pub const GENERATOR_ERROR_WORDS: &str = "The number of words must be between {min} and {max}.";

pub const DETAIL_TOTP: &str = "TOTP";
pub const TOTP_BUTTON: &str = "TOTP…";
pub const TOTP_COPY_HINT: &str = "Click or press Ctrl+T to copy";
pub const TOTP_COPIED: &str = "Copied";
pub const TOTP_SECONDS_LEFT: &str = "{seconds} s";
pub const TOTP_UNREADABLE: &str = "This Entry's TOTP settings can't be read: {reason}";
pub const TOTP_SETUP_TITLE: &str = "Set up TOTP";
pub const TOTP_SECRET: &str = "Secret key or otpauth:// URI";
pub const TOTP_SECRET_PLACEHOLDER: &str = "e.g. JBSWY3DPEHPK3PXP";
pub const TOTP_URI_NOTE: &str =
    "Paste the otpauth:// URI from the site's QR code, or type its secret key and settings.";
pub const TOTP_DIGITS: &str = "Digits";
pub const TOTP_PERIOD: &str = "Period (seconds)";
pub const TOTP_ALGORITHM: &str = "Algorithm";
pub const TOTP_SHA1: &str = "SHA-1";
pub const TOTP_SHA256: &str = "SHA-256";
pub const TOTP_SHA512: &str = "SHA-512";
pub const TOTP_REMOVE: &str = "Remove TOTP";
pub const TOTP_ERROR_MISSING_SECRET: &str = "Enter the secret key or otpauth:// URI.";
pub const TOTP_ERROR_INVALID_SECRET: &str =
    "The secret key isn't valid: use the letters A–Z and digits 2–7.";
pub const TOTP_ERROR_DIGITS: &str = "A code has 1 to 10 digits.";
pub const TOTP_ERROR_PERIOD: &str = "The period is 1 to 86400 seconds.";
pub const TOTP_ERROR_NOT_TOTP: &str = "This isn't an otpauth://totp/ URI.";

pub const LOGIN_TITLE: &str = "Save login?";
pub const LOGIN_PROMPT: &str = "You signed in to {site} as “{username}”. Choose where keefast \
saves this login.";
pub const LOGIN_UPDATE: &str = "Update an existing Entry";
pub const LOGIN_NEW: &str = "Save as a new Entry in";
pub const LOGIN_NEW_BROWSER_GROUP: &str = "{name} (new Group)";
pub const LOGIN_SAVE: &str = "Save";
pub const BROWSER_GROUP_MARK: &str = "Browser group";
pub const USE_AS_BROWSER_GROUP: &str = "Save browser logins here";
pub const ERROR_IN_RECYCLE_BIN: &str = "Logins can't be saved in the Recycle Bin.";
pub const ERROR_NOT_PAIRED: &str = "The browser is not paired with this Vault.";
