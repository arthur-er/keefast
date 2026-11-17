# Spec: Gateway pairings — keefast as a source for agentes-chat's vault

Decisions: [ADR 0015](../adr/0015-gateways-resolve-values-for-another-vault.md), with updates to [ADR 0004](../adr/0004-threat-model.md), [ADR 0005](../adr/0005-browser-settings-live-in-the-vault.md) and [ADR 0012](../adr/0012-agents-reach-secrets-through-mcp.md). Builds on the Agents spec, [agents.md](agents.md). Terms: Gateway pairing and Resolve in the Agents section of [GLOSSARY.md](../../GLOSSARY.md).

## Problem Statement

My agents in agentes-chat sign in to sites and call APIs with logins and keys from agentes-chat's Vault. It can read them from 1Password or Bitwarden, but my secrets live in KeePass. To give an agentes-chat agent a login today I copy it into agentes-chat, and then it lives in two places, changes in one, and goes stale in the other. keefast's Agents design doesn't help: agentes-chat's runner needs the value itself, to type it into a browser or put it in a command's environment, and keefast hands values to nobody without asking each time.

## Solution

agentes-chat connects to a keefast Headless host as one of its Vault sources, as it connects to 1Password. On keefast's side that connection is a **Gateway pairing**: a pairing made by a person, given some Agent group subgroups, that lists their Entries as agentes-chat items and **resolves** a value whenever agentes-chat uses one, without a prompt. Every resolve is logged, with whom agentes-chat said it was for. Logins agentes-chat's agents make land in the pairing's own subgroup, in the KeePass Vault.

agentes-chat keeps its own rules: which of its agents and people may use an item, and that a value goes to its runner sealed and never to the model.

## User Stories

### Pairing a gateway

1. As an operator, I want to create a Gateway pairing with `keefast-agent pairing add --gateway <name>`, so that I can connect agentes-chat to a Headless host from ssh.
2. As a user, I want to create a Gateway pairing in the desktop app's Vault settings, so that I can do it without a terminal.
3. As a user, I want to choose which Agent group subgroups a Gateway pairing reads, so that agentes-chat gets only the secrets I put there.
4. As a user, I want to decide whether a Gateway pairing may create and change logins, so that a read-only connection stays read-only.
5. As a user, I want the Gateway pairing's token shown once and stored only as a hash, as for Agent pairings, so that the Vault file never holds a working token.
6. As a user, I want a Gateway pairing's token refused at the MCP endpoint, and an Agent pairing's at the gateway API, so that no model can be handed a tool that returns a value.
7. As a user, I want the OAuth flow never to produce a Gateway pairing, so that a gateway is always something I made by hand.
8. As a user, I want to revoke a Gateway pairing and have it stop at once, so that a leaked token is cut off.
9. As a user, I want Gateway pairings listed apart from Agent pairings, with what they read and whether they write, so that I can see what each connection reaches.

### Listing

10. As agentes-chat, I want to check a token and learn the Vault's name, the pairing's subgroups and whether it may write, so that the connection shows what it reads.
11. As agentes-chat, I want the Entries in my subgroups as logins and keys, never with a value, so that I can index them.
12. As a user, I want an Entry with an `https` URL and a password to become a login, with its `KP2A_URL*` URLs as further websites, so that one Entry fills every address of a site.
13. As a user, I want `http` URLs left out of a login's websites, so that a password is never typed into a page that isn't encrypted.
14. As a user, I want an Entry tagged `key`, or with no URL and an `UPPER_SNAKE` title, to become a key named by its title, so that my API keys reach agentes-chat's commands by name.
15. As a user, I want Entries that are neither, and every Entry's notes and custom fields, left out, so that agentes-chat sees only what it can use.
16. As agentes-chat, I want each item's ref to be its Entry's UUID, so that renaming or moving it keeps its history and access in agentes-chat.
17. As agentes-chat, I want a list to answer "not modified" when the Vault hasn't changed, so that polling every few minutes costs almost nothing.

### Resolving

18. As agentes-chat, I want to resolve the password of a login or the value of a key by its ref, so that my runner can type it or put it in a command.
19. As a user, I want a resolve to need no prompt for Entries in the pairing's subgroups, so that agentes-chat's routines work when I'm asleep.
20. As a user, I want every resolve logged with the Entry, the pairing, and the agent, person and run agentes-chat named, so that I can see what was used for whom.
21. As a user, I want the log to mark what agentes-chat said about who it was for as unverified, so that I don't mistake it for keefast's own knowledge.
22. As agentes-chat, I want a clear "not found" for an Entry gone from my subgroups, so that I mark its item missing.

