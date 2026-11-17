# The NixOS module (nix/module.nix) in a VM: the Headless host starts locked, an operator in the
# admin group unlocks it over the admin socket and nobody else can reach it, and a restart
# leaves it locked again. An operator pairs an Agent, whose token reaches the MCP endpoint until
# it is revoked. A second machine opts in to unattended unlock from a Key file that
# only root can read.
{ pkgs, module }:

let
  vault = "/var/lib/keefast-agent/agents.kdbx";
  # The fixtures' Master key: this password, or for keepassxc-raw-32.kdbx the Key file alone.
  password = "fixture-password";
in
pkgs.testers.runNixOSTest {
  name = "keefast-agent";

  nodes = {
    machine = {
      imports = [ module ];
      services.keefast-agent = {
        enable = true;
        inherit vault;
        adminGroup = "keefast-operators";
      };
      users.users.operator = {
        isNormalUser = true;
        extraGroups = [ "keefast-operators" ];
      };
      users.users.stranger.isNormalUser = true;
      environment.systemPackages = [ pkgs.curl ];
    };

    unattended = {
      imports = [ module ];
      services.keefast-agent = {
        enable = true;
        inherit vault;
        unattendedUnlock.keyFile = "/etc/keefast/agents.key";
      };
      environment.etc."keefast/agents.key" = {
        source = ../../fixtures/key-files/raw-32.key;
        mode = "0400";
      };
      systemd.tmpfiles.rules = [
        "d /var/lib/keefast-agent 0700 keefast-agent keefast-agent -"
        "C ${vault} 0600 keefast-agent keefast-agent - ${../../fixtures/key-files/keepassxc-raw-32.kdbx}"
      ];
      systemd.services.keefast-agent.after = [ "systemd-tmpfiles-setup.service" ];
    };
  };

  testScript = ''
    def as_user(user, command, stdin=""):
        return f"printf '{stdin}' | su -l {user} -c '{command}' 2>&1"

    start_all()

    with subtest("the host starts locked"):
        machine.wait_for_unit("keefast-agent.service")
        machine.wait_for_file("/run/keefast-agent/admin.sock")
        machine.succeed(
            "install -o keefast-agent -g keefast-agent -m 600 "
            "${../../fixtures/keepassxc-kdbx4.kdbx} ${vault}"
        )
        machine.succeed(as_user("operator", "keefast-agent status") + " | grep 'State: locked'")

    with subtest("only the admin group reaches the admin socket"):
        print(machine.succeed("stat -c '%a %U %G' /run/keefast-agent /run/keefast-agent/admin.sock"))
        out = machine.fail(as_user("stranger", "keefast-agent status"))
        assert "Permission denied" in out, out

    with subtest("a wrong Master key says so"):
        out = machine.fail(as_user("operator", "keefast-agent unlock", "wrong\\n"))
        assert "Master key is wrong" in out, out

    with subtest("unlock opens the Vault"):
        machine.succeed(as_user("operator", "keefast-agent unlock", "${password}\\n"))
        machine.succeed(as_user("operator", "keefast-agent status") + " | grep 'State: unlocked'")

    with subtest("a paired Agent reaches the MCP endpoint, until its pairing is revoked"):
        token = machine.succeed(
            as_user("operator", "keefast-agent pairing create claude --agent-group Agents 2>/dev/null")
        ).strip()
        assert token.startswith("kfa_"), token
        mcp = (
            "curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:7807/mcp "
            "-H 'content-type: application/json' -H 'accept: application/json, text/event-stream' "
            "--data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":"
            "{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},"
            "\"clientInfo\":{\"name\":\"vm\",\"version\":\"1\"}}}'"
        )
        assert machine.succeed(mcp) == "401"
        assert machine.succeed(mcp + f" -H 'authorization: Bearer {token}'") == "200"
        machine.succeed(as_user("operator", "keefast-agent pairing revoke claude"))
        assert machine.succeed(mcp + f" -H 'authorization: Bearer {token}'") == "401"

    with subtest("a restart leaves it locked"):
        machine.systemctl("restart keefast-agent.service")
        machine.wait_for_unit("keefast-agent.service")
        machine.wait_until_succeeds(
            as_user("operator", "keefast-agent status") + " | grep 'State: locked'"
        )

    with subtest("the service is hardened"):
        print(machine.succeed("systemd-analyze security keefast-agent.service | tail -1"))

    with subtest("unattended unlock opens the Vault at startup, and warns"):
        unattended.wait_for_unit("keefast-agent.service")
        unattended.wait_until_succeeds("keefast-agent status | grep 'State: unlocked'")
        unattended.succeed("journalctl -u keefast-agent.service | grep -i 'warning'")
        unattended.systemctl("restart keefast-agent.service")
        unattended.wait_until_succeeds("keefast-agent status | grep 'State: unlocked'")
  '';
}
