# Browser-integration settings live inside the Vault file

Pairings, the Default access policy, remembered Access decisions and the Browser group are stored in the Vault file's custom data, using KeePassXC-compatible keys where they exist, not in keefast's app config. This lets them travel with the Vault across machines and stay compatible with KeePassXC. The app config (TOML) holds only machine-local preferences, such as recent Vault files, theme, lock timings and the browser integration toggle. It never holds secrets: Dropbox tokens go in the OS secret store.

## Update 2026-10-29: Tag colours live there too

KDBX tags are plain text, so the colour chosen for a tag needs a place of its own. It goes in the Vault file's custom data, like the settings above, so it travels with the Vault: one item per tag, `keefast_TAG_COLOR_<tag>`, holding the `#RRGGBB` keefast writes for an Entry's colour. KeePassXC, KeePass2 and KeeWeb keep custom data items they don't know (ADR 0002) and show tags as they always have. Each item is timed, so a Merge keeps the newest choice per tag; undoing a choice leaves the item empty rather than removing it, so a Merge with an older Vault file doesn't bring the choice back. One item per tag rather than one map means choices made for different tags on two machines both survive a Merge.

## Update 2026-11-16: Agent settings live there too

Agent pairings (with their token hashes), Agent access decisions, the Default agent policy, each Entry's allowed hosts for `http_request`, and the "Agents may reveal" flag go in the Vault file's custom data, for the same reason as Pairings: approving on a desktop must reach a Headless host that serves the same Vault (ADR 0012). OAuth access and refresh tokens are the exception. They rotate on every refresh, and keeping them in the Vault would make each refresh a save and a Merge, so they stay hashed in the Headless host's own state. The audit log stays on the machine that wrote it.

## Update 2026-11-17: Gateway pairings live there too

Agent pairings gain a kind, `agent` or `gateway` (ADR 0015). Gateway pairings are stored beside Agent pairings, with their token hash, for the same reason: one made on a desktop must reach the Headless host that serves the Vault. A pairing stored before kinds existed is an Agent pairing.
