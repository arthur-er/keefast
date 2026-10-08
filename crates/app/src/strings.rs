//! Every user-facing string, in one place so the UI is ready for translation.

pub const APP_NAME: &str = "keefast";

pub const UNLOCK_TITLE: &str = "Open a Vault";
pub const UNLOCK_NO_FILE: &str = "No Vault file selected";
pub const UNLOCK_CHOOSE_FILE: &str = "Choose Vault file…";
pub const UNLOCK_CHOOSE_FILE_PROMPT: &str = "Open Vault file";
pub const UNLOCK_PASSWORD_LABEL: &str = "Master password";
pub const UNLOCK_PASSWORD_PLACEHOLDER: &str = "Type your master password";
pub const UNLOCK_KEY_FILE_LABEL: &str = "Key file";
pub const UNLOCK_NO_KEY_FILE: &str = "None (optional)";
pub const UNLOCK_CHOOSE_KEY_FILE: &str = "Choose key file…";
pub const UNLOCK_CHOOSE_KEY_FILE_PROMPT: &str = "Choose key file";
pub const UNLOCK_CLEAR_KEY_FILE: &str = "×";
pub const UNLOCK_BUTTON: &str = "Unlock";
pub const UNLOCK_IN_PROGRESS: &str = "Unlocking…";
pub const UNLOCK_PROGRESS: &str = "Deriving the key. This can take a few seconds.";
pub const UNLOCK_ERROR_WRONG_KEY: &str = "Wrong master password or key file.";
pub const UNLOCK_ERROR_CORRUPT: &str = "This Vault file is damaged or is not a Vault file.";
pub const UNLOCK_ERROR_UNSUPPORTED: &str = "This Vault file format is not supported.";
pub const UNLOCK_ERROR_NOT_FOUND: &str = "The Vault file could not be found.";
pub const UNLOCK_ERROR_READ: &str = "The Vault file could not be read.";
pub const UNLOCK_ERROR_KEY_FILE_NOT_FOUND: &str = "The key file could not be found.";
pub const UNLOCK_ERROR_KEY_FILE_READ: &str = "The key file could not be read.";
pub const UNLOCK_ERROR_KEY_FILE_CORRUPT: &str = "This key file is damaged.";
pub const UNLOCK_ERROR_KEY_FILE_UNSUPPORTED: &str = "This key file format is not supported.";
pub const UNLOCK_ERROR_PICKER: &str = "The file picker could not be opened.";
pub const UNLOCK_ERROR_ALREADY_OPEN: &str = "A Vault from this Vault file is already open.";
pub const UNLOCK_FROM_DROPBOX: &str = "From Dropbox…";
pub const UNLOCK_ERROR_NO_PROVIDER: &str =
    "This Vault file is in Dropbox, which this build of keefast can't reach.";
pub const RECENT_VAULTS: &str = "Recent Vault files";
pub const LOCKED_VAULTS: &str = "Locked Vaults";
pub const RECENT_FORGET: &str = "×";
pub const CONFIG_ERROR: &str =
    "keefast's settings file could not be read or saved, so recent Vault files may be forgotten.";

pub const SIDEBAR_VAULTS: &str = "Vaults";
pub const SIDEBAR_ALL_VAULTS: &str = "All Vaults";
pub const OPEN_VAULT: &str = "+ Vault";
pub const LOCK: &str = "Lock";
pub const LOCK_ALL: &str = "Lock all";
pub const LOCKED_MARKER: &str = " (locked)";
pub const LOCKING_MARKER: &str = " (locking…)";
pub const LOCKING_NOTE: &str = "This Vault is about to lock. Save or discard its changes to continue; nothing in it is shown until then.";
pub const VAULT_LOCKED: &str = "This Vault is locked.";
pub const UNLOCK_VAULT: &str = "Unlock…";
pub const LOCK_UNSAVED: &str = "“{vault}” has unsaved changes. Save them before locking?";
pub const LOCK_UNSAVED_DETAIL: &str = "Discarding loses them.";
pub const LOCK_UNSAVED_AUTO_DETAIL: &str = "keefast is locking, and the Vault stays hidden until you choose. Discarding loses the changes.";
pub const LOCK_SAVE: &str = "Save";
pub const LOCK_DISCARD: &str = "Discard";