### Writing

23. As agentes-chat, I want to create a login with a password I supply in the pairing's own subgroup, so that logins my agents make are kept in KeePass.
24. As agentes-chat, I want the new login returned as the list would show it, so that it is indexed at once.
25. As agentes-chat, I want to change the username or password of a login in the pairing's own subgroup, so that a password my agent changed on a site is changed in KeePass too.
26. As a user, I want each change kept in Entry history, so that I can see and undo it.
27. As agentes-chat, I want a change to an Entry outside my own subgroup refused as *not writable*, and a pairing without the right to write refused as *read only*, so that I keep the new password myself in the first case and stop trying in the second.
28. As a user, I want a gateway never to delete anything, so that a misbehaving gateway can't destroy my secrets.

### When things go wrong

29. As agentes-chat, I want every call to answer "locked" while the Headless host is locked, so that I tell people to unlock it rather than reporting keefast as broken.
30. As agentes-chat, I want errors as a code and keefast's own message, never a value, so that nothing secret ends up in my logs.
31. As an operator, I want the docs to recommend a Vault kept for Agents, with a subgroup kept for agentes-chat, unlocked unattended from a Key file by the NixOS module, and to say what that gives up, so that agentes-chat keeps working after a reboot and I know the risk.

## Implementation Decisions

