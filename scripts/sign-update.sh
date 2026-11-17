#!/usr/bin/env bash
# Signs release packages with keefast's update key, so the in-app updater installs them (ADR 0014):
# it installs nothing whose signature doesn't verify with the public key in
# crates/app/tauri.conf.json. Writes <package>.sig next to each package. The release workflows
# run it in a step of its own, the only one given the secret key, after the UI's dependencies are
# installed (Tauri's CLI is one):
#
#   TAURI_SIGNING_PRIVATE_KEY=… TAURI_SIGNING_PRIVATE_KEY_PASSWORD=… scripts/sign-update.sh dist/keefast-*.AppImage
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <package>..." >&2
  exit 2
fi
[[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] || {
  echo "TAURI_SIGNING_PRIVATE_KEY is not set: a release needs keefast's update key" >&2
  exit 1
}
root="$(cd "$(dirname "$0")/.." && pwd)"

for package in "$@"; do
  package="$(realpath "$package")"
  rm -f "$package.sig"
  pnpm --silent --dir "$root/crates/app/ui" exec tauri signer sign "$package" >/dev/null
  [[ -s "$package.sig" ]] || { echo "no signature was written for $package" >&2; exit 1; }
  echo "$package.sig"
done