pub const LIST_EMPTY: &str = "No Entries";
pub const LIST_NO_MATCHES: &str = "No Entries match the search";
pub const SEARCH_PLACEHOLDER: &str = "Search";
pub const SORT_PREFIX: &str = "Sort: ";
pub const SORT_TITLE: &str = "Title";
pub const SORT_USERNAME: &str = "Username";
pub const SORT_URL: &str = "URL";
pub const SORT_CREATED: &str = "Created";
pub const SORT_MODIFIED: &str = "Modified";
pub const DETAIL_EMPTY: &str = "Select an Entry";
pub const DETAIL_USERNAME: &str = "Username";
pub const DETAIL_PASSWORD: &str = "Password";
pub const DETAIL_URL: &str = "URL";
pub const DETAIL_NOTES: &str = "Notes";
pub const UNTITLED: &str = "(untitled)";
pub const COPIED_USERNAME: &str = "Username copied.";
pub const COPIED_PASSWORD: &str = "Password copied.";
pub const COPIED_TOTP: &str = "TOTP code copied.";
pub const COPIED_FIELD: &str = "Field copied.";
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
pub const CUSTOM_FIELDS: &str = "Custom fields";
pub const ADD_FIELD: &str = "+ Field";
pub const FIELD_NAME_PLACEHOLDER: &str = "Name";
pub const FIELD_VALUE_PLACEHOLDER: &str = "Value";
pub const FIELD_PROTECTED: &str = "Protected";
pub const REMOVE_FIELD: &str = "Remove";
pub const COPY: &str = "Copy";
pub const COPIED: &str = "Copied";

pub const FIELD_TAGS: &str = "Tags";
pub const TAGS_PLACEHOLDER: &str = "Separate tags with commas";
pub const FIELD_COLOR: &str = "Colour";
pub const NO_COLOR: &str = "None";
pub const FIELD_ICON: &str = "Icon";
pub const CHOOSE_ICON: &str = "Change…";
pub const STANDARD_ICONS: &str = "Standard icons";
pub const CUSTOM_ICONS: &str = "This Vault's icons";
pub const IMPORT_ICON: &str = "Import PNG…";
pub const IMPORT_ICON_PROMPT: &str = "Choose an icon image (PNG)";
pub const FIELD_EXPIRES: &str = "Expires";
pub const EXPIRY_PLACEHOLDER: &str = "YYYY-MM-DD";
pub const EXPIRY_NOTE: &str = "At the start of that day, UTC.";
pub const DETAIL_TAGS: &str = "Tags";
pub const DETAIL_EXPIRES: &str = "Expires";
pub const EXPIRED_BADGE: &str = "Expired";
pub const ERROR_EXPIRY_DATE: &str = "Type the expiry date as YYYY-MM-DD, such as 2027-01-31.";
pub const ERROR_ICON_NOT_FOUND: &str = "That icon is no longer in this Vault.";
pub const ERROR_ICON_NOT_PNG: &str = "An icon must be a PNG image.";
pub const ERROR_ICON_TOO_LARGE: &str = "That image is too large for an icon.";
pub const ERROR_ICON_UNREADABLE: &str = "The image could not be read.";
pub const ERROR_PICKER: &str = "The file picker could not be opened.";

pub const SIDEBAR_TAGS: &str = "Tags";
pub const SIDEBAR_COLORS: &str = "Colours";
pub const TAG_HEADER: &str = "Tag: {tag}";
pub const COLOR_HEADER: &str = "Colour: {color}";

/// Each colour's name, as [`keefast_core::EntryColor`] lists them.
pub fn color_name(color: keefast_core::EntryColor) -> &'static str {
    use keefast_core::EntryColor;
    match color {
        EntryColor::Red => "Red",
        EntryColor::Orange => "Orange",
        EntryColor::Yellow => "Yellow",
        EntryColor::Green => "Green",
        EntryColor::Blue => "Blue",
        EntryColor::Violet => "Violet",
    }
}

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

