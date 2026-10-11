#!/usr/bin/env bash
# Signs the Plugins bundled with keefast with keefast's release key, for a release build to
# compile in (crates/plugins/build.rs, docs/plugins/signing.md). The release workflows run it in
# a step of its own, the only one given the secret key, with a keefast-plugin built beforehand:
#
#   KEEFAST_PLUGIN_SIGNING_KEY=… scripts/sign-bundled-plugins.sh target/release/keefast-plugin target/signed-plugins
#
# then build keefast with KEEFAST_SIGNED_PLUGINS=target/signed-plugins.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <keefast-plugin binary> <out dir>" >&2
  exit 2
fi
cli="$1"
out="$2"
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$out"

# Keep in step with BUNDLED in crates/plugins/build.rs.
for name in card; do
  dir="$root/plugins/$name"
  icon=()
  if [[ -f "$dir/icon.png" ]]; then icon=(--icon "$dir/icon.png"); fi
  "$cli" pack --key-env KEEFAST_PLUGIN_SIGNING_KEY \
    --manifest "$dir/plugin.json" --component "$dir/$name.wasm" "${icon[@]}" \
    -o "$out/$name.kfplugin"
done
