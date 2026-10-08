#!/usr/bin/env bash
# Regenerates the committed fixture Vault files: the round-trip corpus that gates keefast's KDBX
# layer (ADR 0002). Each file is written by a real client's own file layer:
#
#   keepassxc-*  KeePassXC (keepassxc-cli)
#   keepass2-*   KeePass2 (KeePassLib, run under Mono)
#   keeweb-*     KeeWeb (kdbxweb, under Node)
#   pykeepass-*  pykeepass, which keeps the raw XML tree and so carries XML nobody models
#
# The passwords are throwaway test values; never put real secrets here.
#
# Usage: nix develop .#corpus -c scripts/make-fixtures.sh
# Needs network access the first time, to fetch kdbxweb and hash-wasm from npm.
set -euo pipefail

scripts=$(cd "$(dirname "$0")" && pwd)
cd "$scripts/../fixtures"

pw=fixture-password
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

: "${KEEPASSLIB:?KEEPASSLIB must point at the directory holding KeePassLib.dll (see flake.nix)}"

## KeePassXC: KDBX 3.1 and 4.0, AES, AES-KDF

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
add "Work/GitHub" octocat https://github.com/login gh-old-secret "Work account"
add "Work/Servers/Prod DB" postgres https://db.example.com pg-secret "Production database"
add "Personal/Email" me@example.org https://mail.example.org mail-secret "Personal mailbox"

# Changing the password keeps the old version in the Entry's history.
printf '%s\n%s\n' "$pw" gh-secret | keepassxc-cli edit -q -p "$draft" "Work/GitHub"
printf 'recovery-code-1\nrecovery-code-2\n' > "$work/codes.txt"
printf '%s\n' "$pw" |
  keepassxc-cli attachment-import -q "$draft" "Work/GitHub" recovery-codes.txt "$work/codes.txt"

cp "$draft" keepassxc-kdbx3.kdbx

# KeePassXC only writes KDBX 4 when the Vault needs it, so give it custom data (which also
# stands in for KeePassXC-Browser's settings) and a custom icon, round-tripping through XML.
printf '%s\n' "$pw" | keepassxc-cli export -q -f xml "$draft" > "$work/draft.xml"
python3 - "$work/draft.xml" <<'EOF'
import sys
from lxml import etree

path = sys.argv[1]
tree = etree.parse(path)
meta = tree.find("Meta")

def item(parent, key, value):
    data = parent.find("CustomData")
    if data is None:
        data = etree.SubElement(parent, "CustomData")
    node = etree.SubElement(data, "Item")
    etree.SubElement(node, "Key").text = key
    etree.SubElement(node, "Value").text = value

icon_uuid = "3q2+796tvu/erb7v3q2+7w=="
icon = etree.SubElement(meta.find("CustomIcons"), "Icon")
etree.SubElement(icon, "UUID").text = icon_uuid
etree.SubElement(icon, "Data").text = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
)
item(meta, "KPXC_BROWSER_fixture-browser", '{"id":"fixture-browser","key":"not-a-real-key"}')

first = tree.find(".//Entry")
item(first, "keefast-fixture", "kdbx4")
etree.SubElement(first, "CustomIconUUID").text = icon_uuid
tree.write(path, xml_declaration=True, encoding="UTF-8")
EOF

rm -f keepassxc-kdbx4.kdbx
printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli import -q -p -t 100 "$work/draft.xml" keepassxc-kdbx4.kdbx

## KeePass2: KDBX 3.1 (AES, AES-KDF) and 4.1 (ChaCha20, Argon2id)

mcs -nologo -r:"$KEEPASSLIB/KeePassLib.dll" "$scripts/fixtures/keepass2.cs" -out:"$work/keepass2.exe"
for version in kdbx3 kdbx4; do
  MONO_PATH=$KEEPASSLIB mono "$work/keepass2.exe" write "keepass2-$version.kdbx" "$pw" "$version"
done

