# Connecting agentes-chat

agentes-chat keeps logins and keys for its agents in its own Vault, and can read more from
password managers it connects to. keefast is one of them: agentes-chat connects to a
[Headless host](headless-host.md) through a **Gateway pairing**, lists the logins and keys in the
subgroups that pairing reads, and **resolves** a value, without a prompt, each time one of its
agents uses one (ADR 0015, [the Gateways spec](specs/gateways.md)). agentes-chat then types the
value into a browser or puts it in a command's environment, and keeps it from its model: that is
agentes-chat's promise, not something keefast can check.

A Gateway pairing works only with a Headless host (`keefast-agent serve`), never with the
desktop app: agentes-chat polls and resolves at any hour, and the desktop is locked whenever you
are away.

## A Vault kept for Agents

Whoever holds a Gateway pairing's token gets every value in its subgroups, without a prompt
(ADR 0004, update of 2026-11-17). So keep what agentes-chat may use apart from everything else:

1. **A Vault kept for Agents**, not your own Vault. The host will unlock it unattended (below),
   which leaves it unprotected at rest on the server. Copy into it only the logins and keys your
   agents need.
2. **An Agent group** in that Vault, such as `Agents`: Agents and gateways see nothing outside
   it. Choose it in the desktop app's Vault settings → Agents, or with `--agent-group` the first
   time you pair (below).
3. **A subgroup kept for agentes-chat**, `Agents/agentes-chat`, which pairing it makes. Put the
   logins and keys agentes-chat may use there. Its other subgroups belong to other pairings and
   it doesn't see them, unless you let it read one with `--read`.

To unlock unattended, the Vault's Master key must be a Key file alone, with no master password.
Make the Vault that way in the desktop app (New Vault, with a Key file and no password), or change
an existing Vault's Master key to it.

## Serving it

