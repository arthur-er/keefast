#!/usr/bin/env bash
# Writes latest.json, the update manifest the in-app updater reads (ADR 0014), from a release's
# signed packages: each package with a <package>.sig next to it (scripts/sign-update.sh) is the
# update for its platform. The release workflow attaches it to the draft release; once the
# release is published, it is at releases/latest/download/latest.json, where keefast looks.
#
#   scripts/update-manifest.sh <version> <assets dir> <download URL> > latest.json
#
# The download URL is where the release's assets are, such as
# https://github.com/arthur-er/keefast/releases/download/v0.2.0.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <version> <assets dir> <download URL>" >&2
  exit 2
fi
version="$1"
assets="$2"
url="${3%/}"

# The updater's platform for a package: {os}-{arch}, as tauri-plugin-updater names them.
platform() {
  case "$1" in
    keefast-*-x86_64.AppImage) echo linux-x86_64 ;;
    keefast-*-aarch64.AppImage) echo linux-aarch64 ;;
    keefast-*-windows-x86_64.msi) echo windows-x86_64 ;;
    *) return 1 ;;
  esac
}

platforms='{}'
for sig in "$assets"/*.sig; do
  [[ -e "$sig" ]] || break
  package="$(basename "${sig%.sig}")"
  [[ -f "$assets/$package" ]] || { echo "$sig signs no package in $assets" >&2; exit 1; }
  name="$(platform "$package")" || { echo "no updater platform for $package" >&2; exit 1; }
  platforms="$(jq --arg name "$name" --arg url "$url/$package" --rawfile signature "$sig" \
    '. + {($name): {url: $url, signature: $signature}}' <<<"$platforms")"
done
[[ "$platforms" != '{}' ]] || { echo "no signed packages in $assets" >&2; exit 1; }

jq -n --arg version "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson platforms "$platforms" \
  '{version: $version, notes: "See the release page for what changed.", pub_date: $date, platforms: $platforms}'
