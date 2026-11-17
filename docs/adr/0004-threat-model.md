# Threat model: protect at rest, against the web, and when unattended, but not against same-user malware

keefast defends against: theft of the Vault file or the disk, malicious websites and page scripts trying to obtain credentials through the Companion extension, clipboard leakage (auto-clear), and an unlocked machine left unattended (auto-lock on idle, OS lock and suspend). Malware running as the same OS user is explicitly out of scope. Keys and decrypted secrets are zeroized on a best-effort basis (`zeroize`/`secrecy`), but we deliberately do not contort the GPUI layer to eliminate every in-memory copy of displayed text.

## Update 2026-10-10: Plugins the user installs

A malicious Plugin the user installed is now in scope. ADR 0009 confines Plugins to a WASM sandbox with no network or filesystem access, and limits what they can do to the Plugin capabilities the user granted.

## Update 2026-10-28: Other users on the same machine

Another local user is in scope for the link between the native host and keefast. On Windows, anyone could create the named pipe first and so receive what the browser sends; on Linux, the socket could fall back to the shared temp directory. Now keefast creates its pipe with an access list that lets only the current user connect, and its socket only in the user's private runtime directory (`XDG_RUNTIME_DIR`), in a directory only the user can enter; without a runtime directory it doesn't serve the browser and says why. The native host relays only to a keefast running as its own user, checked with the OS before relaying anything (the pipe server's process token on Windows, the socket peer's credentials on Linux), and otherwise tells the browser keefast isn't running. An administrator or root can still get around this, as they can read the user's memory anyway.

## Update 2026-11-16: Agents

ADR 0012 lets AI Agents use secrets from the Agent group. A connected Agent that is prompt-injected or otherwise misbehaves is now in scope. It can use only what its Agent pairing reaches, gets values through `keefast-agent run` and `http_request` rather than seeing them where possible, and needs an Agent access decision for each Entry and program or host. Every use is logged. A Headless host adds anyone who can reach its address: it refuses to listen beyond loopback without TLS and has no unauthenticated mode. It runs as a user of its own, so Agents running as the operator on the same server are outside callers too. A Headless host unlocked unattended gives up protection at rest for its Vault, and says so when it is set up. Output redaction is not a defence against an Agent bent on leaking a value it was handed. The approval naming the program is.

## Update 2026-11-16: keefast running hidden

With Close to tray on, keefast keeps running, its Vaults unlocked, after the window closes; ADR 0013 says why the protections for an unattended machine still cover it.

## Update 2026-11-17: Updates

ADR 0014 lets keefast install new releases of itself. Whoever can publish a keefast release can now run code on users' machines, so a forged or tampered update is in scope. The updater installs only packages signed with keefast's update key, verified against the public key built into keefast, and fetched over HTTPS. The key is not the Plugin release key and is given only to CI's signing step. A release reaches users only once it is published by hand. A stolen update key with the power to publish a release is not defended against: that is a compromise of the project itself.
