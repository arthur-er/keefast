# Agents reach secrets through an MCP server and `keefast-agent run`, and the model rarely sees a value

keefast becomes a vault for AI Agents. An Agent connects to an MCP server that keefast serves, sees what secrets exist, and uses them without the value passing through the model wherever we can manage it. The server runs in the desktop app for Agents on the same machine, or as a Headless host on a server for Agents anywhere. The design borrows the central idea of agentes-chat's vault (its ADR 0144): agents get metadata and *uses* of a secret, not the secret, and every use is checked and logged. It differs where keefast already differs. There is no server-wide key: a Vault opens only with its Master key, and only a person can enter that.

The full spec, with user stories and testing decisions, is [docs/specs/agents.md](../specs/agents.md).

## Where it runs

- **One core, two hosts.** A new `agent` crate holds the MCP tools, Agent pairings, policy and the audit log, on top of `workspace`. One `keefast-agent` binary (built with `rmcp`, the official Rust MCP SDK) has the subcommands `mcp`, `run`, `store`, `serve`, `unlock`, `pairing` and `log`. It ships beside `keefast` and `keefast-native-host` in every package, the Windows MSI included, and never links Tauri or WebKit.
- **In the desktop app**, `keefast-agent mcp` is a stdio MCP server that relays to the running app over the native host's per-user socket or pipe, with the same peer checks (ADR 0004, update 2026-10-28). As with the Companion extension (ADR 0001), a locked or closed app answers "keefast is locked", and the Agent can't unlock it. Agent calls don't count as activity for auto-lock on idle, so an Agent loop can't hold a Vault open on an unattended machine.
- **On a Headless host**, `keefast-agent serve` opens one Vault through the usual Storage providers (local or Dropbox, with Merge) and serves Streamable-HTTP MCP. It listens on loopback by default. On any other address it refuses to start without TLS (rustls, with a certificate the operator provides). There is no unauthenticated mode. Tailscale and reverse proxies are documented ways to publish it, not requirements. A Dropbox token goes in the host's OS secret store as ADR 0005 says, and providing one on a server is the operator's job.
- **Unlocking a Headless host** takes a person by default: `keefast-agent unlock` reads the Master key from a terminal (usually over ssh) and passes it to `serve` over an admin unix socket that only the operator can open. Until then, every call answers "locked". Unlocking unattended, from a Key file or a password in the OS secret store, is an opt-in that warns it gives up protection at rest (ADR 0004) for that Vault. The Master key never travels through a web page or the MCP endpoint.
- **A NixOS module** (`services.keefast-agent`) runs `serve` as a system service under a user of its own, so Agents running on the same server as the operator's user can't read its memory or state. They have to come in through a pairing like any other Agent.
- **Any Vault can be served.** The docs recommend a Vault kept for Agents, with its own Master key, so a compromised server gives up only Agent secrets. Nothing in keefast depends on that choice.

## What Agents see, and who they are

- **The Agent group** is the only part of a Vault an Agent can see. Each Agent pairing writes to `Agent group/<pairing name>`, made when the pairing is, and may be given other subgroups to read. Renaming or deleting a pairing leaves its Entries in place.
- **Agent pairings live in the Vault's custom data** (ADR 0005), so they travel to every host that opens the Vault. A pairing is made in three ways:
  - in the desktop app;
  - with `keefast-agent pairing` on the host;
  - by an MCP client through OAuth (see below).
- **Bearer tokens** are random. Each is shown once and stored only as a hash. Claude Code, Codex and the `keefast-agent` command line use them, from `KEEFAST_AGENT_TOKEN` (and `KEEFAST_AGENT_URL` for a remote host).
- **OAuth 2.1, as the MCP authorization spec describes it**, is there for clients that accept only OAuth or no sign-in, such as Executor or claude.ai connectors. keefast is its own authorization server, with protected-resource and authorization-server metadata, Dynamic Client Registration and PKCE. Its consent page shows a short code. The user approves it, and names the pairing and its subgroups and rights, through an Approver or with `keefast-agent pairing approve <code>` on the host. Nobody signs in on the page, and no Master key is typed there. Access and refresh tokens rotate, so they live hashed in the host's own state, not in the Vault. Revoking a pairing in the Vault kills its tokens once the host has the change.
- **A gateway is one pairing.** keefast can't verify which agent behind Executor is asking, so it doesn't pretend to. Prompts and the log name the pairing.

## How a secret is used

The tools, in order of preference:

