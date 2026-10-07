#!/usr/bin/env bash
# Checks that KeePassXC, KeePass2 and KeeWeb read every corpus Vault file the same before and
# after keefast saves it: each client exports both versions as XML, and after canonicalising
# away export noise (scripts/fixtures/canonical_xml.py) the two exports must be identical.
#
# Usage: nix develop .#corpus -c scripts/check-interop.sh
# Needs network access the first time, to fetch kdbxweb and hash-wasm from npm.
set -euo pipefail

scripts=$(cd "$(dirname "$0")" && pwd)
repo=$(dirname "$scripts")
pw=fixture-password
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

: "${KEEPASSLIB:?KEEPASSLIB must point at the directory holding KeePassLib.dll (see flake.nix)}"

(cd "$repo" && cargo build -q -p keefast-core --example resave)
resave=$repo/target/debug/examples/resave

mcs -nologo -r:"$KEEPASSLIB/KeePassLib.dll" "$scripts/fixtures/keepass2.cs" -out:"$work/keepass2.exe"
mkdir "$work/keeweb"
(cd "$work/keeweb" && npm init -y > /dev/null && npm install --silent kdbxweb@2.1.1 hash-wasm@4.12.0)
cp "$scripts/fixtures/keeweb.mjs" "$work/keeweb/"

export_xml() {
  local client=$1 file=$2
  case $client in
    # KeePassXC warns on stderr about each element it doesn't know, and its XML export of a
    # KDBX 4 file leaves attachment contents out.
    keepassxc)
      printf '%s\n' "$pw" | keepassxc-cli export -q -f xml "$file" 2> /dev/null |
        python3 -I "$scripts/fixtures/canonical_xml.py" --without-attachments
      ;;
    keepass2)
      MONO_PATH=$KEEPASSLIB mono "$work/keepass2.exe" export "$file" "$pw" |
        python3 -I "$scripts/fixtures/canonical_xml.py"
      ;;
    keeweb)
      node "$work/keeweb/keeweb.mjs" export "$file" "$pw" |
        python3 -I "$scripts/fixtures/canonical_xml.py"
      ;;
  esac
}

failed=0
for fixture in "$repo"/fixtures/*.kdbx; do
  name=$(basename "$fixture" .kdbx)
  "$resave" "$fixture" "$work/$name.kdbx" "$pw"
  for client in keepassxc keepass2 keeweb; do
    before=$work/$name.$client.before.xml
    after=$work/$name.$client.after.xml
    if ! export_xml "$client" "$fixture" > "$before"; then
      echo "skip  $name: $client can't read the original"
    elif ! export_xml "$client" "$work/$name.kdbx" > "$after"; then
      echo "FAIL  $name: $client can't read keefast's save"
      failed=1
    elif ! diff -u "$before" "$after"; then
      echo "FAIL  $name: $client reads keefast's save differently"
      failed=1
    else
      echo "ok    $name: $client"
    fi
  done
done
exit $failed
