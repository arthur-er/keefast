# The NixOS module (nix/module.nix) in a VM: the Headless host starts locked, an operator in the
# admin group unlocks it over the admin socket and nobody else can reach it, and a restart
# leaves it locked again. An operator pairs an Agent, whose token reaches the MCP endpoint until
# it is revoked. An Approver paired on the host keeps its token, in the state directory, across a
# restart, and opens the prompt stream while locked. A second machine opts in to unattended unlock
# from a Key file that
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
      environment.systemPackages = [
        pkgs.curl
        (pkgs.python3.withPackages (python: [ python.pykeepass ]))
      ];
      # An intranet service an http_request may reach because its Entry allows it by name.
      networking.hosts."127.0.0.2" = [ "intranet.example" ];
      environment.etc."intranet.py".text = ''
        import http.server

        class Echo(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                print("authorization:", self.headers.get("authorization"), flush=True)
                self.send_response(200)
                self.send_header("content-type", "text/plain")
                self.end_headers()
                self.wfile.write(b"hello from the intranet")

        http.server.HTTPServer(("127.0.0.2", 8080), Echo).serve_forever()
      '';
      # Gives an Entry of the Agent's subgroup a password and allows intranet.example for it,
      # with the Default agent policy Allow: no CLI sets allowed hosts, so the test writes the
      # Vault while the host is locked. Prints the Entry's UUID.
      environment.etc."allow-intranet.py".text = ''
        import sys
        from lxml import etree
        from pykeepass import PyKeePass

        vault, password, subgroup = sys.argv[1:]
        kp = PyKeePass(vault, password=password)
        group = kp.find_groups(path=["Agents", subgroup])
        entry = kp.add_entry(group, "intranet", "agent", "s3cret-intranet-value")
        data = kp.tree.find("Meta/CustomData")
        if data is None:
            data = etree.SubElement(kp.tree.find("Meta"), "CustomData")
        for key, value in [
            ("keefast_AGENT_HOSTS_" + entry.uuid.hex, '["intranet.example"]'),
            ("keefast_DEFAULT_AGENT_POLICY", "allow"),
        ]:
            item = next((i for i in data.findall("Item") if i.findtext("Key") == key), None)
            if item is None:
                item = etree.SubElement(data, "Item")
                etree.SubElement(item, "Key").text = key
                etree.SubElement(item, "Value")
            item.find("Value").text = value
        kp.save()
        print(entry.uuid.hex)
      '';
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

    with subtest("an MCP client pairs by OAuth, approved with pairing approve"):
        import base64, hashlib, json, re, urllib.parse
        base = "http://127.0.0.1:7807"
        redirect = "http://127.0.0.1:4444/cb"
        client = json.loads(machine.succeed(
            f"curl -s -X POST {base}/oauth/register -H 'content-type: application/json' "
            f"--data '{{\"client_name\":\"Executor\",\"redirect_uris\":[\"{redirect}\"]}}'"
        ))["client_id"]
        verifier = "v" * 64
        challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
        query = urllib.parse.urlencode({
            "response_type": "code", "client_id": client, "redirect_uri": redirect,
            "code_challenge": challenge, "code_challenge_method": "S256", "state": "vm",
        })
        page = machine.succeed(f"curl -s -o /dev/null -w '%{{redirect_url}}' '{base}/oauth/authorize?{query}'")
        shown = re.search(r"<b>([A-Z0-9]{4}-[A-Z0-9]{4})</b>", machine.succeed(f"curl -s '{page}'"))
        assert shown is not None
        code = shown.group(1)
        machine.succeed(as_user("operator", f"keefast-agent pairing approve {code} --name executor"))
        back = machine.succeed(f"curl -s -o /dev/null -w '%{{redirect_url}}' '{page}'")
        auth_code = urllib.parse.parse_qs(urllib.parse.urlparse(back).query)["code"][0]
        tokens = json.loads(machine.succeed(
            f"curl -s -X POST {base}/oauth/token --data-urlencode grant_type=authorization_code "
            f"--data-urlencode code={auth_code} --data-urlencode redirect_uri={redirect} "
            f"--data-urlencode client_id={client} --data-urlencode code_verifier={verifier}"
        ))
        assert machine.succeed(mcp + f" -H 'authorization: Bearer {tokens['access_token']}'") == "200"
        print(machine.succeed("stat -c '%a %U' /var/lib/keefast-agent/oauth.json"))

    with subtest("an http_request reaches a host its Entry allows by name, resolved by the service"):
        machine.succeed("systemd-run --unit intranet python3 /etc/intranet.py")
        machine.wait_for_open_port(8080, "127.0.0.2")
        web = machine.succeed(
            as_user("operator", "keefast-agent pairing create web --agent-group Agents 2>/dev/null")
        ).strip()
        machine.succeed(as_user("operator", "keefast-agent lock"))
        entry = machine.succeed(
            "python3 /etc/allow-intranet.py ${vault} ${password} web"
        ).strip()
        machine.succeed(as_user("operator", "keefast-agent unlock", "${password}\\n"))

        def mcp_call(session, message):
            headers = (
                "-H 'content-type: application/json' -H 'accept: application/json, text/event-stream' "
                f"-H 'authorization: Bearer {web}'"
            )
            if session:
                headers += f" -H 'mcp-session-id: {session}' -H 'mcp-protocol-version: 2025-06-18'"
            body = json.dumps(message)
            return machine.succeed(
                f"curl -s -D /tmp/mcp-headers {headers} http://127.0.0.1:7807/mcp --data '{body}'"
            )

        def reply(text):
            data = [line[5:] for line in text.splitlines() if line.startswith("data:") and line[5:].strip()]
            return json.loads(data[-1] if data else text)

        mcp_call(None, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "vm", "version": "1"},
        }})
        found = re.search(
            r"(?im)^mcp-session-id:\s*(\S+)", machine.succeed("cat /tmp/mcp-headers")
        )
        assert found is not None
        session = found.group(1)
        mcp_call(session, {"jsonrpc": "2.0", "method": "notifications/initialized"})
        sent = reply(mcp_call(session, {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "http_request",
            "arguments": {
                "method": "GET",
                "url": "http://intranet.example:8080/whoami",
                "headers": {"Authorization": f"Bearer {{{{secret:{entry}#password}}}}"},
            },
        }}))
        text = sent["result"]["content"][0]["text"]
        assert not sent["result"].get("isError"), text
        assert "hello from the intranet" in text and '"status": 200' in text, text
        machine.succeed("journalctl -u intranet | grep 'authorization: Bearer s3cret-intranet-value'")
        machine.succeed(as_user("operator", "keefast-agent log") + " | grep http_request")

    with subtest("an operator pairs an Approver"):
        approver = machine.succeed(
            as_user("operator", "keefast-agent pairing approver laptop 2>/dev/null")
        ).strip()
        assert approver.startswith("kfapprover_"), approver
        machine.succeed(as_user("operator", "keefast-agent pairing approvers") + " | grep laptop")

    with subtest("a restart leaves it locked"):
        machine.systemctl("restart keefast-agent.service")
        machine.wait_for_unit("keefast-agent.service")
        machine.wait_until_succeeds(
            as_user("operator", "keefast-agent status") + " | grep 'State: locked'"
        )

    with subtest("the Approver still opens its prompt stream, locked and after the restart"):
        # The stream stays open: curl gives up after a second, having printed the status.
        events = lambda token: (
            "curl -s -o /dev/null -m 1 -w '%{http_code}' http://127.0.0.1:7807/approver/events "
            f"-H 'authorization: Bearer {token}' || true"
        )
        assert machine.succeed(events(approver)) == "200"
        assert machine.succeed(events("kfapprover_nobodys")) == "401"

    with subtest("the OAuth client's refresh token survives the restart"):
        refreshed = json.loads(machine.succeed(
            f"curl -s -X POST {base}/oauth/token --data-urlencode grant_type=refresh_token "
            f"--data-urlencode refresh_token={tokens['refresh_token']} --data-urlencode client_id={client}"
        ))
        assert refreshed["access_token"].startswith("kfo_"), refreshed

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
