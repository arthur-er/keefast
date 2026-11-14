"""Lowers the AES-KDF rounds of fixture Vault files, in place, keeping the rest of what their
client wrote.

KeePassXC picks AES-KDF rounds by benchmarking the machine (`-t 100`: 100 ms), which came to about
six million. Every open and save of the fixture derives the key again, so the tests spent most of
their time there. This rewrites only the rounds in the outer header, then what depends on them: the
header's hash (and, in KDBX 4, its HMAC), and the encrypted payload under the new key with the same
seeds and IV. The payload's plaintext is kept byte for byte, except that in KDBX 3.1 the XML's
`HeaderHash` (same length) names the new header. Files using another KDF are left as they are.

Usage: python3 -I cheap_kdf.py ROUNDS PASSWORD|- [KEY_FILE|-] FILE...
A password or key file given as `-` isn't part of the key.
"""

import base64
import gzip
import hashlib
import hmac
import re
import struct
import sys
import zlib

from Cryptodome.Cipher import AES, ChaCha20

SIGNATURE = bytes.fromhex("03d9a29a67fb4bb5")
AES_CBC = bytes.fromhex("31c1f2e6bf714350be5805216afc5aff")
CHACHA20 = bytes.fromhex("d6038a2b8b6f4cb5a524339a31dbb59a")
AES_KDF_3 = bytes.fromhex("c9d9f39a628a4460bf740d08c18a4fea")
AES_KDF_4 = bytes.fromhex("7c02bb8279a74ac0927d114a00648238")


def key_file_key(path):
    data = open(path, "rb").read()
    if data.lstrip().startswith(b"<?xml") or data.lstrip().startswith(b"<KeyFile"):
        text = data.decode("utf-8")
        version = re.search(r"<Version>\s*([\d.]+)\s*</Version>", text).group(1)
        key = re.search(r"<Data[^>]*>(.*?)</Data>", text, re.S).group(1)
        if version.startswith("2"):
            return bytes.fromhex("".join(key.split()))
        return base64.b64decode(key.strip())
    if len(data) == 32:
        return data
    if len(data) == 64 and re.fullmatch(rb"[0-9a-fA-F]{64}", data):
        return bytes.fromhex(data.decode())
    return hashlib.sha256(data).digest()


def composite_key(password, key_file):
    parts = b""
    if password is not None:
        parts += hashlib.sha256(password.encode("utf-8")).digest()
    if key_file is not None:
        parts += key_file_key(key_file)
    return hashlib.sha256(parts).digest()


def aes_kdf(composite, seed, rounds):
    cipher = AES.new(seed, AES.MODE_ECB)
    key = composite
    for _ in range(rounds):
        key = cipher.encrypt(key)
    return hashlib.sha256(key).digest()


def crypt(cipher_id, key, iv, data, encrypt):
    if cipher_id == AES_CBC:
        cipher = AES.new(key, AES.MODE_CBC, iv)
        if encrypt:
            pad = 16 - len(data) % 16
            return cipher.encrypt(data + bytes([pad]) * pad)
        plain = cipher.decrypt(data)
        return plain[: -plain[-1]]
    if cipher_id == CHACHA20:
        return ChaCha20.new(key=key, nonce=iv).encrypt(data)
    raise SystemExit(f"unsupported cipher {cipher_id.hex()}")


def rewrite_kdbx3(file, composite, rounds):
    fields, pos = {}, 12
    while True:
        kind, size = file[pos], struct.unpack_from("<H", file, pos + 1)[0]
        fields[kind] = (pos + 3, file[pos + 3 : pos + 3 + size])
        pos += 3 + size
        if kind == 0:
            break
    header_end = pos
    if fields[2][1] != AES_CBC and fields[2][1] != CHACHA20:
        raise SystemExit("unsupported cipher")
    old_rounds = struct.unpack("<Q", fields[6][1])[0]
    if old_rounds <= rounds:
        return None
    master_seed, seed, iv = fields[4][1], fields[5][1], fields[7][1]
    start_bytes, compressed = fields[9][1], struct.unpack("<I", fields[3][1])[0] == 1

    old_key = hashlib.sha256(master_seed + aes_kdf(composite, seed, old_rounds)).digest()
    plain = crypt(fields[2][1], old_key, iv, file[header_end:], False)
    assert plain[:32] == start_bytes, "wrong key"
    blocks, data, pos = [], b"", 32
    while True:
        _, digest, size = struct.unpack_from("<I32sI", plain, pos)
        pos += 40
        if size == 0:
            break
        block = plain[pos : pos + size]
        assert hashlib.sha256(block).digest() == digest
        blocks.append(size)
        data += block
        pos += size
    xml = gzip.decompress(data) if compressed else data

    header = bytearray(file[:header_end])
    header[fields[6][0] : fields[6][0] + 8] = struct.pack("<Q", rounds)
    old_hash = base64.b64encode(hashlib.sha256(file[:header_end]).digest())
    new_hash = base64.b64encode(hashlib.sha256(header).digest())
    tag = b"<HeaderHash>" + old_hash + b"</HeaderHash>"
    if tag in xml:
        xml = xml.replace(tag, b"<HeaderHash>" + new_hash + b"</HeaderHash>")
    data = gzip.compress(xml, mtime=0) if compressed else xml

    payload, index, at = start_bytes, 0, 0
    block_size = max(blocks) if blocks else len(data)
    while at < len(data):
        block = data[at : at + block_size]
        payload += struct.pack("<I32sI", index, hashlib.sha256(block).digest(), len(block)) + block
        index, at = index + 1, at + len(block)
    payload += struct.pack("<I32sI", index, b"\0" * 32, 0)
    new_key = hashlib.sha256(master_seed + aes_kdf(composite, seed, rounds)).digest()
    return bytes(header) + crypt(fields[2][1], new_key, iv, payload, True)