/// A Merge of changes made to a Vault file elsewhere, by another device or a sync tool.
pub const MERGE_NOTICE: &str = "Merged changes made elsewhere into {vault}";
pub const MERGE_NOTICE_DISMISS: &str = "×";
/// What a Merge changed, as `{n}` items: singular, then plural.
pub const MERGE_ENTRIES_ADDED: [&str; 2] = ["{n} Entry added", "{n} Entries added"];
pub const MERGE_ENTRIES_UPDATED: [&str; 2] = ["{n} Entry updated", "{n} Entries updated"];
pub const MERGE_ENTRIES_MOVED: [&str; 2] = ["{n} Entry moved", "{n} Entries moved"];
pub const MERGE_ENTRIES_DELETED: [&str; 2] = ["{n} Entry deleted", "{n} Entries deleted"];
pub const MERGE_GROUPS_ADDED: [&str; 2] = ["{n} Group added", "{n} Groups added"];
pub const MERGE_GROUPS_UPDATED: [&str; 2] = [
    "{n} Group renamed or changed",
    "{n} Groups renamed or changed",
];
pub const MERGE_GROUPS_MOVED: [&str; 2] = ["{n} Group moved", "{n} Groups moved"];
pub const MERGE_GROUPS_DELETED: [&str; 2] = ["{n} Group deleted", "{n} Groups deleted"];
/// When a Merge changed only what the counts leave out, such as Entry history.
pub const MERGE_OTHER_CHANGES: &str = "history and settings updated";

pub const CONFIRM_DELETE_ENTRY: &str = "Delete this Entry for good?";
pub const CONFIRM_DELETE_GROUP: &str = "Delete this Group and everything in it for good?";
pub const CONFIRM_DELETE_DETAIL: &str = "This cannot be undone.";

pub const ERROR_MOVE_INTO_ITSELF: &str = "A Group cannot be moved into itself or its subgroups.";
pub const ERROR_ROOT_GROUP: &str = "The top Group of a Vault cannot be moved or deleted.";
pub const ERROR_NOT_FOUND: &str = "That Entry or Group no longer exists.";
pub const ERROR_FIELD_NAME_EMPTY: &str = "Give every custom field a name.";
pub const ERROR_FIELD_NAME_TAKEN: &str =
    "Each field needs a name of its own, other than Title, UserName, Password, URL or Notes.";

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
pub const ERROR_PAIRING_NOT_FOUND: &str = "This browser is no longer paired with the Vault.";

pub const ACCESS_TITLE: &str = "Allow access?";
pub const ACCESS_PROMPT: &str = "{site} asks for these Entries. Allow only if you are signing in \
there.";
pub const ACCESS_REMEMBER: &str = "Remember this decision for {site}";
pub const ACCESS_ALLOW: &str = "Allow";
pub const ACCESS_DENY: &str = "Deny";
pub const DETAIL_BROWSER_ACCESS: &str = "Browser access";
pub const ACCESS_ALLOWED: &str = "Allowed";
pub const ACCESS_DENIED: &str = "Denied";
pub const ACCESS_FORGET: &str = "Forget";
pub const ACCESS_FORGET_ALL: &str = "Forget all";

pub const VAULT_SETTINGS: &str = "Settings";
pub const VAULT_SETTINGS_TITLE: &str = "Settings for {vault}";
pub const DEFAULT_ACCESS_POLICY: &str = "Default access policy";
pub const DEFAULT_ACCESS_POLICY_NOTE: &str = "When a site asks for an Entry you haven't decided \
on for it. A decision remembered on the Entry always comes first.";
pub const POLICY_ASK: &str = "Ask me";
pub const POLICY_ALLOW: &str = "Always allow";
pub const POLICY_DENY: &str = "Always deny";
pub const RADIO_ON: &str = "◉";
pub const RADIO_OFF: &str = "○";
pub const PAIRINGS: &str = "Paired browsers";
pub const PAIRINGS_NOTE: &str = "These browsers can ask this Vault for logins while it is open.";
pub const PAIRINGS_NONE: &str = "No browser is paired with this Vault.";
pub const REVOKE_PAIRING: &str = "Revoke";
pub const CONFIRM_REVOKE_PAIRING: &str = "Revoke the Pairing “{name}”?";
pub const CONFIRM_REVOKE_PAIRING_DETAIL: &str = "It must pair again to fill logins.";

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

