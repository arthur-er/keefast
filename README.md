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
keefast clears what it copied from the clipboard after 30 seconds, unless you have copied something else since; set
`clipboard_clear_seconds` in `keefast/config.toml` to change the wait (0 never clears).

## Browser integration

keefast speaks the KeePassXC-Browser protocol (ADR 0003) with its Companion extension, and the
stock KeePassXC-Browser extension works with it too. Turn on "Browser integration" in keefast:
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

## Credits

Passphrases are made from the [EFF large word list](https://www.eff.org/dice) by the Electronic
Frontier Foundation, licensed under [CC BY 3.0 US](https://creativecommons.org/licenses/by/3.0/us/).