def variant_map(data):
    """The KDF parameters: name -> (offset of the value in `data`, type, value)."""
    items, pos = {}, 2
    while data[pos] != 0:
        kind = data[pos]
        name_len = struct.unpack_from("<i", data, pos + 1)[0]
        name = data[pos + 5 : pos + 5 + name_len].decode()
        pos += 5 + name_len
        value_len = struct.unpack_from("<i", data, pos)[0]
        items[name] = (pos + 4, kind, data[pos + 4 : pos + 4 + value_len])
        pos += 4 + value_len
    return items


def block_key(master_seed, transformed, index):
    base = hashlib.sha512(master_seed + transformed + b"\x01").digest()
    return hashlib.sha512(struct.pack("<Q", index) + base).digest()


def rewrite_kdbx4(file, composite, rounds):
    fields, pos = {}, 12
    while True:
        kind, size = file[pos], struct.unpack_from("<I", file, pos + 1)[0]
        fields[kind] = (pos + 5, file[pos + 5 : pos + 5 + size])
        pos += 5 + size
        if kind == 0:
            break
    header_end = pos
    kdf_at, kdf = fields[11]
    params = variant_map(kdf)
    if params["$UUID"][2] not in (AES_KDF_3, AES_KDF_4):
        return None
    r_at, r_kind, r_value = params["R"]
    assert r_kind == 0x05 and len(r_value) == 8
    old_rounds = struct.unpack("<Q", r_value)[0]
    if old_rounds <= rounds:
        return None
    seed, master_seed = params["S"][2], fields[4][1]
    cipher_id, iv = fields[2][1], fields[7][1]

    old_transformed = aes_kdf(composite, seed, old_rounds)
    header_hmac = file[header_end + 32 : header_end + 64]
    key = block_key(master_seed, old_transformed, 0xFFFFFFFFFFFFFFFF)
    assert hmac.new(key, file[:header_end], "sha256").digest() == header_hmac, "wrong key"
    sizes, cipher_text, pos, index = [], b"", header_end + 64, 0
    while True:
        size = struct.unpack_from("<i", file, pos + 32)[0]
        block = file[pos + 36 : pos + 36 + size]
        pos += 36 + size
        if size == 0:
            break
        sizes.append(size)
        cipher_text += block
        index += 1
    plain = crypt(cipher_id, hashlib.sha256(master_seed + old_transformed).digest(), iv, cipher_text, False)

    header = bytearray(file[:header_end])
    at = kdf_at + r_at
    header[at : at + 8] = struct.pack("<Q", rounds)
    header = bytes(header)
    transformed = aes_kdf(composite, seed, rounds)
    cipher_text = crypt(cipher_id, hashlib.sha256(master_seed + transformed).digest(), iv, plain, True)
    out = header + hashlib.sha256(header).digest()
    out += hmac.new(block_key(master_seed, transformed, 0xFFFFFFFFFFFFFFFF), header, "sha256").digest()
    block_size = max(sizes) if sizes else len(cipher_text)
    index, at = 0, 0
    while True:
        block = cipher_text[at : at + block_size]
        mac = hmac.new(
            block_key(master_seed, transformed, index),
            struct.pack("<Qi", index, len(block)) + block,
            "sha256",
        ).digest()
        out += mac + struct.pack("<i", len(block)) + block
        if not block:
            break
        index, at = index + 1, at + len(block)
    return out


def main():
    rounds = int(sys.argv[1])
    password = None if sys.argv[2] == "-" else sys.argv[2]
    key_file = None if sys.argv[3] == "-" else sys.argv[3]
    composite = composite_key(password, key_file)
    for path in sys.argv[4:]:
        file = open(path, "rb").read()
        assert file[:8] == SIGNATURE, f"{path}: not a KDBX file"
        major = struct.unpack_from("<H", file, 10)[0]
        # KDBX 3.1 only has AES-KDF.
        if major == 3:
            new = rewrite_kdbx3(file, composite, rounds)
        else:
            new = rewrite_kdbx4(file, composite, rounds)
        if new is None:
            print(f"{path}: not AES-KDF above {rounds} rounds, left as it is")
            continue
        open(path, "wb").write(new)
        print(f"{path}: AES-KDF at {rounds} rounds")


if __name__ == "__main__":
    main()
