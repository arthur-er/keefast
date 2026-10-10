#!/usr/bin/env bash
# Builds the Card Plugin's WebAssembly component, card.wasm, which keefast compiles in
# (crates/plugins/src/bundled.rs) and the mock UI runs through jco. Run it after changing the
# Plugin or the host API (crates/plugins/wit), and commit card.wasm with the change.
#
# Needs rustc with the wasm32-unknown-unknown target and wasm-tools, as in `nix develop`. The
# build is reproducible with the same toolchain: paths are remapped out of the binary, and
# `--check` rebuilds and fails if card.wasm differs from the committed one.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"

cargo_home="${CARGO_HOME:-$HOME/.cargo}"
export RUSTFLAGS="--remap-path-prefix=$here=/card --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$(cd ../.. && pwd)=/keefast"
export CARGO_TARGET_DIR="$here/target"

cargo build --locked --release --target wasm32-unknown-unknown
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
wasm-tools component new target/wasm32-unknown-unknown/release/keefast_card_plugin.wasm -o "$out/card.wasm"
wasm-tools validate --features component-model "$out/card.wasm"

if [[ "${1:-}" == "--check" ]]; then
  if cmp -s "$out/card.wasm" card.wasm; then
    echo "card.wasm is up to date"
  else
    echo "card.wasm differs from a fresh build: run plugins/card/build.sh and commit it" >&2
    exit 1
  fi
else
  cp "$out/card.wasm" card.wasm
  echo "built card.wasm ($(wc -c < card.wasm) bytes)"
fi
