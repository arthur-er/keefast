#!/usr/bin/env bash
# Checks that keefast Merges like KeePassXC and KeePass2: two versions of a Vault file are
# edited apart with keepassxc-cli (edits on both sides, an edit against a move, a move to the
# Recycle Bin, Entries and Groups added), then Merged both ways by keefast and by KeePassXC.
# KeePassXC must read keefast's Merges, and see in them the same Entries in the same Groups,
# with the same fields and history, as in its own.
#
# KeePassXC's merge leaves deletions alone unless a Group is set to synchronise, so a delete
# for good is checked against KeePass2's Synchronize instead, which honours DeletedObjects.
#
# Usage: nix develop .#corpus -c scripts/check-merge-interop.sh
set -euo pipefail

scripts=$(cd "$(dirname "$0")" && pwd)
repo=$(dirname "$scripts")
pw=fixture-password
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

: "${KEEPASSLIB:?KEEPASSLIB must point at the directory holding KeePassLib.dll (see flake.nix)}"

(cd "$repo" && cargo build -q -p keefast-core --example merge)
merge=$repo/target/debug/examples/merge
mcs -nologo -r:"$KEEPASSLIB/KeePassLib.dll" "$scripts/fixtures/keepass2.cs" -out:"$work/keepass2.exe"

# keepassxc-cli <command> on a Vault file; with -p, the Entry's new password follows.
kpxc() {
  local command=$1
  shift
  if [[ $1 == -p ]]; then
    printf '%s\n' "$pw" "$2" | keepassxc-cli "$command" -q -p "${@:3}"
  else
    printf '%s\n' "$pw" | keepassxc-cli "$command" -q "$@"
  fi
}

cp "$repo/fixtures/keepassxc-kdbx4.kdbx" "$work/ours.kdbx"
cp "$repo/fixtures/keepassxc-kdbx4.kdbx" "$work/theirs.kdbx"
chmod u+w "$work"/*.kdbx
ours=$work/ours.kdbx
theirs=$work/theirs.kdbx

# KDBX 4 keeps times to the second, so each round of edits gets its own second.
kpxc edit -p ours-secret "$ours" "Work/GitHub"
kpxc add -p bank-secret "$ours" "Personal/Bank"
kpxc mv "$ours" "Personal/Email" "Work"
sleep 2
kpxc edit -p theirs-secret "$theirs" "Work/GitHub"
kpxc rm "$theirs" "Work/Servers/Prod DB"
kpxc mkdir "$theirs" "Shared"
kpxc add -p wifi-secret "$theirs" "Shared/Wi-Fi"
kpxc mv "$theirs" "Root Entry" "Personal"
sleep 2
kpxc edit -p root-edited-later "$ours" "Root Entry"
# Deleted for good: to the Recycle Bin and out of it again.
deleted=$work/deleted.kdbx
cp "$ours" "$deleted"
kpxc rm "$deleted" "Work/Email"
kpxc rm "$deleted" "Recycle Bin/Email"

# The Entries KeePassXC reads in a Vault file: Group path, title, username, password, and the
# passwords in its history.
describe() {
  printf '%s\n' "$pw" | keepassxc-cli export -q -f xml "$1" 2> /dev/null |
    python3 -I -c '
import sys
import xml.etree.ElementTree as ET

def field(entry, key):
    for string in entry.findall("String"):
        if string.findtext("Key") == key:
            return string.findtext("Value") or ""
    return ""

def walk(group, path):
    path = path + "/" + (group.findtext("Name") or "")
    print(path)
    for entry in group.findall("Entry"):
        history = [field(old, "Password") for old in entry.findall("History/Entry")]
        print(path, "|", field(entry, "Title"), "|", field(entry, "UserName"), "|",
              field(entry, "Password"), "| history", sorted(history))
    for child in group.findall("Group"):
        walk(child, path)

walk(ET.parse(sys.stdin).getroot().find("Root/Group"), "")
' | sort
}

# Merges <from> into <into> with <client>, writing the Merge to <out>.
merge_with() {
  local client=$1 into=$2 from=$3 out=$4
  cp "$into" "$out"
  case $client in
    keefast) "$merge" "$into" "$from" "$out" "$pw" 2> /dev/null ;;
    keepassxc) printf '%s\n' "$pw" | keepassxc-cli merge -q -s "$out" "$from" 2> /dev/null ;;
    keepass2) MONO_PATH=$KEEPASSLIB mono "$work/keepass2.exe" merge "$out" "$from" "$pw" ;;
  esac
}

failed=0
for check in "keepassxc ours theirs" "keepassxc theirs ours" \
  "keepass2 deleted theirs" "keepass2 theirs deleted"; do
  read -r client into from <<< "$check"
  merge_with keefast "$work/$into.kdbx" "$work/$from.kdbx" "$work/keefast.kdbx"
  merge_with "$client" "$work/$into.kdbx" "$work/$from.kdbx" "$work/$client.kdbx"
  if ! describe "$work/keefast.kdbx" > "$work/keefast.txt"; then
    echo "FAIL  $from into $into: KeePassXC can't read keefast's Merge"
    failed=1
  elif ! diff -u <(describe "$work/$client.kdbx") "$work/keefast.txt"; then
    echo "FAIL  $from into $into: keefast Merges differently from $client"
    failed=1
  else
    echo "ok    $from into $into: as $client"
  fi
done
exit $failed
