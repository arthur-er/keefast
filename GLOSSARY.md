# keefast

A fast, native password manager for KeePass (KDBX) vaults, with a companion browser extension.

## Language

### Vaults

**Vault**:
An opened, decrypted collection of Groups and Entries that the user is working with. Several Vaults can be open at once.
_Avoid_: Database, file, store

**Locked Vault**:
A Vault that is still open but whose decrypted contents have been dropped, until it is unlocked again with its key. Neither the app nor the browser can see into it.
_Avoid_: Closed Vault, sleeping Vault

**Vault file**:
The encrypted KDBX file that a Vault is read from and saved to, wherever it is stored.
_Avoid_: Database file, kdbx

**Key file**:
A file whose contents unlock a Vault file, alone or together with the master password, in any of the formats KeePass clients use.
_Avoid_: Keyfile, key, token

**Master key**:
What unlocks a Vault file: its master password, its Key file, or both together. Changing it re-encrypts the Vault file on the next save.
_Avoid_: Credentials, passphrase

**Entry**:
A single stored credential or secret record inside a Vault (title, username, password, URL, notes, custom fields, attachments, history).
_Avoid_: Item, record, login

**Attachment**:
A file kept with an Entry, such as recovery codes or a key file. Its contents live once in the Vault file's binary pool, shared by every Entry and history version that has it.
_Avoid_: Binary, file (for the stored copy)

**Group**:
A folder-like container in a Vault that holds Entries and other Groups.
_Avoid_: Folder, category

**Recycle Bin**:
The Group that deleted Entries and Groups move to, when the Vault has it enabled, so a delete can be undone. Deleting from the Recycle Bin deletes for good.
_Avoid_: Trash, deleted items

**Smart list**:
A list in the sidebar of a Vault's Entries outside the Recycle Bin, picked by what they have rather than where they are: With TOTP, Recently modified (changed in the last 30 days, newest first) and Expired.
_Avoid_: Filter, saved search, view

**Tag colour**:
The colour a tag is drawn in. The user can choose one per Vault, kept in the Vault file's custom data; a tag without one gets a colour picked from its name.
_Avoid_: Label colour, tag style

**Storage provider**:
Where a Vault file lives: the local filesystem or a remote service such as Dropbox.
_Avoid_: Backend, remote, cloud

**Merge**:
Reconciling a Vault with a newer version of its Vault file that was changed elsewhere, so that no changes from either side are lost.
_Avoid_: Sync (sync is the overall process; merge is the reconciliation step)

**Local cache**:
The copy of a Dropbox Vault file kept on this machine, still encrypted, with the revision it was last synced at. It lets the Vault open offline, and holds saves made offline until they are uploaded.
_Avoid_: Offline copy, mirror

**Sync status**:
Where a Dropbox Vault stands with its Vault file in Dropbox: synced, syncing, offline or error.
_Avoid_: Connection state

### Browser integration

**Companion extension**:
The browser extension. It holds no secrets and gets credentials only from the running desktop app.
_Avoid_: Plugin, add-on

**Pairing**:
The trust relationship between one browser profile's Companion extension and one Vault, created by the user and stored in that Vault.
_Avoid_: Association, connection, link

**Access decision**:
The user's allow or deny answer to a site asking for a specific Entry, remembered on that Entry. It always overrides the Vault's Default access policy.
_Avoid_: Permission, consent, grant

**Default access policy**:
A per-Vault setting (ask, always allow, or always deny) applied when an Entry has no Access decision for the requesting site.
_Avoid_: Global permission, auto-allow

**Browser group**:
The Group in each Vault where sign-ins captured by the Companion extension are saved as Entries by default.
_Avoid_: Inbox, new logins folder

**Field icon**:
keefast's icon in a page's sign-in, 2FA, new password and payment fields, shown by the Companion extension once the user turns it on. Clicking it fills the site's only Entry, or opens the Command palette; in a payment field, it always opens the Command palette.
_Avoid_: Autofill button, inline menu

**Command palette** (browser):
The palette the Companion extension opens over a page to pick an Entry to fill, the site's or any found by searching the paired Vaults, to fill a generated password, or to pick an Entry with a Card to fill a payment form. Not the desktop app's command palette.
_Avoid_: Inline menu, overlay

### Plugins

**Plugin**:
A package of code from keefast or a third party that teaches the desktop app a kind of data kept on Entries, such as payment cards, installed by the user. Not a KeePass2 plugin and not the Companion extension.
_Avoid_: Extension, add-on, module

**Plugin capability**:
Something a Plugin may do, granted by the user when installing it: the fields it owns, revealing protected values, copying, filling in the browser. A Plugin can do nothing it wasn't granted.
_Avoid_: Permission, grant, scope

**Owned field**:
A custom field a Plugin declared as its own. It is still an ordinary custom field in the Vault file, shown as one when the Plugin is missing.
_Avoid_: Plugin field, plugin data

**Card**:
Payment card details kept on an Entry by the Card Plugin, beside anything else the Entry holds: number, cardholder, expiry, CVV and PIN.
_Avoid_: Credit card, payment method