## KeeWeb: KDBX 4.0, ChaCha20, Argon2d

# Node resolves ES module imports next to the script, so run it from the npm install.
mkdir "$work/keeweb"
(cd "$work/keeweb" && npm init -y > /dev/null && npm install --silent kdbxweb@2.1.1 hash-wasm@4.12.0)
cp "$scripts/fixtures/keeweb.mjs" "$work/keeweb/"
node "$work/keeweb/keeweb.mjs" write keeweb-kdbx4.kdbx "$pw"

## pykeepass: unknown XML elements and attributes, KDBX 3.1 and 4.1

# pykeepass fails to decrypt the protected values in KeePass2's forced KDBX 3.1 file, so the 3.1
# file starts from KeePassXC's.
python3 -I "$scripts/fixtures/unknown_xml.py" keepassxc-kdbx3.kdbx pykeepass-unknown-xml-kdbx3.kdbx "$pw"
python3 -I "$scripts/fixtures/unknown_xml.py" keepass2-kdbx4.kdbx pykeepass-unknown-xml-kdbx4.kdbx "$pw"

## Key files: one Vault file per KeePass key file format

# Kept out of the top-level corpus, whose files all open with the password alone.
rm -rf key-files
mkdir key-files
cd key-files

# Format 1, XML v1.0: the key is Base64. KeePassXC only generates v2.0 now, so write it by hand.
cat > xml-v1.key <<'XML'
<?xml version="1.0" encoding="utf-8"?>
<KeyFile>
	<Meta>
		<Version>1.00</Version>
	</Meta>
	<Key>
		<Data>a2VlZmFzdCB4bWwgdjEga2V5IGZpbGUgZml4dHVyZSE=</Data>
	</Key>
</KeyFile>
XML
# Format 2, XML v2.0: the key is hex with a hash, as KeePassXC generates it.
keepassxc-cli db-create -q -t 100 --set-key-file xml-v2.keyx "$work/unused.kdbx"
# Formats 3 and 4: exactly 32 raw bytes, or exactly 64 hex digits.
printf 'keefast raw 32-byte key fixture!' > raw-32.key
printf '%s' 6b656566617374206865782036342d6469676974206b65792066697874757265 > hex-64.key
# Format 5: any other file, hashed.
printf 'Any file can be a key file; this one is hashed.\n' > any-file.txt

# KeePassXC writes these as KDBX 3.1. Key file alone, or with the password.
create() {
  local out=$1 key=$2 with_password=$3
  if [ "$with_password" = yes ]; then
    printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli db-create -q -p -t 100 --set-key-file "$key" "$out"
    printf '%s\n%s\n' "$pw" key-file-secret |
      keepassxc-cli add -q -p -k "$key" -u key-user "$out" "Key file Entry"
  else
    keepassxc-cli db-create -q -t 100 --set-key-file "$key" "$out"
    printf '%s\n' key-file-secret |
      keepassxc-cli add -q -p --no-password -k "$key" -u key-user "$out" "Key file Entry"
  fi
}
create keepassxc-xml-v1.kdbx xml-v1.key no
create keepassxc-xml-v2.kdbx xml-v2.keyx yes
create keepassxc-raw-32.kdbx raw-32.key no
create keepassxc-hex-64.kdbx hex-64.key no
create keepassxc-any-file.kdbx any-file.txt yes

# KeePass2 writes KDBX 4.1 with Argon2id: the password and KeePassXC's XML v2.0 key file.
MONO_PATH=$KEEPASSLIB mono "$work/keepass2.exe" write keepass2-xml-v2-kdbx4.kdbx "$pw" kdbx4 xml-v2.keyx
# KeePass 2 can also combine an empty password with a key file, unlike a key file alone.
MONO_PATH=$KEEPASSLIB mono "$work/keepass2.exe" write keepass2-empty-password-kdbx4.kdbx "" kdbx4 any-file.txt
cd ..

chmod 644 ./*.kdbx key-files/*