pub const DROPBOX_LOCATION: &str = "Dropbox: {path}";
pub const DROPBOX_TITLE: &str = "Open from Dropbox";
pub const DROPBOX_CHECKING: &str = "Looking for your Dropbox account…";
pub const DROPBOX_CONNECT_PROMPT: &str = "Connect your Dropbox account to open Vault files stored \
there. Dropbox asks you in your browser; keefast never sees your Dropbox password.";
pub const DROPBOX_CONNECT: &str = "Connect Dropbox";
pub const DROPBOX_CONNECTING: &str =
    "Allow keefast in the Dropbox page that opened in your browser, then come back here.";
pub const DROPBOX_OPEN_PAGE_AGAIN: &str = "Open the page again";
pub const DROPBOX_STOP_CONNECTING: &str = "Stop";
pub const DROPBOX_CONNECTED_AS: &str = "Connected as {account}";
pub const DROPBOX_DISCONNECT: &str = "Disconnect";
pub const DROPBOX_TOP: &str = "Dropbox";
pub const DROPBOX_UP: &str = "↑ Up";
pub const DROPBOX_FOLDER_ICON: &str = "▸";
pub const DROPBOX_FILE_ICON: &str = "·";
pub const DROPBOX_LOADING: &str = "Loading…";
pub const DROPBOX_EMPTY_FOLDER: &str = "This folder is empty.";
pub const DROPBOX_ERROR_NOT_CONFIGURED: &str = "This build of keefast has no Dropbox app key.";
pub const DROPBOX_ERROR_NOT_CONNECTED: &str =
    "Dropbox no longer lets keefast in. Connect your account again.";
pub const DROPBOX_ERROR_REFUSED: &str = "Dropbox was not connected.";
pub const DROPBOX_ERROR_CALLBACK: &str =
    "keefast could not wait for Dropbox's answer. Is another keefast connecting Dropbox?";
pub const DROPBOX_ERROR_NETWORK: &str = "Dropbox can't be reached. Check your connection.";
pub const DROPBOX_ERROR_NOT_FOUND: &str = "That is no longer in Dropbox.";
pub const DROPBOX_ERROR_CONFLICT: &str = "The Vault file changed in Dropbox since keefast read it.";
pub const DROPBOX_ERROR_API: &str = "Dropbox reported an error.";
pub const DROPBOX_ERROR_SECRET_STORE: &str =
    "The OS secret store isn't available, so keefast can't keep your Dropbox account connected.";

pub const HISTORY_BUTTON: &str = "History";
pub const HISTORY_TITLE: &str = "History of {title}";
pub const HISTORY_CLOSE: &str = "Close";
pub const HISTORY_CURRENT: &str = "Current version";
pub const HISTORY_EMPTY: &str = "This Entry has no earlier versions yet. Each edit keeps the \
version it replaces here.";
pub const HISTORY_VERSION_OF: &str = "Version of {time}";
pub const HISTORY_RESTORE: &str = "Restore this version";
pub const HISTORY_RESTORED: &str =
    "Version restored. The one it replaced is kept in the Entry's history.";
pub const HISTORY_UNKNOWN_TIME: &str = "Unknown time";
pub const HISTORY_TOTP_NONE: &str = "Not set up";
pub const HISTORY_TOTP_SET: &str = "Set up";
pub const HISTORY_TOTP_UNREADABLE: &str = "Set up, but its settings can't be read";
pub const ERROR_VERSION_NOT_FOUND: &str = "That version is no longer in the Entry's history.";

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

