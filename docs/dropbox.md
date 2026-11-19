# Dropbox

"From Dropbox…" in the Open a Vault dialog connects a Dropbox account and browses the whole
Dropbox for a Vault file. Dropbox asks for permission in your browser (OAuth with PKCE, so keefast
holds no client secret), then sends the browser back to keefast on
`http://localhost:47826/dropbox`. keefast keeps the account's tokens in the OS secret store
(Windows Credential Manager, or the Secret Service on Linux), never in `config.toml`; recent
Vault files remember only the Dropbox path. "Disconnect" revokes the tokens and removes them.

keefast needs a Dropbox app to connect as. To set one up:

1. In the [Dropbox App Console](https://www.dropbox.com/developers/apps), create an app with
   **Scoped access** and **Full Dropbox** access.
2. On its Permissions tab, enable `account_info.read`, `files.metadata.read`,
   `files.content.read` and `files.content.write`.
3. On its Settings tab, add the redirect URI `http://localhost:47826/dropbox`, and allow public
   clients (PKCE). No app secret is needed.
4. Give keefast the app key, either when building (`KEEFAST_DROPBOX_APP_KEY=<key> cargo build`),
   at run time (`KEEFAST_DROPBOX_APP_KEY=<key> keefast`), or in `keefast/config.toml`:
   `dropbox_app_key = "<key>"`. Without one, keefast doesn't offer Dropbox.

To try it without a Dropbox account, run the fake Dropbox the tests use; it lets keefast in as
soon as the browser opens its page, and holds a few Vault files (master password
`fixture-password`):

```sh
cargo run -p keefast-workspace --example fake_dropbox   # prints the environment to start keefast with
```

Type `change` into the fake Dropbox's terminal to have "another device" add a Group to every
Vault file, which keefast finds and Merges in within 30 seconds.

## Sync

- **Saving** fetches the Vault file's revision from Dropbox, Merges in whatever another device
  saved since, and uploads only over that revision. If another device uploads in between,
  Dropbox refuses, and keefast Merges again and retries. Nothing is overwritten.
- **Background checks**: Dropbox doesn't notify desktop apps of changes, so keefast checks each
  open Dropbox Vault every 30 seconds. A check asks only for the revision, and downloads and
  Merges the Vault file only when it changed. While Dropbox can't be reached, checks back off
  (up to every 5 minutes), so an offline save can take that long to upload once the network is
  back; "Sync Dropbox Vaults Now" in the command palette checks at once. Locked Vaults are
  never checked.
- **Offline**: every Dropbox Vault file keefast opens is cached in
  `<local data dir>/keefast/vault-cache/` (`~/.local/share` on Linux, `%LOCALAPPDATA%` on
  Windows), with its revision. The cache holds the Vault file itself, encrypted with the
  Vault's key, never anything decrypted. Without a network a cached Vault still opens, and
  saves go to the cache. They're uploaded, with a Merge, as soon as Dropbox is reachable
  again, even after a restart or a lock.
- **Sync status**: the footer shows each Dropbox Vault as *Synced*, *Syncing…*, *Offline* or
  *Sync error* (for instance, its Vault file was deleted from Dropbox).

The cache is keyed by Dropbox path, not by account. Connecting a different Dropbox account
that has a Vault file at the same path reuses its cache entry. A save made offline would then
be Merged into the other account's file, if the same key opens it.

The test against the real Dropbox is opt-in: it needs a sandbox account and someone to click
"Allow" (see `crates/workspace/tests/dropbox_live.rs`):

```sh
KEEFAST_DROPBOX_TEST_APP_KEY=<key> cargo test -p keefast-workspace --test dropbox_live -- --ignored --nocapture
```