- **`list_secrets`** returns metadata for the Entries a pairing can read: title, username, URL, tags, field names, Secret references, and a hint of the last four characters. Text an Agent didn't write comes wrapped as untrusted content. Notes are left out: they are where prompt injection would hide, and they often hold secrets themselves.
- **`keefast-agent run --env NAME=<Secret reference> -- <program> …`** is how an Agent with a shell uses a secret. The command line asks keefast for the values, puts them in the program's environment, and redacts them from its output. The value never goes through MCP or the model. This is the way the MCP server's instructions teach.
- **`http_request`** is how an Agent with only MCP uses a secret. keefast itself makes the HTTP request, putting values into `{{secret:<ref>}}` placeholders in the headers, the body or basic auth, and redacts them from the response. Each Entry lists the hosts its secrets may be sent to, so a secret only ever reaches its own API. Requests to private and loopback addresses are refused unless the Entry allows them by name, so the Headless host can't be turned on its own network.
- **Creating**: `create_entry` makes an Entry in the pairing's own subgroup, and can ask keefast to generate its password, which is never returned. `keefast-agent store <ref>#<field>` reads a value from stdin, so `mint-token | keefast-agent store …` saves a secret no model has seen. Agents can't edit or delete Entries. That stays with people, with Entry history as usual.
- **`reveal_secret`** returns a value into the model's context, for the rare secret that must end up in a file. It works only on Entries marked "Agents may reveal", asks every time, and is never remembered.

Redaction is a guard against accidents, such as a token printed into a transcript. It doesn't stop an Agent set on leaking: `sh -c 'echo $T | base64'` gets around it. What stops that is the approval, which names the program.

## Approvals

- **Agent access decisions** are remembered per Agent pairing, Entry, and program (for `run`) or host (for `http_request`). Approving `gh` with a GitHub token doesn't approve `curl`, and shells and interpreters (`sh`, `bash`, `pwsh`, `python`, `node`, …) are never remembered. The **Default agent policy** applies when no decision fits. It starts at ask, as the browser's does, and lives in the Vault beside it.
- **A prompt** names the pairing, the Entry, and the full command line or request. If nobody answers within 60 seconds, it is denied.
- **On a Headless host**, prompts go to an **Approver**: a desktop keefast paired with the host, holding an SSE connection to it. With no Approver online, a request that would ask is denied. Decisions made at a prompt are saved into the Vault, so they reach every host that serves it. The first Approver is paired with `keefast-agent` on the host, since there's no Approver yet to approve it.

## Audit

Every call that touches a secret (list, use, create, reveal, and the decision) goes to an append-only log on the machine that served it. Each record has the time, the pairing, the Entry, the program or host, and the outcome, never the value. The log rotates by size. It is local rather than in the Vault because every use would otherwise be a save and a Merge. The desktop app shows its own log per Vault, and a Headless host's log through its Approver, as that host's Activity. `keefast-agent log` reads it on the host.

## Considered and rejected

- **keefast runs the command itself** (`run_with_secrets` as an MCP tool, as agentes-chat's runner does). That would make a password manager a remote command executor. `keefast-agent run` does the same job in the Agent's own process, with the Agent's own rights.
- **A proxy that intercepts TLS** to put secrets into any request. That means a certificate authority to trust and every request to parse. `http_request` covers the APIs an Agent calls by name.
- **Plain `get_secret` as the main tool.** The value would then sit in transcripts and the model provider's logs, and a prompt-injected Agent could send it anywhere.
- **Streamable HTTP on localhost for the desktop app.** Any local user, and any web page through DNS rebinding, could reach it. The socket the native host already secured can't be reached that way.
- **A Dropbox token or Master key outside the OS secret store** on servers, for example in systemd credentials, as a keefast mechanism. Hosts that need it can provide a secret store.
- **Prompting on a Headless host without an Approver** (treating ask as deny with no way to answer). It was the first design. With decisions per program it would have made every new use fail until the user found the Entry on a desktop.

## Consequences

- **ADR 0004 gains attackers**: a connected Agent that is prompt-injected or misbehaves, and, for a Headless host, anyone who can reach its address. See its update of 2026-11-16.
- **ADR 0005's Vault custom data** gains Agent pairings, Agent access decisions, the Default agent policy, each Entry's allowed hosts, and the "Agents may reveal" flag. OAuth tokens are the exception and stay in host state. See its update of 2026-11-16.
- **ADR 0008 said secrets cross only when shown.** That still holds for the UI. Agents are a second way out, and approvals stand in for showing: no value leaves for an Agent without an Agent access decision or the Default agent policy allowing it.
- **The Windows and Linux packages** grow a binary, and the AppImage copies `keefast-agent` out as it does the native host.
