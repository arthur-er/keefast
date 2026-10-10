# The Card Plugin

keefast's own Plugin for payment cards (ADR 0009), written against the same host API as
third-party Plugins (`crates/plugins/wit/plugin.wit`). A Card is kept on an Entry in plain custom
fields, so any KeePass app reads it: `Card number` (protected), `Cardholder`, `Expiry` (`MM/YY`),
`CVV` (protected) and `PIN` (protected).

- `plugin.json`: the manifest. The facet `card` is detected by a `Card number` field.
- `src/card.rs`: brands, number and expiry formatting, Luhn, and what `validate` reports.
- `src/lib.rs`: the hooks. `derive` labels the number "Visa •••• 1111"; it has no clock, so the
  host says whether the `Expiry` (a `monthYear` field) has passed.
- `card.wasm`: the built component, committed. keefast compiles it in
  (`crates/plugins/src/lib.rs`), and the mock UI runs it through jco
  (`crates/app/ui/bundled-plugins.ts`).

keefast lists it in Settings → Plugins as keefast's: it can be disabled, not removed. It isn't a
signed package yet: compiled in, it is trusted as part of keefast. Packaging and signing it with
keefast's key, so it goes through the same checks as third-party Plugins
(`crates/plugins/PACKAGE.md`), is #123.

## Building

`card.wasm` is committed rather than built by `cargo build`: building it needs the
`wasm32-unknown-unknown` target and `wasm-tools`, which keefast's own build (CI, the Nix package,
Windows) shouldn't have to install, and a nested Cargo build in `build.rs` would fight the outer
one for locks. Rebuild it after changing the Plugin or the host API, and commit it with the change:

```sh
nix develop -c plugins/card/build.sh          # rebuilds card.wasm
nix develop -c plugins/card/build.sh --check  # fails if card.wasm isn't what the source builds
cd plugins/card && cargo test                  # the card logic, natively
```

The build is reproducible with the same toolchain (the dev shell's rustc, lld and wasm-tools):
paths are remapped out of the binary, and `--check` compares a fresh build with the committed file
byte for byte.