/// The app-wide settings, kept on this machine, as opposed to a Vault's own settings.
pub const SETTINGS: &str = "App settings";
pub const SETTINGS_CLOSE: &str = "Done";
pub const SETTINGS_APPEARANCE: &str = "Appearance";
pub const SETTINGS_THEME: &str = "Theme";
pub const THEME_SYSTEM: &str = "Same as the system";
pub const THEME_LIGHT: &str = "Light";
pub const THEME_DARK: &str = "Dark";
pub const SETTINGS_LOCKING: &str = "Locking";
pub const SETTINGS_IDLE_LOCK: &str = "Lock every Vault after this long without input";
pub const SETTINGS_LOCK_ON_OS_LOCK: &str =
    "Lock every Vault when the screen locks or the computer sleeps";
pub const SETTINGS_CLIPBOARD: &str = "Clipboard";
pub const SETTINGS_CLIPBOARD_CLEAR: &str = "Clear copied usernames and passwords after";
pub const SETTINGS_SAVING: &str = "Saving";
pub const SETTINGS_AUTOSAVE: &str = "Save Vaults automatically after every change";
pub const SETTINGS_GENERATOR: &str = "Password generator";
pub const SETTINGS_GENERATOR_NOTE: &str =
    "keefast and the Companion extension generate passwords with these settings.";
pub const SETTINGS_BROWSER: &str = "Browser";
pub const SETTINGS_BROWSER_INTEGRATION: &str =
    "Let browser extensions ask keefast for Entries to fill (KeePassXC-Browser protocol)";
pub const SETTINGS_NETWORK: &str = "Network";
pub const SETTINGS_FAVICONS: &str = "Download website icons for Entries";
pub const SETTINGS_FAVICONS_NOTE: &str = "keefast then asks each Entry's website for its icon, \
so those websites see your IP address. Nothing goes through anyone else, and the icons are kept \
in memory only, never on disk.";
pub const NEVER: &str = "Never";
pub const DURATION_SECONDS: &str = "{n} s";
pub const DURATION_MINUTES: &str = "{n} min";
pub const DURATION_HOURS: &str = "{n} h";

/// The names of commands in the command palette, by action. The palette names an action not
/// listed here after the action itself.
pub fn command_name(action: &str) -> Option<&'static str> {
    Some(match action {
        "vault_browser::SelectNextEntry" => "Select Next Entry",
        "vault_browser::SelectPreviousEntry" => "Select Previous Entry",
        "vault_browser::SelectFirstEntry" => "Select First Entry",
        "vault_browser::SelectLastEntry" => "Select Last Entry",
        "vault_browser::OpenEntry" => "Edit Entry",
        "vault_browser::CopyUsername" => "Copy Username",
        "vault_browser::CopyPassword" => "Copy Password",
        "vault_browser::CopyTotp" => "Copy TOTP Code",
        "vault_browser::CopyCustomField" => "Copy Custom Field",
        "vault_browser::LockAll" => "Lock All Vaults",
        "vault_browser::FocusSearch" => "Search Entries",
        "vault_browser::SortByTitle" => "Sort Entries by Title",
        "vault_browser::SortByUsername" => "Sort Entries by Username",
        "vault_browser::SortByUrl" => "Sort Entries by URL",
        "vault_browser::SortByCreated" => "Sort Entries by Creation Time",
        "vault_browser::SortByModified" => "Sort Entries by Last Change",
        "vault_browser::ShowEntryHistory" => "Show Entry History",
        "entry_history::SelectOlderVersion" => "Select Older Version",
        "entry_history::SelectNewerVersion" => "Select Newer Version",
        "entry_history::RestoreVersion" => "Restore This Version",
        "entry_history::CloseHistory" => "Close Entry History",
        "vault_browser::SaveAll" => "Save All Vaults",
        "vault_browser::OpenVault" => "Open Another Vault…",
        "vault_browser::OpenVaultSettings" => "Vault Settings",
        "settings::OpenSettings" => "Open App Settings",
        "settings::CloseSettings" => "Close App Settings",
        _ => return None,
    })
}
pub const PALETTE_PLACEHOLDER: &str = "Type a command";
pub const PALETTE_NO_MATCHES: &str = "No matching commands";
pub const KEY_CTRL: &str = "Ctrl";
pub const KEY_ALT: &str = "Alt";
pub const KEY_SUPER: &str = "Super";
pub const KEY_SHIFT: &str = "Shift";
