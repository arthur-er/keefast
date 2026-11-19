#!/usr/bin/env bash
# Runs what CI (.github/workflows/ci.yml) runs, here, before you push. By default: rustfmt,
# clippy with warnings as errors, the workspace's tests (bindings_are_up_to_date among them), the
# desktop UI's typecheck, tests and builds, and, when this branch changes them, the flake's
# evaluation, the Companion extension and the Plugins.
#
#   --crate <name>   check only this crate (repeatable), e.g. --crate keefast-agent: clippy and
#                    tests are scoped to it, and the bindings check runs on its own
#   --no-clippy      skip clippy
#   --no-tests       skip the Rust tests (the bindings check still runs)
#   --no-ui          skip the desktop UI
#   --no-ui-build    skip the UI's two builds (typecheck and tests still run)
#   --no-nix         skip `nix flake check`, even when Nix files changed
#   --base <ref>     what "changed" is measured from (default: the merge base with origin/main)
#
# Not covered: CI's Windows run, and building the Nix package (`nix build .#keefast`).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

args=("$@")
crates=()
clippy=1 tests=1 ui=1 ui_build=1 nix_check=1 base=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --crate) crates+=("$2"); shift ;;
    --no-clippy) clippy=0 ;;
    --no-tests) tests=0 ;;
    --no-ui) ui=0 ;;
    --no-ui-build) ui_build=0 ;;
    --no-nix) nix_check=0 ;;
    --base) base="$2"; shift ;;
    -h | --help) sed -n '2,/^set /p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

if [[ -z "${IN_NIX_SHELL:-}" ]] && command -v nix >/dev/null; then
  exec nix develop "$root" -c "$0" "${args[@]}"
fi
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

base="${base:-$(git merge-base HEAD origin/main 2>/dev/null || echo HEAD)}"
changed="$( (git diff --name-only "$base"; git ls-files --others --exclude-standard) | sort -u)"
changes() { grep -Eq "$1" <<< "$changed"; }

failed=()
step() {
  local name="$1"
  shift
  echo
  echo "==> $name"
  if ! "$@"; then failed+=("$name"); fi
}

if [[ ${#crates[@]} -eq 0 ]]; then
  scope=(--workspace)
else
  scope=()
  for crate in "${crates[@]}"; do scope+=(-p "$crate"); done
fi

step "cargo fmt" cargo fmt --all --check
if [[ $clippy == 1 ]]; then
  step "cargo clippy" cargo clippy "${scope[@]}" --all-targets --locked -- -D warnings
fi
if [[ $tests == 1 ]]; then
  step "cargo test" cargo test "${scope[@]}" --locked
fi
# The workspace's tests include it; a scoped run checks it on its own.
if [[ $tests == 0 || ${#crates[@]} -gt 0 ]]; then
  step "bindings" cargo test --locked -p keefast --bin keefast -- \
    bindings_are_up_to_date no_field_reaches_the_ui_as_null_unless_it_is_an_option
fi

if [[ $ui == 1 ]]; then
  ui_dir=crates/app/ui
  step "ui: pnpm install" pnpm --dir "$ui_dir" install --frozen-lockfile
  step "ui: typecheck" pnpm --dir "$ui_dir" typecheck
  step "ui: test" pnpm --dir "$ui_dir" test
  if [[ $ui_build == 1 ]]; then
    step "ui: build" pnpm --dir "$ui_dir" build
    step "ui: build:mock" pnpm --dir "$ui_dir" build:mock
  fi
fi

# CI's "nix" job runs on the same paths; it also builds the package, which this leaves to you.
nix_paths='^(flake\.nix|flake\.lock|nix/|Cargo\.(toml|lock)$|crates/[^/]+/(Cargo\.toml|build\.rs)$|crates/app/tauri\.conf\.json$|crates/app/ui/(package\.json|pnpm-lock\.yaml)$|assets/|packaging/|\.github/workflows/ci\.yml$)'
if [[ $nix_check == 1 ]] && changes "$nix_paths"; then
  step "nix flake check" nix flake check --no-build
fi

if changes '^extension/'; then
  step "extension: pnpm install" pnpm --dir extension install --frozen-lockfile
  step "extension: typecheck" pnpm --dir extension typecheck
  step "extension: test" pnpm --dir extension test
fi

if changes '^(plugins/|crates/plugin-)'; then
  step "plugins: card and SDK" nix develop .#plugins -c bash -c 'cd plugins && cargo fmt --all --check &&
    cargo clippy --all-targets --locked -- -D warnings && cargo test --locked'
  step "plugins: card.wasm" nix develop .#plugins -c plugins/card/build.sh --check
  step "plugins: template" nix develop .#plugins -c bash -c 'cd plugins/template && cargo test --locked'
fi

echo
if [[ ${#failed[@]} -gt 0 ]]; then
  printf 'FAILED: %s\n' "${failed[@]}" >&2
  exit 1
fi
echo "All checks passed."
