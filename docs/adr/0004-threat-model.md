# Threat model: protect at rest, against the web, and when unattended, but not against same-user malware

keefast defends against: theft of the Vault file or the disk, malicious websites and page scripts trying to obtain credentials through the Companion extension, clipboard leakage (auto-clear), and an unlocked machine left unattended (auto-lock on idle, OS lock and suspend). Malware running as the same OS user is explicitly out of scope. Keys and decrypted secrets are zeroized on a best-effort basis (`zeroize`/`secrecy`), but we deliberately do not contort the GPUI layer to eliminate every in-memory copy of displayed text.

## Update 2026-10-10: Plugins the user installs

A malicious Plugin the user installed is now in scope. ADR 0009 confines Plugins to a WASM sandbox with no network or filesystem access, and limits what they can do to the Plugin capabilities the user granted.

## Update 2026-10-28: Other users on the same machine

Another local user is in scope for the link between the native host and keefast. On Windows, anyone could create the named pipe first and so receive what the browser sends; on Linux, the socket could fall back to the shared temp directory. Now keefast creates its pipe with an access list that lets only the current user connect, and its socket only in the user's private runtime directory (`XDG_RUNTIME_DIR`), in a directory only the user can enter; without a runtime directory it doesn't serve the browser and says why. The native host relays only to a keefast running as its own user, checked with the OS before relaying anything (the pipe server's process token on Windows, the socket peer's credentials on Linux), and otherwise tells the browser keefast isn't running. An administrator or root can still get around this, as they can read the user's memory anyway.
