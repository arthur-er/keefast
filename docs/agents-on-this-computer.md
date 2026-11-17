# Agents on this computer

An Agent running on the same machine as the keefast desktop app, such as Claude Code or Codex,
reaches the Vaults you have unlocked there through `keefast-agent` (ADR 0012). Nothing listens
on the network: `keefast-agent` talks to the app over a socket in your runtime directory
(`$XDG_RUNTIME_DIR/keefast/agent.sock`), or a named pipe on Windows, that only your user can
open.

1. In Vault settings → Agents, choose the Agent group and pair the Agent. Copy its token.
2. Add keefast to the Agent as an MCP server that runs `keefast-agent mcp` with the token:

   ```sh
   claude mcp add keefast --env KEEFAST_AGENT_TOKEN=kfa_... -- keefast-agent mcp
   ```

   From an AppImage, the command is `~/.local/share/keefast/keefast-agent`. The MSI puts
   `keefast-agent` on your `PATH`.
3. Give the Agent's shell the same `KEEFAST_AGENT_TOKEN`, without `KEEFAST_AGENT_URL`, so
   `keefast-agent run` and `keefast-agent store` reach the app too.

The tools are those of a Headless host ([Running a Headless host](headless-host.md)):
`list_secrets`, `http_request`, `create_entry` and `reveal_secret`. While the app is closed,
or the Vault is locked, every call answers that keefast is locked; the Agent can't unlock it.
Agent calls don't count as your activity, so the Vault still locks when you are idle.

Prompts appear in the app, as coming from "this computer". What Agents asked for, and whether
they got it, is under Vault settings → Agents → Activity…: each Vault's own log, kept in
keefast's data directory, never with a value.
