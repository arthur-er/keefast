# Browser-integration settings live inside the Vault file

Pairings, the Default access policy, remembered Access decisions and the Browser group are stored in the Vault file's custom data, using KeePassXC-compatible keys where they exist, not in keefast's app config. This lets them travel with the Vault across machines and stay compatible with KeePassXC. The app config (TOML) holds only machine-local preferences, such as recent Vault files, theme, lock timings and the browser integration toggle. It never holds secrets: Dropbox tokens go in the OS secret store.

## Update 2026-10-29: Tag colours live there too

KDBX tags are plain text, so the colour chosen for a tag needs a place of its own. It goes in the Vault file's custom data, like the settings above, so it travels with the Vault: one item per tag, `keefast_TAG_COLOR_<tag>`, holding the `#RRGGBB` keefast writes for an Entry's colour. KeePassXC, KeePass2 and KeeWeb keep custom data items they don't know (ADR 0002) and show tags as they always have. Each item is timed, so a Merge keeps the newest choice per tag; undoing a choice leaves the item empty rather than removing it, so a Merge with an older Vault file doesn't bring the choice back. One item per tag rather than one map means choices made for different tags on two machines both survive a Merge.

## Update 2026-11-16: Agent settings live there too

Agent pairings (with their token hashes), Agent access decisions, the Default agent policy, each Entry's allowed hosts for `http_request`, and the "Agents may reveal" flag go in the Vault file's custom data, for the same reason as Pairings: approving on a desktop must reach a Headless host that serves the same Vault (ADR 0012). OAuth access and refresh tokens are the exception. They rotate on every refresh, and keeping them in the Vault would make each refresh a save and a Merge, so they stay hashed in the Headless host's own state. The audit log stays on the machine that wrote it.

## Update 2026-11-17: How the Agent settings are kept

Every Agent setting is a Vault-level custom data item of keefast's own, timed like a tag colour, so a Merge keeps the newest of each (crates/agent/src/settings.rs):

- `keefast_AGENT_GROUP`: the Agent group's UUID. `keefast_DEFAULT_AGENT_POLICY`: `ask`, `allow` or `deny`; a value keefast doesn't know asks.
- `keefast_AGENT_PAIRING_<id>`: one Agent pairing as JSON (name, own subgroup, readable subgroups, create right, and the SHA-256 of its token or its OAuth client). The id is random and never reused, so a rename rewrites the value under the same key. JSON keys a later keefast adds are kept when this one rewrites it.
- `keefast_AGENT_REVOKED_PAIRING_<id>`: marks a pairing revoked, for good. The pairing's own item is left as it is, so a Merge with a copy of the Vault that still has the pairing can't bring it back.
- `keefast_AGENT_DECISION_<Entry UUID>_<pairing id>_<program:name | host:name>`: one Agent access decision, `allow` or `deny`, left empty once forgotten.
- `keefast_AGENT_HOSTS_<Entry UUID>`: an Entry's allowed hosts, as a JSON array, `[]` once emptied. One item per Entry rather than per host: the list is edited as a whole, and the newest list wins a Merge.
- `keefast_AGENT_REVEAL_<Entry UUID>`: an Entry's "Agents may reveal" mark, `true`, or `false` once taken off.

Agent access decisions are "on each Entry" as far as the user sees them, but they are not kept in the Entry's own custom data, as KeePassXC-Browser's Access decisions are. An Entry is Merged whole, newest version winning, so two Approvers remembering decisions on the same Entry at once would lose one of them to Entry history. One timed Vault-level item per decision keeps both, and remembering a decision doesn't add an Entry history version.
