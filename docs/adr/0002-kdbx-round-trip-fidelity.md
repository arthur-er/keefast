# Strict KDBX round-trip fidelity, KDBX 4 writes only

keefast reads KDBX 3.1 and 4.x but always writes KDBX 4.x, and every save must preserve data we don't model ourselves (unknown XML, custom data, history, icons, attachments), so Vault files stay interchangeable with KeePassXC, KeePass2 and KeeWeb. KDBX handling starts on the `keepass` crate behind our own domain layer, and that layer is gated by a round-trip test corpus of real files. If the crate fails the corpus, we fork it or replace it.

## Update 2026-10-07: the `keepass` crate is replaced

The `keepass` crate (0.15) failed the round-trip corpus, so keefast reads and writes KDBX itself (`crates/core/src/kdbx/`, `xml.rs`). The crate maps the XML onto typed structs, which can't carry what they don't declare:

- It rejects Vault files with unknown XML outright (`unknown variant FutureGroupSetting`), so it can't even open them, let alone save them unchanged.
- Opening a KDBX 3.1 file with several attachments keeps only the last one, because every attachment gets ID 0.
- Saving rebuilds the XML from its structs, so any element or attribute it doesn't model would be dropped.

Forking wouldn't fix this cheaply, because the typed model is the crate's core. Instead keefast keeps the whole decrypted XML document as a tree and builds its domain model from it. Saving writes the tree back with fresh seeds, the same cipher, compression and KDF settings, and header fields passed through. Opening a KDBX 3.1 file converts it to KDBX 4 form in memory: attachments move into the binary pool, timestamps change encoding, and `HeaderHash` is dropped. The crypto comes from the RustCrypto crates and `rust-argon2`. Writes are KDBX 4 only.

The corpus (`fixtures/`, written by `scripts/make-fixtures.sh` with KeePassXC, KeePass2, KeeWeb's kdbxweb and pykeepass) covers KDBX 3.1/4.0/4.1, AES/ChaCha20, AES-KDF/Argon2d/Argon2id, attachments, history, custom icons, plugin custom data and unknown XML. `crates/core/tests/round_trip.rs` gates it in CI, and still uses the `keepass` crate as an independent reader on the files it parses correctly. `scripts/check-interop.sh` checks keefast's saves with the real clients: KeePassXC, KeePass2 and KeeWeb each read every re-saved corpus file the same as the original. It needs those clients, so it doesn't run in CI.
