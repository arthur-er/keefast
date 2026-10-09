"""Adds XML that no KeePass client models to a Vault file, using pykeepass.

pykeepass edits the decrypted XML tree directly and writes back whatever is in it, which makes
it a real-world source of Vault files carrying data a client doesn't understand (newer format
versions, plugins, other clients).

Usage: python unknown_xml.py <in.kdbx> <out.kdbx> <password>
"""

import sys

from lxml import etree
from pykeepass import PyKeePass

source, target, password = sys.argv[1:]
kp = PyKeePass(source, password=password)
root = kp.tree.getroot()


def unknown(parent, tag, text=None, **attributes):
    element = etree.SubElement(parent, tag, attributes)
    element.text = text
    return element


meta = root.find("Meta")
# pykeepass rewrites the KDBX 3.1 header but not this hash of it, and readers check it when
# present. It is optional, so drop it.
header_hash = meta.find("HeaderHash")
if header_hash is not None:
    meta.remove(header_hash)
future = unknown(meta, "KeefastFutureMeta", Origin="pykeepass")
unknown(future, "Nested", "meta-payload", Flag="1")
unknown(future, "Empty")

top = root.find("Root/Group")
top.set("FutureGroupAttribute", "group-attribute")
unknown(top, "FutureGroupSetting", "group-payload")

entry = root.find(".//Group/Entry")
entry.set("FutureEntryAttribute", "entry-attribute")
unknown(entry, "FutureEntryField", "entry-payload", Kind="opaque")
entry.find("String/Value").set("FutureValueAttribute", "value-attribute")
history_entry = entry.find("History/Entry")
if history_entry is not None:
    unknown(history_entry, "FutureHistoryField", "history-payload")

unknown(root.find("Root"), "FutureRootElement", "root-payload")

kp.save(target)
