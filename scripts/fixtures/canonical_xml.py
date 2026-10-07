"""Canonicalises a client's XML export of a Vault, so exports of the same Vault compare equal
whatever format version the Vault file had. Used by scripts/check-interop.sh.

Removes what differs between exports but isn't Vault content: export-time custom data that
KeePassXC adds, version-dependent Meta fields, timestamp encodings and attachment numbering.

Usage: python canonical_xml.py [--without-attachments] < export.xml > canonical.xml

--without-attachments compares attachments by name only, for exports that leave attachment
contents out (KeePassXC's, of KDBX 4 files).
"""

import base64
import datetime
import gzip
import hashlib
import sys

from lxml import etree

TIMESTAMPS = {
    "CreationTime", "LastModificationTime", "LastAccessTime", "ExpiryTime", "LocationChanged",
    "DatabaseNameChanged", "DatabaseDescriptionChanged", "DefaultUserNameChanged",
    "MasterKeyChanged", "RecycleBinChanged", "EntryTemplatesGroupChanged", "DeletionTime",
}
# KeePassXC writes these into every export, with the export's own time or a random value.
EXPORT_NOISE = {"_LAST_MODIFIED", "KPXC_RANDOM_SLUG"}
EPOCH = datetime.datetime(1, 1, 1, tzinfo=datetime.timezone.utc)

tree = etree.parse(sys.stdin.buffer, etree.XMLParser(remove_blank_text=True))
root = tree.getroot()


def remove(element):
    element.getparent().remove(element)


for element in root.iter(*TIMESTAMPS):
    text = (element.text or "").strip()
    try:
        raw = base64.b64decode(text, validate=True)
    except ValueError:
        continue
    if len(raw) == 8:
        seconds = int.from_bytes(raw, "little", signed=True)
        time = EPOCH + datetime.timedelta(seconds=seconds)
        element.text = time.strftime("%Y-%m-%dT%H:%M:%SZ")

for item in list(root.iter("Item")):
    if item.findtext("Key") in EXPORT_NOISE:
        remove(item)

meta = root.find("Meta")
for name in ("HeaderHash", "SettingsChanged"):
    for element in meta.findall(name):
        remove(element)

# Attachments are numbered per file; refer to them by content instead.
digests = {}
binaries = meta.find("Binaries")
if binaries is not None:
    for binary in binaries.findall("Binary"):
        data = base64.b64decode(binary.text or "")
        if binary.get("Compressed", "").lower() == "true":
            data = gzip.decompress(data)
        digests[binary.get("ID")] = hashlib.sha256(data).hexdigest()
    remove(binaries)
without_attachments = "--without-attachments" in sys.argv[1:]
for value in root.iter("Value"):
    if value.get("Ref") is not None:
        digest = "*" if without_attachments else digests.get(value.get("Ref"), "missing")
        value.set("Ref", digest)

sys.stdout.write(etree.tostring(root, pretty_print=True, encoding="unicode"))
