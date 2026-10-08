#!/usr/bin/env bash
# Regenerates fixtures/totp.kdbx: a Vault file written by KeePassXC holding TOTP settings in
# every field format KeePass clients use, for keefast's TOTP tests. The secrets are the RFC 6238
# test secrets ("12345678901234567890" and its SHA-256 and SHA-512 lengths), Base32-encoded.
#
#   OTP URI         otp: otpauth:// URI, as KeePassXC and KeeWeb write it
#   OTP URI SHA-256 otp: otpauth:// URI with SHA-256, 8 digits, 60 second period
#   Steam           otp: otpauth:// URI with KeePassXC's Steam encoder
#   Legacy          TOTP Seed and TOTP Settings (8 digits), as older KeePassXC wrote them
#   KeeOtp          otp: KeeOtp plugin settings (key=…&size=…&step=…)
#   Bare secret     otp: a Base32 secret alone, in lowercase with spaces, which KeeWeb reads
#   KeePass2        TimeOtp-* fields, as KeePass 2.47+ writes them (SHA-512, 8 digits)
#   Broken          otp: an otpauth:// URI whose secret isn't Base32
#   No TOTP         no TOTP at all
#
# The password is a throwaway test value; never put real secrets here.
#
# Usage: nix develop .#corpus -c scripts/make-totp-fixture.sh
set -euo pipefail

scripts=$(cd "$(dirname "$0")" && pwd)
cd "$scripts/../fixtures"

pw=fixture-password
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
draft=$work/draft.kdbx

printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli db-create -q -p -t 100 "$draft"
for title in "OTP URI" "OTP URI SHA-256" "Steam" "Legacy" "KeeOtp" "Bare secret" "KeePass2" \
  "Broken" "No TOTP"; do
  printf '%s\n%s\n' "$pw" "password" | keepassxc-cli add -q -p -u alice "$draft" "$title"
done

# keepassxc-cli can't set TOTP fields, so they go in through KeePassXC's XML export and import.
printf '%s\n' "$pw" | keepassxc-cli export -q -f xml "$draft" > "$work/draft.xml"
python3 -I - "$work/draft.xml" <<'EOF'
import sys
import xml.etree.ElementTree as ET

path = sys.argv[1]
tree = ET.parse(path)
root = tree.getroot()

SHA1 = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
SHA256 = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA===="
SHA512 = (
    "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
    "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNA="
)

def entry(title):
    for element in root.iter("Entry"):
        if element.findtext("String[Key='Title']/Value") == title:
            return element
    raise KeyError(title)

def string(title, key, value, protect=False):
    element = entry(title)
    field = ET.Element("String")
    ET.SubElement(field, "Key").text = key
    value_element = ET.SubElement(field, "Value")
    value_element.text = value
    if protect:
        value_element.set("ProtectInMemory", "True")
    element.insert(list(element).index(element.find("AutoType")), field)

string("OTP URI", "otp",
       f"otpauth://totp/GitHub:alice?secret={SHA1}&period=30&digits=6&issuer=GitHub", True)
string("OTP URI SHA-256", "otp",
       f"otpauth://totp/Example:alice?secret={SHA256.rstrip('=')}&period=60&digits=8"
       "&issuer=Example&algorithm=SHA256", True)
string("Steam", "otp",
       f"otpauth://totp/Steam:alice?secret={SHA1}&period=30&digits=5&issuer=Steam&encoder=steam",
       True)
string("Legacy", "TOTP Seed", SHA1, True)
string("Legacy", "TOTP Settings", "30;8")
string("KeeOtp", "otp", f"key={SHA1}&size=8&step=30", True)
string("Bare secret", "otp", "gezd gnbv gy3t qojq gezd gnbv gy3t qojq", True)
string("KeePass2", "TimeOtp-Secret-Base32", SHA512, True)
string("KeePass2", "TimeOtp-Length", "8")
string("KeePass2", "TimeOtp-Period", "30")
string("KeePass2", "TimeOtp-Algorithm", "HMAC-SHA-512")
string("Broken", "otp", "otpauth://totp/Broken:alice?secret=not-base32!", True)
tree.write(path, xml_declaration=True, encoding="UTF-8")
EOF

rm -f totp.kdbx
printf '%s\n%s\n' "$pw" "$pw" | keepassxc-cli import -q -p -t 100 "$work/draft.xml" totp.kdbx
chmod 644 totp.kdbx
