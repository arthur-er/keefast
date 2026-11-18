# A Gateway pairing resolves values for another vault, such as agentes-chat's, through its own API on the Headless host

agentes-chat keeps logins and keys for its agents in its own Vault (its ADR 0144), and reads others from connected password managers (1Password, Bitwarden) through Vault sources. keefast should be one of those sources, so a workspace whose secrets live in KeePass doesn't copy them into agentes-chat. A Vault source lists metadata and, at each use, *resolves* one value: agentes-chat's runner types a login into a browser or puts a key in a command's environment, and keeps the value away from its model. ADR 0012 gives Agents no such use. `run` and `http_request` keep the value inside keefast, and `reveal_secret` asks every time and only for Entries marked "Agents may reveal", so an agentes-chat routine at 3am could never sign in to anything.

So keefast gains a second kind of pairing, the **Gateway pairing**, for another program's vault that resolves values on its agents' behalf. ADR 0012's rule that the model rarely sees a value still holds, but at a gateway it is that gateway's promise, not keefast's: keefast hands it the value, and trusts it, as it trusts a person's browser, to keep the value away from its model. The full spec is [docs/specs/gateways.md](../specs/gateways.md).

## Decisions

- **Headless host only.** A Gateway pairing works only with `keefast-agent serve`. A gateway is a server polling and resolving at any hour. The desktop app answers "locked" whenever its user is away, and its socket can't be reached from another machine.
- **Its own API, not MCP.** `serve` adds a small JSON API under `/gateway/v1/`: list items, resolve one field, create a login, change a login. A pairing is either an Agent pairing, accepted only at `/mcp` and the `run` and `store` endpoints, or a Gateway pairing, accepted only at `/gateway/v1/`. A token is refused at the other. No tool that returns a raw value ever appears in an MCP tool list, so a mis-set pairing can't hand one to a model.
- **Made by a person, by hand.** A Gateway pairing is made in the desktop app's Vault settings or with `keefast-agent pairing add --gateway` on the host. The OAuth flow (ADR 0012) only ever makes Agent pairings.
- **Its subgroups are the approval.** A Gateway pairing reads the Agent group subgroups it was given, as an Agent pairing does, and resolves any value in them without a prompt, an Agent access decision or the Default agent policy. Putting an Entry in one of those subgroups *is* the decision. A prompt would land at 3am, with no Approver online, on a routine that then fails for no reason anyone can see. The gateway already checks, at every use, which of its agents and people may use an item. keefast leaves that check to the gateway, and the operator limits the damage by what goes in the subgroups.
- **Every resolve is logged.** Each list, resolve, create and change goes to the audit log like an Agent's use, never with the value. The gateway sends who it is resolving for (its agent, its person, its run), and keefast records that as the gateway's own claim, marked unverified, as ADR 0012 says keefast can't verify who sits behind a gateway.
- **keefast maps Entries to the gateway's items.** The list answers in the gateway's terms, so the rules live and are tested in keefast and the gateway's client stays thin:
  - **Login**: an Entry with a password and either an `https://` URL or the tag `login`, which a login a gateway creates without a website carries. Its websites are that URL and KeePassXC's `KP2A_URL*` fields, `https` only, matched exactly.
  - **Key**: an Entry with a password, tagged `key` or with no URL, and a title that is already `UPPER_SNAKE`. The title is the key's name, and the password field its value.
  - **Ref**: the Entry's UUID, which a rename, a move or a Merge keeps.
  - Everything else, notes and custom fields included, is left out.
- **Writes stay in its own subgroup.** A Gateway pairing with the create right may create a login, with a password it supplies, in `Agent group/<pairing name>`, and change the username or password of Entries in that subgroup, each change kept in Entry history. Anywhere else a change is refused as *not writable*, which the gateway can tell apart from a pairing that may not write at all. Nothing is deleted.
- **Locked means 423.** While the Headless host is locked, every gateway route answers `423 Locked`, so the gateway can say "keefast is locked" rather than "keefast is broken" or "the item is gone". agentes-chat files it under its own `unreachable`, which retries by itself, since its `locked` means a wrong master password and waits for someone to reconnect.
- **Listing is cheap when nothing changed.** The list carries the Vault's revision as an `ETag`, and a gateway polling every few minutes gets `304` until the Vault changes.

## Considered and rejected

- **`reveal_secret` as the gateway's resolve.** Only marked Entries, and a prompt every time. Unusable for unattended agents.
- **A `resolve_secret` MCP tool, hidden from tool lists unless the pairing is a gateway.** Less code, but the only thing between a model and a raw-value tool would be one condition in the tool list.
- **Asking the first time a gateway resolves each Entry, then remembering it.** The first resolve is usually unattended, so it would be denied, and the gateway can only report that as a vague failure. It would also make the connector wait for Approvers (#157).
- **Keeping keefast pure: metadata only, with uses through `http_request` or `keefast-agent run`.** The gateway would need a second way to use a secret, around its own runner and checks, and still couldn't type a login into a browser.
- **The desktop app as a gateway's keefast, through agentes-chat's runner on the same machine.** It needs a new protocol between agentes-chat's runner and its server, works only while the desktop is unlocked, and makes keefast the one source agentes-chat must treat differently.
- **One item per custom protected field**, so one Entry could give several keys. Useful for an API with a key pair, but the ref becomes `uuid#field` and renaming a field quietly renames the key. Left for later.

## Consequences

- **ADR 0012's promise narrows.** "The model rarely sees a value" now reads: an Agent pairing rarely gets a value, and a Gateway pairing gets the values in its subgroups and keeps them from its own model. Recommended setups give a gateway a subgroup kept for it, in a Vault kept for Agents.
- **ADR 0004 gains an attacker**: a compromised gateway, or anyone holding its token, gets every value in its subgroups, without a prompt. See its update of 2026-11-17.
- **ADR 0005's Agent pairings** gain a kind, `agent` or `gateway`, and Gateway pairings are stored beside Agent pairings. See its update of 2026-11-17.
- **A gateway needs its Headless host unlocked to work at all.** For agentes-chat the docs recommend a Vault kept for Agents, unlocked unattended from a Key file and run by the NixOS module (#154), and say plainly what that gives up (ADR 0004).
- **agentes-chat's side** is a Vault source module in agentes-chat, outside keefast. It needs one change to agentes-chat's engine: telling a write refused for one item (`not_writable`) from a credential that may only read (`read_only`), which agentes-chat records for the whole connection.