Serve the Vault from a server that is always on. On NixOS, the flake's module does it (see
[On NixOS](headless-host.md#on-nixos) for every option):

```nix
services.keefast-agent = {
  enable = true;
  vault = "/var/lib/keefast-agent/agents.kdbx";
  adminGroup = "keefast-operators";
  unattendedUnlock.keyFile = "/etc/keefast/agents.keyx";
};
```

Elsewhere, run the same thing as a service of your own, under a user kept for it:

```sh
keefast-agent serve --vault /srv/agents.kdbx --unattended-key-file /etc/keefast/agents.keyx
```

The NixOS module serves a local Vault file, not a Dropbox one: keep the copy you edit on your
desktop in step with it by a sync of your own. The host Merges changes made to its Vault file
elsewhere, so Entries you add there reach agentes-chat on its next list.

### What unattended unlock gives up

Without it, a reboot or a restart leaves the host locked until someone runs `keefast-agent
unlock`, and agentes-chat can't use anything until then. With it, the host unlocks by itself at
startup, and agentes-chat keeps working through reboots. The price (ADR 0004):

- **The Vault loses its protection at rest.** Whoever can read the Key file can open the Vault:
  root on the server, a backup of the server's disk, anyone the Key file was copied to. The host
  warns about this each time it starts. The module hands the Key file to the service as a
  systemd credential, so it may be readable by root alone; give its path as a string, not a Nix
  path, so it stays out of the Nix store. Don't keep the Key file beside the Vault file, or in
  whatever syncs the Vault file.
- **A stolen token works at any hour.** A Gateway pairing never prompts, and with the host
  always unlocked nothing stops a token used at 3am. Revoke it (below) and it stops at once.

That is why the Vault should be one kept for Agents: what it exposes is only what you chose to
give them.

## Reaching it from agentes-chat

agentes-chat only speaks HTTPS to keefast, and fetches through a guard that refuses loopback,
private and tailnet addresses, since the address is typed by a workspace member. So the host
needs an HTTPS address agentes-chat's server can reach, in one of two ways:

- **Public HTTPS.** Keep the host on loopback (the default, `127.0.0.1:7807`) behind a reverse
  proxy with a public certificate (Caddy, nginx, …), or listen on the network with built-in TLS
  (`--tls-cert` and `--tls-key`, or the module's `tls.certFile`, `tls.keyFile` and
  `openFirewall`). See [Where Agents reach it](headless-host.md#where-agents-reach-it).
  Anyone can then reach the address, but every gateway route needs a Gateway pairing's token.
- **A private address**, such as a tailnet, or keefast and agentes-chat on the same server.
  `tailscale serve --bg --https=443 http://127.0.0.1:7807` gives the host a tailnet name and a
  certificate, `https://<machine>.<tailnet>.ts.net`. That name resolves to a tailnet address,
  which agentes-chat refuses unless whoever runs agentes-chat's server lists the host name in
  its `VAULT_KEEFAST_PRIVATE_HOSTS` setting. Workspace members can't widen that list. A listed
  host is still reached over HTTPS only, so even on the same server put TLS in front of the
  host.

agentes-chat calls `<address>/gateway/v1/…`, so the address may carry a path, such as a proxy's
`https://example.com/keefast`. It never follows a redirect, so give it the address the host
answers on, and don't have your proxy redirect `/gateway/v1/` paths.

## Pairing agentes-chat

Make the Gateway pairing on the host, with the Vault unlocked:

```sh
keefast-agent pairing add --gateway agentes-chat --write
keefast-agent pairing list
keefast-agent pairing revoke agentes-chat     # its token stops working at once
```

`pairing add --gateway <name>` makes the subgroup `Agents/<name>` (or uses one already there) and
prints the token **once**: the Vault keeps only its hash. While the Vault has no Agent group yet,
add `--agent-group Agents` to make that Group the Agent group. `--read <subgroup>`, repeated as
needed, lets it read another subgroup of the Agent group too. Without `--write` the pairing only
reads. With it, agentes-chat
may create logins in its own subgroup and change the username or password of the ones there, each
change kept in Entry history; it never deletes anything, and never changes an Entry in a
subgroup it only reads. Revoking leaves the subgroup and its Entries where they are.

Or, in the desktop app, open Vault settings → **Gateways** → **Pair a Gateway…**, name it, tick
the subgroups it also reads and whether it **Creates and changes logins in its own subgroup**,
and copy the token it shows once. The pairing is saved in the Vault, and the host picks it up when
it Merges the Vault file. The Gateways section lists each Gateway pairing with what it reads and
whether it writes, and revokes it.

A Gateway pairing's token works only at `/gateway/v1/`, and an Agent pairing's never does: a
Gateway pairing can't be used as an MCP server, and an Agent can't resolve values.

In agentes-chat, connect a keefast vault source. Its form asks for:

- **keefast host address**: the HTTPS address above, such as `https://keefast.example.com`,
  without `/gateway/v1`;
- **Gateway pairing token**: the token `pairing add --gateway` printed.

agentes-chat checks the token and labels the connection with the Vault's name and the subgroups
the pairing reads. If it says the address is on a private or internal network, see the previous
section. If it says keefast didn't accept the token, the pairing was revoked or the token mistyped:
pair again.

## When the host is locked

While the host is locked, it answers every gateway call `423 Locked`, whatever the token: it keeps
token hashes in the Vault, so it can't check one until it is unlocked. agentes-chat shows the
connection as unreachable, with:

> keefast at keefast.example.com is locked. Someone who runs it unlocks it with
> `keefast-agent unlock`; it is read again by itself after that.

The logins and keys it already listed stay in its index, but using one fails until the host is
unlocked. agentes-chat tries again by itself, so nobody needs to reconnect. Unlock it on the
host, as a member of the module's `adminGroup` (or the host's own user):

```sh
keefast-agent unlock --key-file /etc/keefast/agents.keyx --no-password
keefast-agent status
```

With unattended unlock, restarting the service unlocks it too. The host is locked after a
`keefast-agent lock`, or a restart without unattended unlock.

## Which Entries agentes-chat sees

keefast lists the Entries in the pairing's subgroups, and the Groups below them, as agentes-chat's
logins and keys, by these rules:

- **A login** is an Entry with a non-empty password and an `https://` URL. Its websites are that
  URL and each of its `KP2A_URL*` fields (KeePassXC's additional URLs) that is `https`, matched
  exactly. Its username is the Entry's. An Entry tagged `login` with a password is a login too,
  even without an `https` URL; that is how a login agentes-chat creates without a website is kept.
- **A key** is an Entry with a non-empty password, tagged `key` or with no URL, whose title is
  `UPPER_SNAKE` (`GITHUB_TOKEN`, `OPENAI_API_KEY`): a capital letter, then capitals, digits and
  `_`. The title is the key's name in agentes-chat's commands, and the Entry's password is its
  value. A `key`-tagged Entry whose title isn't `UPPER_SNAKE` is left out.
- An Entry that is both is a login.
- **Left out**: every other Entry, such as one with only an `http://` URL, an empty password or
  a blank title; `http` websites; and every Entry's notes, custom fields, attachments and
  history. A value always comes from the password field, never a custom field.

Each item's ref is its Entry's UUID, so renaming or moving an Entry within the subgroups keeps
it the same item in agentes-chat. Moving it out of them makes it missing there.

## What is logged

Each list, resolve, created login and changed login is a record in the host's audit log, with
the time, the pairing (marked as a gateway), the action (`list`, `resolve`, `create_login`,
`update_login`), the Entry and the outcome, never a value. A create or change keefast refuses is
logged as denied, even one whose body it couldn't read. A list is one record, not one per
Entry. Read it on the host:

```sh
keefast-agent log              # the last 50 records
keefast-agent log --lines 500
```

```text
2026-11-17T03:00:12Z  agentes-chat (gateway)  resolve GitHub (4f0c…)  allowed
2026-11-17T03:00:12Z  agentes-chat (gateway)  resolve OPENAI_API_KEY (9a1e…) for agent research, run 812 (claimed, unverified)  allowed
2026-11-17T03:05:40Z  agentes-chat (gateway)  resolve  denied: not found
```

A gateway may say whom a resolve or change is for: its agent, its person, its run. The log
keeps that as **claimed, unverified**: it is what agentes-chat said, which keefast has no way to
check. A record without it names only the pairing and the Entry; agentes-chat's keefast source
doesn't send a claim yet. A resolve denied as not found asked for an Entry outside the pairing's
subgroups, or one that is no longer a login or key.

In keefast on a desktop paired as an [Approver](headless-host.md#approvers), the same log is
under Settings → Agent hosts → **Activity…**, where Gateway pairings show as `<name> (Gateway)`,
each record says what was resolved, created or changed and for whom it was claimed, and the list
filters by pairing, Entry and outcome.
