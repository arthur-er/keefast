# Threat model: protect at rest, against the web, and when unattended, but not against same-user malware

keefast defends against: theft of the Vault file or the disk, malicious websites and page scripts trying to obtain credentials through the Companion extension, clipboard leakage (auto-clear), and an unlocked machine left unattended (auto-lock on idle, OS lock and suspend). Malware running as the same OS user is explicitly out of scope. Keys and decrypted secrets are zeroized on a best-effort basis (`zeroize`/`secrecy`), but we deliberately do not contort the GPUI layer to eliminate every in-memory copy of displayed text.

## Update 2026-10-10: Plugins the user installs

A malicious Plugin the user installed is now in scope. ADR 0009 confines Plugins to a WASM sandbox with no network or filesystem access, and limits what they can do to the Plugin capabilities the user granted.
