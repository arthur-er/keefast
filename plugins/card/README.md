# The Card Plugin

keefast's own Plugin for payment cards (ADR 0009), written against the same host API as
third-party Plugins (`crates/plugins/wit/plugin.wit`). A Card is kept on an Entry in plain custom
fields, so any KeePass app reads it: `Card number` (protected), `Cardholder`, `Expiry` (`MM/YY`),
`CVV` (protected) and `PIN` (protected).

- `plugin.json`: the manifest. The facet `card` is detected by a `Card number` field.
- `src/card.rs`: brands, number and expiry formatting, Luhn, and what `validate` reports.
- `src/lib.rs`: the hooks, written with keefast's Plugin SDK (`crates/plugin-sdk`), as a
  third-party Plugin is. `derive` labels the number "Visa •••• 1111"; `format` groups the number
  and adds the slash in `MM/YY`; `validate` warns about a number failing the Luhn check and an
  expiry that has passed, expires soon or isn't `MM/YY`. It has no clock: the host passes
  `validate` the month it is.
- `plugin.json`'s `editor` gives the Card its section in the Entry editor, with "Add card" and
  "Remove card".
- `card.wasm`: the built component, committed. keefast's build signs it into a package and
  compiles that in (`crates/plugins/build.rs`), and keefast reads and checks that package at every
  start like any installed one. The mock UI runs `card.wasm` through jco
  (`crates/app/ui/bundled-plugins.ts`).

keefast lists it in Settings → Plugins as keefast's: it can be disabled, not removed. Official
releases sign it with keefast's release key; other builds with a key they make for themselves
([docs/plugins/signing.md](../../docs/plugins/signing.md)).

## Building

`card.wasm` is committed rather than built by `cargo build`: building it needs the
`wasm32-unknown-unknown` target, which keefast's own build (CI, the Nix package, Windows) shouldn't
have to install, and a nested Cargo build in `build.rs` would fight the outer one for locks.
Rebuild it after changing the Plugin, the SDK or the host API, and commit it with the change.
`build.sh` runs `keefast-plugin build`, the command third-party authors use:

```sh
nix develop .#plugins -c plugins/card/build.sh          # rebuilds card.wasm
nix develop .#plugins -c plugins/card/build.sh --check  # fails if card.wasm isn't what the source builds
cd plugins/card && cargo test                            # the card logic and hooks, natively
```

The build is reproducible with the same toolchain (the dev shell's rustc and lld): paths are
remapped out of the binary, and `--check` compares a fresh build with the committed file byte for
byte. CI runs it (the `Plugins` job).
