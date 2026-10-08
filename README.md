<img src="assets/icon.svg" width="64" height="64" alt="">

# keefast

A fast, native password manager for KeePass (KDBX) Vault files, written in Rust with GPUI.

Created in [T3 Code](https://t3.codes).

## Development

The Cargo workspace lives in `crates/`:

- `core`: domain model and KDBX handling. No I/O, no GPUI, no async runtime.
- `workspace`: the headless app model, with Storage providers injected. No GPUI.
- `app`: the GPUI views (the `keefast` binary).
- `native-host`: the native-messaging host Chromium starts for the browser extension
  (`keefast-native-host`). It relays messages to the running app over a local socket.

The Companion extension, a Chromium MV3 extension in TypeScript, lives in `extension/`.

On NixOS (or anywhere with Nix), `nix develop` gives you the Rust toolchain and the system
libraries GPUI needs. Elsewhere, install stable Rust; on Linux also install the libraries listed
in `.github/workflows/ci.yml`.

```sh
cargo test --workspace
cargo run -p keefast -- fixtures/keepassxc-kdbx4.kdbx   # master password: fixture-password
# With a key file (fixtures/key-files/ has one Vault file per key file format):
cargo run -p keefast -- fixtures/key-files/keepassxc-xml-v2.kdbx --key-file fixtures/key-files/xml-v2.keyx
```

The Vault files in `fixtures/` are the round-trip corpus (ADR 0002): real files written by
KeePassXC, KeePass2, KeeWeb and pykeepass. `nix develop .#corpus` adds those clients, to
regenerate the corpus and to check keefast's saves against them:

```sh
nix develop .#corpus -c scripts/make-fixtures.sh
nix develop .#corpus -c scripts/check-interop.sh
```

## Keyboard

Ctrl+Shift+P opens the command palette: type part of a command's name, pick one with Up/Down and
press Enter to run it, or Escape to close the palette. It lists every action of what has focus,
with its keys.

In the Entry list, Up/Down (Home/End) select an Entry and Enter opens it for editing.
Ctrl+B copies the selected Entry's username, Ctrl+C its password and Ctrl+T its TOTP code.
keefast clears what it copied from the clipboard after 30 seconds, unless you have copied something else since;
change the wait in App settings.

## App settings

App settings (Ctrl+, or "App settings" in the sidebar and the unlock dialog) hold the preferences
kept on this machine, in `keefast/config.toml` in the OS config directory: the theme (the
system's light/dark setting, or always light or dark), the idle lock and locking with the OS, how
long copied secrets stay on the clipboard, autosave, the password generator, browser integration
and favicon download. Tab moves between the settings and Enter or Space picks one; Escape closes
them.

Favicon download is off by default, as keefast makes no network requests you haven't agreed to.
Turned on, keefast asks each Entry's site for `/favicon.ico`, or the icon its home page links
to, and never a third-party favicon service. Favicons are kept in memory only, and forgotten when
every Vault locks: a cache on disk would list your sites in the clear.

## Tags, colours, icons and expiry

Edit an Entry to give it tags (separated by commas), a colour, an icon and an expiry date. The
sidebar lists every tag and colour; click one to list the Entries with it across the open
Vaults. Expired Entries are struck through and flagged in the Entry list. Icons are KeePass's
standard icons, drawn with [Lucide](https://lucide.dev) pictures (ISC, see
`crates/app/assets/icons/standard/LICENSE`; `scripts/make-standard-icons.mjs` regenerates
them), or PNG images imported into the Vault as custom icons. All of these are stored as
KeePassXC, KeePass and KeeWeb store them, so those clients show them too.

## Browser integration

keefast speaks the KeePassXC-Browser protocol (ADR 0003) with its Companion extension, and the
stock KeePassXC-Browser extension works with it too. Turn on browser integration in keefast's App settings:
it registers `keefast-native-host`, installed next to the `keefast` binary, with Chrome,
Chromium, Brave, Edge and Vivaldi. It registers under KeePassXC's host name, so it takes over
from a KeePassXC registered for the same browsers; turning it off removes only keefast's
registrations. Then click "Connect" in the extension and name the browser in keefast to pair it
with the Vault you are looking at.

To try the Companion extension, build it and load `extension/dist` as an unpacked extension in
`chrome://extensions` (with "Developer mode" on):

```sh
cd extension
npm ci
npm test
npm run build
```

Click its toolbar button, then "Connect", and name the browser in keefast. On a login page, the
popup lists the site's Entries: click one to fill it, or press Alt+Shift+K to fill the only one.
Nothing is filled until you ask. The extension never asks for a master password: when keefast
is locked or not running, it says so, and you unlock keefast itself.

Once you've opened the popup on a page, the extension also notices the logins you submit there:
a sign-in it doesn't have, a sign-up, or a password change. keefast asks you whether to save
it, and where; a changed password updates its Entry. A sign-in with an Entry as filled sends
nothing. Pages you haven't opened the popup on are never read.

To try the protocol without a browser, run keefast with browser integration on and:

```sh
cargo run -p keefast-native-host --example fake_extension -- target/debug/keefast-native-host
```

## Dropbox

"From Dropbox…" in the Open a Vault dialog connects a Dropbox account and browses the whole
Dropbox for a Vault file. Dropbox asks for permission in your browser (OAuth with PKCE, so keefast
holds no client secret), then sends the browser back to keefast on
`http://localhost:47826/dropbox`. keefast keeps the account's tokens in the OS secret store
(Windows Credential Manager, or the Secret Service on Linux), never in `config.toml`; recent
Vault files remember only the Dropbox path. "Disconnect" revokes the tokens and removes them.
Saving a Dropbox Vault replaces only the revision keefast read, so a change made on another
device is never overwritten.

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

The test against the real Dropbox is opt-in: it needs a sandbox account and someone to click
"Allow" (see `crates/workspace/tests/dropbox_live.rs`):

```sh
KEEFAST_DROPBOX_TEST_APP_KEY=<key> cargo test -p keefast-workspace --test dropbox_live -- --ignored --nocapture
```

## Credits

Passphrases are made from the [EFF large word list](https://www.eff.org/dice) by the Electronic
Frontier Foundation, licensed under [CC BY 3.0 US](https://creativecommons.org/licenses/by/3.0/us/).
