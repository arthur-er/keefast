#!/usr/bin/env bash
# Regenerates fixtures/keepassxc-browser.kdbx: a Vault file written by KeePassXC holding what
# its browser integration reads from Entries and Groups, for keefast's matching and Access
# decision tests. Kept apart from make-fixtures.sh so the round-trip corpus stays as it is.
#
#   Example            https://example.com, plus additional URLs (KP2A_URL, KP2A_URL_1)
#   Hidden             https://example.com, hidden from browsers (BrowserHideEntry)
#   Remembered         https://example.com, example.com allowed and evil.example.com denied
#   Other site         https://other.test
#   Hidden group/      hidden from browsers, with an Entry for https://example.com
#     Shown again/     shown again (BrowserHideEntry false), with an Entry for example.com
#   Recycle Bin/       the Vault's Recycle Bin, with a deleted Entry for https://example.com
#
# The password is a throwaway test value; never put real secrets here.
#
# Usage: nix develop .#corpus -c scripts/make-browser-fixture.sh
set -euo pipefail

scripts=$(cd "$(dirname "$0")" && pwd)
cd "$scripts/../fixtures"

pw=fixture-password
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
draft=$work/draft.kdbx

printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli db-create -q -p -t 100 "$draft"
for group in "Hidden group" "Hidden group/Shown again" "Recycle Bin"; do
  printf '%s\n' "$pw" | keepassxc-cli mkdir -q "$draft" "$group"
done

add() {
  local path=$1 user=$2 url=$3 secret=$4
  printf '%s\n%s\n' "$pw" "$secret" | keepassxc-cli add -q -p -u "$user" --url "$url" "$draft" "$path"
}

add "Example" alice https://example.com example-secret
add "Hidden" hidden https://example.com hidden-secret
add "Remembered" remembered https://example.com remembered-secret
add "Other site" other https://other.test other-secret
add "Hidden group/In hidden group" grouped https://example.com grouped-secret
add "Hidden group/Shown again/Shown again" shown example.com shown-secret
add "Recycle Bin/Deleted" deleted https://example.com deleted-secret

# keepassxc-cli can't set additional URLs, custom data or the Recycle Bin, so they go in
# through KeePassXC's XML export and import.
printf '%s\n' "$pw" | keepassxc-cli export -q -f xml "$draft" > "$work/draft.xml"
python3 -I - "$work/draft.xml" <<'EOF'
import sys
import xml.etree.ElementTree as ET

path = sys.argv[1]
tree = ET.parse(path)
root = tree.getroot()

def find(kind, name):
    field = "Name" if kind == "Group" else "String[Key='Title']/Value"
    for element in root.iter(kind):
        if element.findtext(field) == name:
            return element
    raise KeyError(name)

def custom_data(element, key, value):
    data = element.find("CustomData")
    if data is None:
        data = ET.SubElement(element, "CustomData")
    item = ET.SubElement(data, "Item")
    ET.SubElement(item, "Key").text = key
    ET.SubElement(item, "Value").text = value

def string(entry, key, value):
    field = ET.Element("String")
    ET.SubElement(field, "Key").text = key
    ET.SubElement(field, "Value").text = value
    entry.insert(list(entry).index(entry.find("AutoType")), field)

example = find("Entry", "Example")
string(example, "KP2A_URL", "https://example.org")
string(example, "KP2A_URL_1", "https://login.example.net")
custom_data(find("Entry", "Hidden"), "BrowserHideEntry", "true")
custom_data(
    find("Entry", "Remembered"),
    "KeePassXC-Browser Settings",
    '{"Allow":["example.com"],"Deny":["evil.example.com"],"Realm":""}',
)
custom_data(find("Group", "Hidden group"), "BrowserHideEntry", "true")
custom_data(find("Group", "Shown again"), "BrowserHideEntry", "false")

meta = root.find("Meta")
meta.find("RecycleBinEnabled").text = "True"
meta.find("RecycleBinUUID").text = find("Group", "Recycle Bin").findtext("UUID")
tree.write(path, xml_declaration=True, encoding="UTF-8")
EOF

rm -f keepassxc-browser.kdbx
printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli import -q -p -t 100 "$work/draft.xml" keepassxc-browser.kdbx
# KeePassXC benchmarks AES-KDF to millions of rounds; cut them so each open in the tests is quick
# (scripts/fixtures/cheap_kdf.py keeps the rest of what KeePassXC wrote).
python3 -I "$scripts/fixtures/cheap_kdf.py" 1000 "$pw" - keepassxc-browser.kdbx
chmod 644 keepassxc-browser.kdbx
