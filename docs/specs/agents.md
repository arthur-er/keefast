# Spec: Agents — keefast as an MCP vault for agent secrets

Decisions: [ADR 0012](../adr/0012-agents-reach-secrets-through-mcp.md), with updates to [ADR 0004](../adr/0004-threat-model.md) and [ADR 0005](../adr/0005-browser-settings-live-in-the-vault.md). Terms: the Agents section of [GLOSSARY.md](../../GLOSSARY.md). Tracking issue: [#152](https://github.com/arthur-er/keefast/issues/152). Gateway pairings, for vaults like agentes-chat's that resolve values: [ADR 0015](../adr/0015-gateways-resolve-values-for-another-vault.md) and [gateways.md](gateways.md).

## Problem Statement

AI agents I run (Claude Code, Codex, agents behind a gateway such as Executor) need API keys, tokens and passwords to do their work. Today I paste these into `.env` files, shell profiles or MCP configs, or straight into a chat. That leaves them on disk in plain text, in transcripts, and in the model provider's logs. A prompt-injected agent can read them and send them anywhere, and I can't see which agent used which secret.

My secrets already live in KeePass Vaults that I trust, but those Vaults reach only me and my browser. There is no safe way to let an agent use a secret, especially an agent on another machine than the one where I unlock keefast, and no way to keep a small set of secrets available to agents on an always-on server.

## Solution

keefast serves an MCP server that Agents connect to like any other. Agents see only a Vault's **Agent group**, and only what their **Agent pairing** reaches. They see metadata about secrets, and they *use* secrets in ways that mostly keep the value away from the model:

- **Agents with a shell** run `keefast-agent run --env NAME=<Secret reference> -- <program>`, which puts the value in that program's environment and redacts it from the output.
- **Agents with only MCP** call `http_request`, and keefast makes the HTTP call itself, sending the secret only to that Entry's allowed hosts.
- **Agents create secrets** they mint without seeing them (`create_entry` with a generated password, or `keefast-agent store` from stdin).
- **Revealing** a value to the model is possible only for Entries I mark "Agents may reveal", and it asks me every time.

Every use is checked against **Agent access decisions**, remembered per pairing, Entry, and program or host, and falls back to the **Default agent policy** (ask). Every use is logged, without the value.

The server runs in two places:
- **On a Headless host** (`keefast-agent serve`), a server that I unlock once after it starts. Agents anywhere reach it over HTTPS. A desktop keefast paired with it as an **Approver** shows its prompts and its activity.
- **In the desktop app**, for Agents on the same machine, over the same protected socket the browser integration uses.

Any MCP client can pair, with a bearer token or with MCP's own OAuth 2.1 flow, which I approve with a short code.

## User Stories

### Setting up a Headless host

1. As a keefast user, I want to run keefast without a window on a server, so that my Agents can use secrets while my laptop is closed.
2. As an operator, I want the Headless host to start locked and stay locked until I unlock it, so that a stolen disk or a reboot doesn't hand my secrets to anyone.
3. As an operator, I want to unlock the Headless host over ssh with a command that reads my Master key from the terminal, so that the Master key never travels through a web page or the MCP endpoint.
4. As an operator, I want to unlock with a Key file, a password, or both, so that I can use the same Master key as in my other KeePass clients.
5. As an operator, I want unlocking to tell a wrong Master key from a damaged Vault file, so that I know what to fix.
6. As an operator, I want to lock the Headless host and check its status from the command line, so that I can shut agents out at once.
7. As an operator who accepts the risk, I want to opt in to unlocking unattended from a Key file or the OS secret store, with a clear warning, so that the host recovers by itself after a reboot.
8. As an operator, I want the Headless host to open a Vault file from local disk or Dropbox, so that I can serve the same Vault my desktop uses.
9. As an operator, I want the Headless host to Merge changes made to its Vault file elsewhere, so that edits from my desktop and from Agents are never lost.
10. As an operator, I want to serve a Vault kept only for Agents, so that a compromised server gives up only Agent secrets.
11. As an operator, I want the Headless host to listen on loopback by default, so that nothing is exposed until I choose to expose it.
12. As an operator, I want the Headless host to refuse to listen beyond loopback without TLS, so that I can't expose secrets over plain HTTP by mistake.
13. As an operator, I want to give the Headless host my own TLS certificate, so that it works without Tailscale or a reverse proxy.
14. As an operator, I want documentation for putting the Headless host behind Tailscale or a reverse proxy, so that I can use what my network already has.
15. As a NixOS user, I want a `services.keefast-agent` module, so that I can declare the Headless host in my configuration.
16. As an operator, I want the Headless host to run as a user of its own, so that Agents running on the same server as my user can't read its memory or files.
17. As an operator, I want the Headless host's Dropbox token kept in the OS secret store, so that it never sits in a plain file.

### Pairing Agents

18. As a user, I want to choose a Vault's Agent group, so that Agents can never see anything outside it.
19. As a user, I want to create an Agent pairing from the desktop app, so that I can give one Agent access without a terminal.
20. As an operator, I want to create, list and revoke Agent pairings with `keefast-agent pairing` on the host, so that I can manage a Headless host from ssh.
21. As a user, I want a new pairing to get its own subgroup in the Agent group, so that what each Agent creates stays separate.
22. As a user, I want to give a pairing read access to other subgroups, such as a shared one, so that several Agents can use the same secret without copies.
23. As a user, I want to decide whether a pairing may create Entries, so that read-only Agents stay read-only.
24. As a user, I want a pairing's token to be shown only once and kept only as a hash, so that reading the Vault file doesn't reveal working tokens.
25. As a user, I want pairings to live in the Vault, so that a pairing I make on my desktop works on every Headless host that serves that Vault.
26. As a user, I want to revoke a pairing and have its access stop at once, so that a leaked token or a misbehaving Agent is cut off.
27. As a user, I want renaming or deleting a pairing to leave the Entries it created in place, so that I don't lose secrets by cleaning up.
28. As a user of Executor or claude.ai connectors, I want to add keefast by its URL and sign in with OAuth, so that it works like any other MCP server.
29. As a user approving an OAuth client, I want the consent page to show a short code that I confirm on my desktop or on the host, so that I never type my Master key into a web page.
30. As a user approving an OAuth client, I want to name the pairing and choose its subgroups and rights when I approve, so that new clients get only what I intend.
31. As a user, I want OAuth tokens to stop working when I revoke their pairing in the Vault, so that revoking is one action wherever the client connected.
32. As a user of a gateway such as Executor, I want the gateway to count as one pairing, so that prompts and the log are honest about who keefast is talking to.

### Seeing secrets

33. As an Agent, I want to list the secrets my pairing can read, with title, username, URL, tags, field names and a hint, so that I can pick the right one.
34. As an Agent, I want each secret listed with a Secret reference by UUID and by path, so that I can name it reliably and readably.
35. As a user, I want Agents never to see an Entry's notes, so that text planted in notes can't steer them and secrets kept in notes stay put.
36. As a user, I want text from Entries marked as untrusted when an Agent sees it, so that a malicious title is less likely to be taken as an instruction.
37. As an Agent, I want the MCP server's instructions to explain Secret references and `keefast-agent run`, so that I use secrets the safe way without being told.
38. As an Agent, I want a clear error when the Vault is locked, so that I can tell my user to unlock it instead of retrying.

### Using secrets in commands

39. As an Agent with a shell, I want to run a program with secrets in its environment, so that tools like `gh`, `aws` or `psql` work without the secret passing through me.
40. As an Agent, I want to name several secrets in one `run`, so that a program needing a key and a URL gets both.
41. As an Agent, I want the program's exit code passed through, so that I can tell success from failure.
42. As a user, I want secret values redacted from the program's output, so that a program printing its token doesn't put it in a transcript.
43. As a user, I want redaction to catch a value split across chunks of output, so that buffering doesn't defeat it.
44. As an Agent, I want a clear error for an unknown, ambiguous or unreachable Secret reference, so that I can correct it.
45. As an Agent on another machine, I want `keefast-agent run` to reach a Headless host by URL and token, so that I can use secrets on my laptop from a server.
46. As an Agent on Windows, I want `keefast-agent run` to work there as on Linux, so that my laptop's Agents can use secrets.

### Using secrets in HTTP calls

47. As an Agent with only MCP, I want to make an HTTP request with secrets filled into placeholders, so that I can call an API without ever seeing its key.
48. As a user, I want each Entry to list the hosts its secrets may be sent to, so that a GitHub token only ever goes to GitHub.
49. As a user, I want redirects checked against those hosts again, so that an allowed API can't bounce a secret elsewhere.
50. As an operator, I want requests to private, loopback and link-local addresses refused unless an Entry allows that host by name, so that Agents can't turn the Headless host against my network.
51. As a user, I want secrets redacted from the response, so that an API echoing a key doesn't hand it to the model.
52. As an operator, I want limits on request and response size and time, so that an Agent can't use the host to download or hang without end.

### Creating secrets

53. As an Agent, I want to create an Entry in my own subgroup, so that I can save credentials for something I set up.
54. As an Agent, I want keefast to generate a new Entry's password without returning it, so that a password I create is never in my context.
55. As an Agent, I want to pipe a minted token into `keefast-agent store`, so that I can save it without reading it.
56. As a user, I want Agents unable to edit or delete Entries they didn't create, so that an Agent can't destroy my secrets.
57. As a user, I want Agent changes recorded in Entry history, so that I can see and undo what they did.

### Revealing secrets

58. As a user, I want to mark an Entry "Agents may reveal", so that the rare secret that must go into a file can be handed over.
59. As a user, I want every reveal to ask me and never be remembered, so that a value enters a model's context only when I say so that time.
60. As an Agent, I want a reveal of an unmarked Entry refused without bothering my user, so that I know to use `run` or `http_request` instead.

### Approving

61. As a user, I want to be asked the first time a pairing uses an Entry with a program or host, so that nothing uses my secrets without my say.
62. As a user, I want the prompt to name the pairing, the Entry, and the full command line or request, so that I can judge what will happen.
63. As a user, I want to allow or deny once or for good, so that routine uses stop asking.
64. As a user, I want approving `gh` not to approve `curl`, so that an Agent can't reuse my approval for something else.
65. As a user, I want shells and interpreters to ask every time, so that "always allow" can't be turned into "run anything".
66. As a user, I want a per-Vault Default agent policy of ask, always allow or always deny, so that I can set how strict a Vault is.
67. As a user, I want an unanswered prompt denied after 60 seconds, so that a request doesn't wait forever or succeed while I'm away.
68. As a user, I want to see and change remembered Agent access decisions in the Entry editor, so that I can withdraw one.
69. As a user, I want to pair my desktop keefast with a Headless host as an Approver, so that its prompts reach me.
70. As a user, I want a Headless host to deny at once when no Approver is online, so that its Agents fail fast instead of hanging.
71. As a user with several desktops, I want the first answer from any Approver to win and the others to dismiss the prompt, so that I answer once.
72. As a user, I want decisions I remember on one Approver to reach every host serving that Vault, so that I approve once per use.

### Auditing

73. As a user, I want every list, use, create and reveal logged with time, pairing, Entry, program or host, and outcome, so that I can see what each Agent did.
74. As a user, I want the log never to contain a secret value, so that the log isn't a new place to steal from.
75. As a user, I want each Headless host's activity in my desktop app, so that I can review it without ssh.
76. As an operator, I want to read the log with `keefast-agent log` on the host, so that I can investigate from a terminal.
77. As an operator, I want the log rotated by size, with retention I can set, so that it doesn't fill the disk.

### Agents on the desktop's machine

78. As a user, I want local Agents to use my unlocked desktop keefast through a stdio MCP server, so that I need no Headless host for local work.
79. As a user, I want local Agents to get "keefast is locked" when the app is locked or closed, and not be able to unlock it, so that locking still means locked.
80. As a user, I want Agent activity not to keep my desktop Vault unlocked, so that auto-lock on idle still protects an unattended machine.
81. As a user, I want local prompts to show in the desktop app itself, so that approving works the same as for a Headless host.

### Packaging

82. As a Linux or Windows user, I want `keefast-agent` shipped with keefast in every package, so that I don't install it separately.
83. As an AppImage user, I want `keefast-agent` copied somewhere stable, as the native host is, so that Agent configs keep working between runs.

## Implementation Decisions

- **New `agent` crate** on top of `workspace`. It owns Agent pairings, the Agent group, Secret references, Agent access decisions and the Default agent policy, the tools, approvals and the audit log. `core` stays free of I/O. `workspace` stays the only owner of decrypted Vaults, and `agent` reaches Vaults only through it.
- **`AgentSession`** is the single entry point, shaped like `BrowserSession`. It is built for one Agent pairing and handles requests against a `Workspace`. It is given its collaborators: an `Approver` (prompt → allow/deny, once or remembered, or timeout), an HTTP transport, an audit sink, and the workspace's clock. Every host, whether `serve`, the in-app relay or the CLI's remote calls, translates its transport into `AgentSession` requests and back.
- **Requests the session handles**: `list_secrets`, `use_secrets` (for `run`: references plus program and arguments → values), `http_request`, `create_entry`, `store`, and `reveal_secret`.
- **Agent settings in the Vault's custom data** (ADR 0005 update). Stored there:
  - Agent pairings: id, name, readable subgroups, create right, token hash or OAuth client id.
  - The Agent group.
  - Agent access decisions on each Entry, keyed by pairing and program or host.
  - The Default agent policy.
  - Each Entry's allowed hosts and its "Agents may reveal" flag.

  Items are timed so Merge keeps the newest, following the tag-colour precedent. Unknown keys survive other KeePass clients (ADR 0002).
- **Secret reference format**: an Entry UUID or a path relative to the Agent group, followed by `#<field>` (`password`, `username`, `url`, or a custom field name). A path matching two Entries is an error.
- **Shell and interpreter list**: a fixed list (`sh`, `bash`, `zsh`, `fish`, `dash`, `pwsh`, `powershell`, `cmd`, `python*`, `node`, `deno`, `bun`, `ruby`, `perl`, `php`, `env`, `xargs`, …), matched on the executable's basename. Decisions for these are never remembered.
- **`keefast-agent` binary** with subcommands `mcp`, `run`, `store`, `serve`, `unlock`, `lock`, `status`, `pairing`, `log`. It is built on `rmcp` and never links Tauri or WebKit.
- **`serve`**:
  - **Transport:** Streamable-HTTP MCP, with rustls TLS required for any non-loopback address. It also serves the endpoints `run` and `store` use, and an SSE stream for Approvers.
  - **Admin socket:** for `unlock`, `lock`, `status`, `pairing` and `log`. It is a unix socket readable only by the service user and an operator group.
  - **Host state:** OAuth clients and hashed tokens, Approver credentials, and the audit log, in its own state directory.
- **Authorization**: bearer tokens (hashed in the Vault), plus the MCP authorization spec's OAuth 2.1. keefast is its own authorization server, with protected-resource and authorization-server metadata, Dynamic Client Registration and PKCE (S256). Consent works by a short code approved through an Approver or `keefast-agent pairing approve`.
- **Approver protocol**: a desktop keefast holds an authenticated SSE stream per host and receives prompts, and answers them over HTTP. The first answer wins. Remembered answers become Agent access decisions saved into the Vault, and the host picks them up through Merge or directly.
- **`run` and `store` clients** read `KEEFAST_AGENT_URL` and `KEEFAST_AGENT_TOKEN`. Without a URL, they reach the local desktop app over the native host's socket or pipe.
- **Redaction** is a streaming filter that holds back enough bytes to match a value spanning chunk boundaries. It is best effort, as ADR 0012 says.
- **`http_request` safety**:
  - **Hosts:** checked against the Entry's allowed hosts after placeholder expansion and on every redirect.
  - **Private addresses:** DNS-resolved addresses are checked against private, loopback and link-local ranges, unless the host is allowed by name.
  - **Limits:** size and time limits.
- **In-app host**: `keefast-agent mcp` relays to the desktop app like `keefast-native-host`, with the same peer checks. The app runs an `AgentSession` whose `Approver` is the app's own prompt. Agent calls don't reset the idle timer.
- **Desktop UI**:
  - Vault settings → Agents: the Agent group, the Default agent policy, and pairings.
  - The Entry editor's Agents section: decisions, allowed hosts, and "Agents may reveal".
  - Settings → Agent hosts: Approver pairing and the per-host Activity view.
  - The approval prompt.
- **NixOS module** `services.keefast-agent`: a system service with a dedicated user, a state directory and hardening. It has options for the Vault, the listen address, the TLS files, the admin group, unattended unlock, and log retention.

## Testing Decisions

- **Good tests drive external behaviour** through the highest seam available, and assert on what an Agent, the user or the log would see. They don't assert on internal structures. Fakes stand in only at process and network boundaries.
- **Seam 1, `AgentSession`, the main seam.** It runs against a `Workspace` with `FakeStorage` and `FakeClock`, a scripted `FakeApprover`, a fake HTTP transport and an in-memory audit sink. It covers:
  - visibility limited to the Agent group and the pairing's subgroups;
  - notes never exposed;
  - Secret reference resolution and its errors;
  - decisions per pairing, Entry and program or host, and the Default agent policy;
  - shells and interpreters always asking, and the 60 s timeout;
  - create rights, and that edits and deletes outside the pairing's subgroup are refused;
  - the "Agents may reveal" check;
  - allowed hosts, redirects, and private-address blocking;
  - audit records, including a test that no secret value ever appears in a record;
  - a locked Vault;
  - Agent settings round-tripping through a Vault save and Merge.

  Prior art: `BrowserSession` tests with `FakeExtension` in the `workspace` crate's tests, and the `core` crate's round-trip and Merge tests.
- **Seam 2, `serve` in-process on loopback**, with a real `rmcp` client. It covers:
  - bearer auth and revocation;
  - the OAuth 2.1 flow end to end with consent-code approval;
  - refusing a non-loopback address without TLS;
  - `unlock`, `lock` and `status` over the admin socket;
  - the Approver SSE stream, including the first answer winning.
- **Seam 3, `keefast-agent run` as a process** against a fake host. It covers:
  - values reaching the child's environment;
  - redaction, including values split across chunks;
  - exit codes passed through.

  It runs on Linux and Windows. Prior art: the native host's relay tests.
- **The in-app relay** reuses the native host's socket-pair test approach and adds no new seam.
- **A NixOS VM test** for the module boots the service, unlocks it over the admin socket, and checks it is locked again after a restart.

## Out of Scope

- Agents editing or deleting Entries outside their own subgroup, or deleting at all.
- A proxy that intercepts TLS to put secrets into arbitrary requests.
- keefast running commands on an Agent's behalf (a `run_with_secrets` MCP tool).
- Sending approval prompts to phones or anything other than a desktop keefast Approver.
- Unlocking a Headless host from the desktop or any web page.
- Verifying the identity of individual agents behind a gateway.
- Handing values to another program's vault. That is a Gateway pairing's job, specced in [gateways.md](gateways.md).
- Secret leases, expiry or automatic rotation.
- Any unauthenticated mode.
- Exposing notes or attachments to Agents.
- A keefast-specific secret store for servers without an OS secret store (such as systemd credentials).

## Further Notes

- **Slices**: the work is split into eleven slices tracked as sub-issues of #152 (#153–#163), each blocked by the one before. The Headless host comes first, and the in-app host comes last.
- **Before Approvers land (#157)**, *ask* on a Headless host means deny, and uses are allowed ahead of time in the desktop app.
- **Design reference**: agentes-chat's vault (its ADR 0144). keefast keeps that vault's rule that the model rarely sees a value, but doesn't adopt its server-wide encryption key. A Vault opens only with its Master key. keefast is also one of that vault's sources, through a Gateway pairing ([gateways.md](gateways.md)).
