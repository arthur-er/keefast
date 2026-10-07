<img src="assets/icon.svg" width="64" height="64" alt="">

# keefast

A fast, native password manager for KeePass (KDBX) Vault files, written in Rust with GPUI.

Created in [T3 Code](https://t3.codes).

## Development

The Cargo workspace lives in `crates/`:

- `core`: domain model and KDBX handling. No I/O, no GPUI, no async runtime.
- `workspace`: the headless app model, with Storage providers injected. No GPUI.
- `app`: the GPUI views (the `keefast` binary).

On NixOS (or anywhere with Nix), `nix develop` gives you the Rust toolchain and the system
libraries GPUI needs. Elsewhere, install stable Rust; on Linux also install the libraries listed
in `.github/workflows/ci.yml`.

```sh
cargo test --workspace
cargo run -p keefast -- fixtures/keepassxc-kdbx4.kdbx   # master password: fixture-password
```

The Vault files in `fixtures/` are the round-trip corpus (ADR 0002): real files written by
KeePassXC, KeePass2, KeeWeb and pykeepass. `nix develop .#corpus` adds those clients, to
regenerate the corpus and to check keefast's saves against them:

```sh
nix develop .#corpus -c scripts/make-fixtures.sh
nix develop .#corpus -c scripts/check-interop.sh
```
