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

**Entry**:
A single stored credential or secret record inside a Vault (title, username, password, URL, notes, custom fields, attachments, history).
_Avoid_: Item, record, login

**Group**:
A folder-like container in a Vault that holds Entries and other Groups.
_Avoid_: Folder, category

**Recycle Bin**:
The Group that deleted Entries and Groups move to, when the Vault has it enabled, so a delete can be undone. Deleting from the Recycle Bin deletes for good.
_Avoid_: Trash, deleted items

**Storage provider**:
Where a Vault file lives: the local filesystem or a remote service such as Dropbox.
_Avoid_: Backend, remote, cloud

**Merge**:
Reconciling a Vault with a newer version of its Vault file that was changed elsewhere, so that no changes from either side are lost.
_Avoid_: Sync (sync is the overall process; merge is the reconciliation step)

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
The Group in each Vault where logins captured by the Companion extension are saved by default.
_Avoid_: Inbox, new logins folder
