# Contributing to keefast

The Cargo workspace lives in `crates/`:

- `core`: domain model and KDBX handling. No I/O, no UI, no async runtime.
- `workspace`: the headless app model, with Storage providers injected. No UI.
- `app`: the desktop app (the `keefast` binary): a Tauri 2 window whose web UI, in
  `crates/app/ui/` (React, TypeScript, Tailwind), talks to the Workspace through typed commands
  (ADR 0008). See [`crates/app/ui/README.md`](crates/app/ui/README.md) for working on it.
- `agent`: Agents (ADR 0012), on top of `workspace`: the Agent settings kept in the Vault, and
  the `keefast-agent` command with its Headless host. No Tauri, no WebKit. See
  [Running a Headless host](docs/headless-host.md),
  [Agents on this computer](docs/agents-on-this-computer.md) and
  [Connecting agentes-chat](docs/agentes-chat.md).
- `native-host`: the native-messaging host Chromium starts for the browser extension
  (`keefast-native-host`). It relays messages to the running app over a local socket.
- `plugins`: the Plugin host (`keefast-plugins`), which runs Plugins in wasmtime and installs
  and checks their packages. Beside it, for Plugin authors: `plugin-cli` (the `keefast-plugin`
  command) and `plugin-package` (the `.kfplugin` format).

Plugins teach keefast kinds of data kept on Entries, such as payment cards, and third parties can
write them. **[Writing a keefast Plugin](docs/plugins/README.md)** is the guide; keefast's own Card
Plugin is in `plugins/card/`, and a template to start from in `plugins/template/`. The SDK Plugins
are written with, `keefast-plugin-sdk`, is in `plugins/sdk/`: `plugins/` is a Cargo workspace of
its own, which builds for WebAssembly.

The Companion extension, a Chromium MV3 extension in TypeScript, lives in `extension/`.

On NixOS (or anywhere with Nix), `nix develop` gives you the Rust toolchain, Node, pnpm,
`cargo tauri` and the system libraries Tauri needs (WebKitGTK). Elsewhere, install stable Rust,
Node 24 and pnpm; on Linux also install the libraries listed in `.github/workflows/ci.yml`.

```sh
nix develop            # or prefix each command: nix develop -c cargo test -p keefast-core
cargo test -p keefast-core   # one crate at a time; --workspace builds the Tauri app too
pnpm --dir crates/app/ui install
# The app, with the web UI reloading as you edit (runs `pnpm dev` itself):
cd crates/app && cargo tauri dev -- -- ../../fixtures/keepassxc-kdbx4.kdbx   # master password: fixture-password
# With a key file (fixtures/key-files/ has one Vault file per key file format):
cd crates/app && cargo tauri dev -- -- ../../fixtures/key-files/keepassxc-xml-v2.kdbx --key-file ../../fixtures/key-files/xml-v2.keyx
# The web UI alone, in any browser, over a mock backend with fake Vaults:
pnpm --dir crates/app/ui dev:mock
```

`scripts/check.sh` runs what CI runs (formatting, clippy, the tests, the UI's checks) before you
push, and `scripts/setup-worktree.sh` readies a new checkout or worktree, pre-commit hook
included. More on building and testing: [docs/agents/build-test.md](docs/agents/build-test.md).

The Vault files in `fixtures/` are the round-trip corpus (ADR 0002): real files written by
KeePassXC, KeePass2, KeeWeb and pykeepass. `nix develop .#corpus` adds those clients, to
regenerate the corpus and to check keefast's saves against them:

```sh
nix develop .#corpus -c scripts/make-fixtures.sh
nix develop .#corpus -c scripts/check-interop.sh
```

To work on the Companion extension or try the KeePassXC-Browser protocol without a browser, see
[Browser integration](docs/browser-integration.md); to try Dropbox without an account, see
[Dropbox](docs/dropbox.md). Architecture decisions are in [docs/adr](docs/adr), and the words
keefast uses for things in [GLOSSARY.md](GLOSSARY.md).
