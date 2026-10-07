# Strict KDBX round-trip fidelity, KDBX 4 writes only

keefast reads KDBX 3.1 and 4.x but always writes KDBX 4.x, and every save must preserve data we don't model ourselves (unknown XML, custom data, history, icons, attachments), so Vault files stay interchangeable with KeePassXC, KeePass2 and KeeWeb. KDBX handling starts on the `keepass` crate behind our own domain layer, and that layer is gated by a round-trip test corpus of real files. If the crate fails the corpus, we fork it or replace it.