- **Pairing kind.** Agent pairings in the Vault's custom data gain `kind`: `agent` (the default, so pairings stored before this read as Agent pairings) or `gateway`. A Gateway pairing has the same name, readable subgroups, own subgroup, create right and token hash. It has no OAuth client, and Agent access decisions don't apply to it.
- **`GatewaySession`**, in the `agent` crate beside `AgentSession`, built for one Gateway pairing over a `Workspace`, with the same audit sink and clock. It handles `whoami`, `list`, `resolve`, `create_login` and `update_login`. Visibility (the Agent group, the pairing's subgroups, notes never exposed) is shared with `AgentSession`, not duplicated.
- **The Entry mapping** is a pure function from an Entry to an optional gateway item, in the `agent` crate:
  - **login**: an `https` URL and a non-empty password. Its websites are the URL and every `KP2A_URL*` field that is `https`, each `{ url, match: "exact" }`. Its username is the Entry's.
  - **key**: tagged `key` (case-insensitive), or no URL and a title matching `^[A-Z][A-Z0-9_]*$`. `keyName` is the title, which must match that pattern; a `key`-tagged Entry whose title doesn't is left out.
  - A login takes precedence over a key when both match.
  - Every item has `ref` (the UUID, hyphenated lower case), `kind`, `title`, and `updatedAt` (the Entry's modification time, ISO 8601). A login has `username` and `websites`; a key has `keyName`. Nothing else.
- **The API**, served by `keefast-agent serve` under `/gateway/v1/`, JSON in and out, with `Authorization: Bearer <token>`:
  - `GET /whoami` → `{ vault, pairing, subgroups, canWrite }`.
  - `GET /items` → `{ items: [...] }`, with an `ETag` from the Vault's revision. `If-None-Match` with the current one answers `304`.
  - `POST /items/{ref}/resolve` with `{ field: "password" | "key", for?: { agent?, person?, run? } }` → `{ value }`. Both fields read the Entry's password. `for` is free text, length-capped, logged as the gateway's unverified claim.
  - `POST /logins` with `{ title, username?, password, website?, notes?, for? }` → `201` and the item. Created in `Agent group/<pairing name>`. A `website` must be `https`.
  - `PATCH /logins/{ref}` with `{ username?, password?, for? }` → the item. Only Entries in the pairing's own subgroup. The change goes through Entry history.
- **Errors** are `{ code, message }`, the message keefast's own and never containing a value:

  | Status | `code` | When |
  |---|---|---|
  | 400 | `invalid_input` | A malformed body, an `http` website, a missing password |
  | 401 | `unauthorized` | No token, an unknown or revoked one, or an Agent pairing's |
  | 403 | `read_only` | A write by a pairing without the create right |
  | 403 | `not_writable` | A change to an Entry outside the pairing's own subgroup |
  | 404 | `not_found` | An Entry gone, or outside the pairing's subgroups |
  | 423 | `locked` | The Headless host is locked |

- **Routing by kind.** `serve`'s token check returns the pairing and its kind. `/mcp` and the `run` and `store` endpoints refuse a Gateway pairing's token, and `/gateway/v1/` refuses an Agent pairing's, both as `401` with no hint that the token is valid elsewhere.
- **Audit.** Each call is a record, as for Agents: time, pairing, kind, Entry, action (`list`, `resolve`, `create_login`, `update_login`), outcome, and the `for` claim under a field named so that nobody reads it as verified (`claimed_for`). A list is one record, not one per Entry.
- **Pairing commands.** `keefast-agent pairing add --gateway <name> [--read <subgroup>...] [--write]` prints the token once. `pairing list` shows the kind. The desktop app's Vault settings → Agents lists Gateway pairings in their own section, with the same create flow and a Gateway toggle.
- **Docs.** A page on connecting agentes-chat: making the Vault and subgroup, the pairing, unattended unlock and what it gives up, and the URL and token agentes-chat's connect form asks for.

### The agentes-chat side

It lives in agentes-chat (`arthur-forks/agentes-chat`), not here, and is listed so the two sides agree:

- A Vault source module, registered through `AgentesModule.adapters()` → `vaultSources`, kind `keefast`, implementing `VaultSource`. `connect` asks for the host's URL and the token, calls `whoami`, and labels the connection with the Vault and subgroups. `list`, `reveal`, `createLogin` and `updateLogin` are thin calls to the API above. Every request goes through agentes-chat's guarded fetch (`publicHttpsFetch`), so the operator's host must be reachable over public HTTPS or a tailnet agentes-chat's server is on.
- Codes map onto `VaultSourceError`: `unauthorized` → `auth_failed`, `locked` → `locked`, `not_found` → `not_found`, `read_only` → `read_only`, `not_writable` → `not_writable`, `invalid_input` → `invalid_input`, and anything unreachable → `unreachable`.
- One engine change: a `not_writable` code, which, unlike `read_only`, doesn't mark the whole connection read-only. On it, agentes-chat keeps the new password in its own Vault and tells the agent the keefast Entry still has the old one, as it does for a source without `updateLogin`.
- A failed list leaves the indexed items in place, and a `locked` connection says how to unlock it.

## Testing Decisions

- **Good tests drive external behaviour**, as in the Agents spec: through the highest seam, asserting on what the gateway, the user or the log would see.
- **The Entry mapping** is tested as the pure function it is, with an Entry per rule: https and http URLs, `KP2A_URL*` fields, the `key` tag, `UPPER_SNAKE` titles, a `key`-tagged Entry with a bad title, an Entry matching both, an Entry matching neither, and notes and custom fields never appearing.
- **`GatewaySession`** is the main seam, with the same fakes as `AgentSession` (`FakeStorage`, `FakeClock`, an in-memory audit sink). It covers:
  - visibility limited to the pairing's subgroups, and notes never exposed;
  - resolve needing no decision and never consulting the Default agent policy;
  - create only in the own subgroup, change only there, `not_writable` elsewhere, `read_only` without the right;
  - Entry history recording a change;
  - a locked Vault;
  - audit records, with the `for` claim recorded as claimed, and a test that no value appears in any record;
  - the pairing kind surviving a save and a Merge, and a pairing without a kind reading as an Agent pairing.

  Prior art: the `AgentSession` tests.
- **`serve` in-process on loopback** with a plain HTTP client covers: tokens refused across kinds both ways, the `ETag` and `304`, `423` while locked, and each error's status and code. Prior art: the Agents spec's seam 2.
- **agentes-chat's side** runs agentes-chat's shared Vault source contract test against a fake keefast that speaks the API above.

## Out of Scope

- Gateway pairings on the desktop app, through its socket or agentes-chat's runner.
- Custom protected fields as keys of their own (`uuid#field`).
- Contacts, attachments, notes and TOTP seeds.
- Prompts or Agent access decisions for Gateway pairings.
- Verifying who a gateway says a resolve is for.
- Deleting or archiving anything through a gateway, and changing Entries outside the pairing's own subgroup.
- Gateways other than agentes-chat. The API is general, but nothing else is designed for or tested.

## Further Notes

- **Depends on** the Headless host (#153) and HTTP with bearer pairings (#155). The deployment the docs recommend uses the NixOS module (#154). Not Approvers (#157), OAuth (#158), or anything after them.
- **Design reference**: agentes-chat's ADR 0144, §3 (sources) and §9 (sources may write), and its `VaultSource` port, `packages/adapters/src/vault/source.ts`.
