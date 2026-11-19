#!/usr/bin/env bash
# Gets a fresh worktree ready to build, test and run: the dev shell, the desktop UI's
# dependencies, the pre-commit hook, and this worktree's own port for the mock UI. t3.json runs it
# when T3 Code creates a worktree; it is safe to run again by hand.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# Everything below runs in the flake's dev shell, which has Node and pnpm. Building it here once
# means the first `nix develop -c cargo …` doesn't wait on it.
if [[ -z "${IN_NIX_SHELL:-}" ]] && command -v nix >/dev/null; then
  exec nix develop "$root" -c "$0" "$@"
fi

echo "==> pnpm install (crates/app/ui)"
# A stale lockfile must fail, not be rewritten; closed stdin makes any prompt fail too.
pnpm --dir crates/app/ui install --frozen-lockfile < /dev/null

echo "==> pre-commit hook (.githooks/pre-commit)"
git config core.hooksPath .githooks

echo "==> mock UI port"
echo "scripts/ui-mock.sh start serves the mock UI on :$(scripts/ui-mock.sh port)"

echo "==> ready. Cargo goes through \`nix develop -c\`, one crate at a time (AGENTS.md)."
