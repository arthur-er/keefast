#!/usr/bin/env bash
# Regenerates the committed fixture Vault files with KeePassXC's CLI.
# The passwords are throwaway test values; never put real secrets here.
#
# Usage: nix shell nixpkgs#keepassxc -c scripts/make-fixtures.sh
set -euo pipefail

cd "$(dirname "$0")/../fixtures"

pw=fixture-password
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

draft=$work/draft.kdbx

# Without KDBX 4-only features KeePassXC writes KDBX 3.1, which becomes the 3.1 fixture.
printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli db-create -q -p -t 100 "$draft"

for group in Work Work/Servers Personal; do
  printf '%s\n' "$pw" | keepassxc-cli mkdir -q "$draft" "$group"
done

add() {
  local path=$1 user=$2 url=$3 secret=$4 notes=$5
  printf '%s\n%s\n' "$pw" "$secret" |
    keepassxc-cli add -q -p -u "$user" --url "$url" --notes "$notes" "$draft" "$path"
}

add "Root Entry" root@example.com https://example.com root-secret "Lives in the root Group"
add "Work/GitHub" octocat https://github.com/login gh-secret "Work account"
add "Work/Servers/Prod DB" postgres https://db.example.com pg-secret "Production database"
add "Personal/Email" me@example.org https://mail.example.org mail-secret "Personal mailbox"

cp "$draft" keepassxc-kdbx3.kdbx

# KeePassXC only writes KDBX 4 when the Vault needs it, so give one Entry custom data
# and round-trip through XML. A short decryption time keeps tests fast.
printf '%s\n' "$pw" | keepassxc-cli export -q -f xml "$draft" > "$work/draft.xml"
sed -i '0,/<Entry>/s//<Entry><CustomData><Item><Key>keefast-fixture<\/Key><Value>kdbx4<\/Value><\/Item><\/CustomData>/' \
  "$work/draft.xml"

rm -f keepassxc-kdbx4.kdbx
printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli import -q -p -t 100 "$work/draft.xml" keepassxc-kdbx4.kdbx
chmod 644 keepassxc-kdbx3.kdbx keepassxc-kdbx4.kdbx
