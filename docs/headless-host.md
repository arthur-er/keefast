# Running a Headless host

A Headless host is keefast with no window, serving one Vault to Agents from a server
(ADR 0012, [the Agents spec](specs/agents.md)). It is the `keefast-agent serve` command.

## Serving and unlocking

```sh
keefast-agent serve --vault /srv/agents.kdbx          # or --vault dropbox:/Vaults/agents.kdbx
```

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
