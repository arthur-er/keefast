// The encoding and nonces of the KeePassXC-Browser protocol (ADR 0003).
import nacl from "tweetnacl";

export function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function decodeBase64(text: string): Uint8Array {
  return Uint8Array.from(atob(text), (char) => char.charCodeAt(0));
}

export function randomNonce(): Uint8Array {
  return nacl.randomBytes(nacl.box.nonceLength);
}

/** The nonce plus one, little-endian, as libsodium's `sodium_increment`: replies carry it. */
export function incrementNonce(nonce: Uint8Array): Uint8Array {
  const next = Uint8Array.from(nonce);
  for (let index = 0; index < next.length; index += 1) {
    next[index] = (next[index]! + 1) & 0xff;
    if (next[index] !== 0) break;
  }
  return next;
}
