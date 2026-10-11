#!/usr/bin/env bash
# Builds the Card Plugin's WebAssembly component, card.wasm, with the same command third-party
# Plugin authors use (`keefast-plugin build`, docs/plugins/README.md). keefast's build signs it
# into a package and compiles that in (crates/plugins/build.rs); the mock UI runs it through jco.
# Run it after changing the Plugin, the SDK or the host API (crates/plugins/wit), and commit
# card.wasm with the change.
#
# Needs rustc with the wasm32-unknown-unknown target, as in `nix develop`. The build is
# reproducible with the same toolchain: `keefast-plugin build` remaps paths out of the binary, and
# `--check` rebuilds and fails if card.wasm differs from the committed one.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
cargo run --quiet --locked --manifest-path "$root/Cargo.toml" -p keefast-plugin-cli -- \
  build "$here" -o "$out/card.wasm"

if [[ "${1:-}" == "--check" ]]; then
  if cmp -s "$out/card.wasm" "$here/card.wasm"; then
    echo "card.wasm is up to date"
  else
    echo "card.wasm differs from a fresh build: run plugins/card/build.sh and commit it" >&2
    exit 1
  fi
else
  cp "$out/card.wasm" "$here/card.wasm"
  echo "built card.wasm ($(wc -c < "$here/card.wasm") bytes)"
fi
