# Running a Headless host

A Headless host is keefast with no window, serving one Vault to Agents from a server
(ADR 0012, [the Agents spec](specs/agents.md)). It is the `keefast-agent serve` command.

## Serving and unlocking

```sh
keefast-agent serve --vault /srv/agents.kdbx          # or --vault dropbox:/Vaults/agents.kdbx
```

Without NixOS, run it as a service of your own (a systemd unit, say) under a user kept for it,
who can read and write the Vault file and its directory. On NixOS, the module below does that.

The host starts **locked**, and stays locked until someone unlocks it with the Master key:

```sh
keefast-agent unlock                      # asks for the Master password
keefast-agent unlock --key-file ~/agents.keyx           # password and Key file
keefast-agent unlock --key-file ~/agents.keyx --no-password   # Key file alone
keefast-agent status
keefast-agent lock                        # saves unsaved changes, then locks
```

`unlock` reads the password from the terminal without echoing it (or a line of input when
it isn't a terminal), and reads the Key file as you, so the host's user needn't be able to read
it. It says whether the Master key is wrong or the Vault file is damaged.

These commands reach the host over its **admin socket**, a unix socket that only the host's
user can open (mode 0600). With `--admin-group <group>`, members of that group can open it too
(mode 0660), so an operator can unlock a host that runs as a user of its own. The socket is
`--admin-socket <path>` on every command, or `$KEEFAST_AGENT_ADMIN_SOCKET`, or else
`$XDG_RUNTIME_DIR/keefast/agent-admin.sock`.

## Where Agents reach it

Agents reach the host at `--listen`, an IP address and port: `127.0.0.1:7807` unless you say
otherwise. On loopback it needs no TLS, since only this machine can connect. Any other address
carries Agents' tokens across the network, so `serve` refuses to start there without TLS:

```sh
keefast-agent serve --vault /srv/agents.kdbx --listen '[::]:7807' \
    --tls-cert /etc/ssl/agents/fullchain.pem --tls-key /etc/ssl/agents/key.pem
```

Agents talk MCP (Streamable HTTP) to `/mcp` on that address, as
`http://127.0.0.1:7807/mcp`, or `https://…/mcp` with TLS. Every request needs an Agent pairing's
token as `Authorization: Bearer <token>`: one without, or with a token no pairing has (or a
revoked one), gets 401. While the Vault is locked, the host can't check tokens, since it keeps
their hashes in the Vault: every call then answers that keefast is locked, and Agents tell their
user to unlock it. The host doesn't check the `Host` header, so a proxy can forward its own name.

Three ways to publish the host:

- **Tailscale**: keep the host on loopback and let `tailscale serve` give it a tailnet name
  and a certificate: `tailscale serve --bg --https=443 http://127.0.0.1:7807`. Agents on your
  tailnet use `https://<machine>.<tailnet>.ts.net/mcp`.
- **A reverse proxy** (Caddy, nginx, …) on the same machine: keep the host on loopback and
  proxy `https://agents.example.com/` to `http://127.0.0.1:7807`. Don't buffer responses: the
  MCP endpoint streams them (SSE).
- **Built-in TLS**: listen on the network with `--tls-cert` and `--tls-key`, PEM files of a
  certificate chain and its private key, from ACME or your own CA.

## Pairing Agents

An Agent needs an **Agent pairing** to reach the host: a name, its own subgroup of the Vault's
Agent group, the other subgroups it may read, and a bearer token. Make one on the host, with the
Vault unlocked:

```sh
keefast-agent pairing create claude --agent-group Agents   # the first time: makes Agents the Agent group
keefast-agent pairing create codex --reads shared          # also reads the subgroup "shared"
keefast-agent pairing list
keefast-agent pairing revoke codex                         # its token stops working at once
```

`create` makes the subgroup `Agents/claude` (or uses one already there) and prints the token,
**once**: the Vault keeps only its hash. `--agent-group <path>` is needed only while the Vault
has no Agent group; it makes the Group at that path if it is missing. The desktop app does the
same under Vault settings → Agents. Revoking leaves the subgroup and its Entries where they are.

Give the token to the Agent. For Claude Code:

```sh
claude mcp add --transport http keefast https://agents.example.com/mcp \
    --header "Authorization: Bearer kfa_…"
```

The Agent then sees, through `list_secrets`, the Entries in its subgroup and the subgroups it
reads: titles, usernames, URLs, tags, field names and Secret references, and the last four
characters of a long password. Never values, never notes, and nothing outside the Agent group.

## Unattended unlock

By default a reboot leaves the host locked until someone unlocks it. To have it unlock by
itself at startup, opt in:

```sh
keefast-agent serve --vault /srv/agents.kdbx --unattended-key-file /etc/keefast/agents.keyx
keefast-agent serve --vault /srv/agents.kdbx --unattended-password
```

`--unattended-password` uses the password kept in the host user's OS secret store, which
`keefast-agent unlock --remember` puts there. **The Vault then loses its protection at rest:**
whoever can read that Key file or secret store can open it. The host warns about this each time
it starts. Serve a Vault kept only for Agents if you do this.

## Changes made elsewhere

Like the desktop app, the host Merges changes made to its Vault file elsewhere: a local Vault
file is watched, and a Dropbox one checked every 30 seconds. A Dropbox Vault file needs
`KEEFAST_DROPBOX_APP_KEY`, and a Dropbox account connected for the host's user; connecting one
from the host isn't possible yet.

`--state-dir <dir>` is where the host keeps its own state, such as the cached copy of a Dropbox
Vault file; it defaults to `$XDG_STATE_HOME/keefast-agent`.

## On NixOS

The flake has a NixOS module, `services.keefast-agent`. It runs `keefast-agent serve` as a
system service under a user of its own, `keefast-agent`, with its state in
`/var/lib/keefast-agent`, its admin socket at `/run/keefast-agent/admin.sock`, and systemd
hardening (`ProtectSystem=strict`, `NoNewPrivileges`, no capabilities, a system call filter).
It puts `keefast-agent` on the PATH with `KEEFAST_AGENT_ADMIN_SOCKET` set, so `keefast-agent
unlock` finds the host by itself.

```nix
{
  inputs.keefast.url = "github:arthur-er/keefast";

  outputs = { nixpkgs, keefast, ... }: {
    nixosConfigurations.server = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        keefast.nixosModules.default
        {
          services.keefast-agent = {
            enable = true;
            vault = "/var/lib/keefast-agent/agents.kdbx";
            adminGroup = "keefast-operators";
          };
          users.users.alice.extraGroups = [ "keefast-operators" ];
        }
      ];
    };
  };
}
```

| Option | Default | What it does |
|---|---|---|
| `enable` | `false` | Runs the Headless host. |
| `package` | the flake's `keefast-agent` | The `keefast-agent` package (also `packages.<system>.keefast-agent`, or `pkgs.keefast-agent` from the overlay). |
| `vault` | (required) | The Vault file: a path, or `dropbox:/path`. The `keefast-agent` user must read and write it; outside `/var/lib/keefast-agent` its directory is made writable to the service. |
| `listen` | `"127.0.0.1:7807"` | The address and port Agents reach the host on. Anything but loopback needs `tls`: the system refuses to build without it, as `serve` refuses to start. |
| `tls.certFile`, `tls.keyFile` | `null` | PEM certificate chain and private key for built-in TLS. Both or neither. |
| `adminGroup` | `null` | A group whose members may `unlock`, `lock`, `status` and `pairing` the host over its admin socket; made if missing. Without one, only root and `keefast-agent` can (`sudo keefast-agent unlock`). |
| `unattendedUnlock.keyFile` | `null` | Unlock at startup from this Key file, for a Vault whose Master key is the Key file alone. The Vault loses its protection at rest, and the host warns about it each time it starts. |
| `openFirewall` | `false` | Opens `listen`'s port. |
| `extraArgs` | `[ ]` | More arguments for `keefast-agent serve`. |

The TLS files and the unattended Key file reach the service as systemd credentials
(`LoadCredential`), so they can stay readable by root alone. Give them as strings, not Nix
paths, so they stay out of the Nix store. Unattended unlock from a password in the OS secret
store isn't offered: a system service has no secret store to keep it in.

A NixOS VM test (`nix build .#checks.x86_64-linux.headless-host`, part of `nix flake check`;
it needs KVM) boots the service, unlocks it over the admin socket, and checks it is locked
again after a restart.
