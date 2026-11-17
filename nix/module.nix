# services.keefast-agent: a Headless host (`keefast-agent serve`, ADR 0012) as a NixOS system
# service, under a user of its own, with its state in /var/lib/keefast-agent. docs/headless-host.md
# is the operator's guide; its "On NixOS" section documents these options.
{
  config,
  lib,
  pkgs,
  utils,
  ...
}:

let
  cfg = config.services.keefast-agent;
  inherit (lib)
    mkEnableOption
    mkIf
    mkOption
    types
    ;

  stateDir = "/var/lib/keefast-agent";
  adminSocket = "/run/keefast-agent/admin.sock";

  # As `serve` decides it: loopback is 127.0.0.0/8 or [::1]; anything else needs TLS.
  loopback = lib.hasPrefix "127." cfg.listen || lib.hasPrefix "[::1]:" cfg.listen;
  tls = cfg.tls.certFile != null;
  port = lib.toInt (lib.last (lib.splitString ":" cfg.listen));

  # A local Vault file is saved through a temporary file beside it, so its directory must be
  # writable; a Dropbox one is cached in the state directory.
  vaultDir =
    if lib.hasPrefix "dropbox:" cfg.vault || lib.hasPrefix "${stateDir}/" cfg.vault then
      null
    else
      dirOf cfg.vault;

  # Secret files reach the service as systemd credentials: they stay readable only by root (or
  # whoever owns them), and the service reads its own copy, which nothing else can.
  credentials =
    lib.optional tls "tls-cert:${cfg.tls.certFile}"
    ++ lib.optional tls "tls-key:${cfg.tls.keyFile}"
    ++ lib.optional (
      cfg.unattendedUnlock.keyFile != null
    ) "unattended-key-file:${cfg.unattendedUnlock.keyFile}";

  args = [
    "serve"
    "--vault"
    cfg.vault
    "--listen"
    cfg.listen
    "--admin-socket"
    adminSocket
    "--state-dir"
    stateDir
  ]
  ++ lib.optionals (cfg.adminGroup != null) [
    "--admin-group"
    cfg.adminGroup
  ]
  ++ cfg.extraArgs;

  # The credentials' paths, in systemd's %d (the credentials directory), which must not be
  # escaped as the arguments above are.
  credentialArgs =
    lib.optionals tls [
      "--tls-cert %d/tls-cert"
      "--tls-key %d/tls-key"
    ]
    ++ lib.optional (
      cfg.unattendedUnlock.keyFile != null
    ) "--unattended-key-file %d/unattended-key-file";
in
{
  options.services.keefast-agent = {
    enable = mkEnableOption ''
      a keefast Headless host (`keefast-agent serve`), which serves one Vault to Agents. It
      starts locked: an operator unlocks it with `keefast-agent unlock`, which this module puts
      on the PATH, pointed at the host's admin socket'';

    package = mkOption {
      type = types.package;
      default = pkgs.callPackage ./agent-package.nix { };
      defaultText = lib.literalExpression "keefast.packages.\${system}.keefast-agent";
      description = "The keefast-agent package.";
    };

    vault = mkOption {
      type = types.str;
      example = "/var/lib/keefast-agent/agents.kdbx";
      description = ''
        The Vault file to serve: a path, or `dropbox:/path` for one in Dropbox. The service's
        user, `keefast-agent`, must be able to read and write it, and write its directory. Under
        ${stateDir} it is the service's own; elsewhere its directory is made writable to the
        service. Serve a Vault kept only for Agents.
      '';
    };

    listen = mkOption {
      type = types.str;
      default = "127.0.0.1:7807";
      example = "[::]:7807";
      description = ''
        The IP address and port Agents reach the host on. An address other machines can reach
        needs `tls`; on loopback, put a proxy that does TLS in front, such as `tailscale serve`
        or a reverse proxy.
      '';
    };

    tls = {
      certFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/var/lib/acme/agents.example.com/fullchain.pem";
        description = ''
          The host's TLS certificate chain, as PEM. Read by systemd as a credential, so it
          needn't be readable by the service's user. Give it as a string, not a Nix path, so it
          stays out of the Nix store.
        '';
      };
      keyFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/var/lib/acme/agents.example.com/key.pem";
        description = ''
          The private key for `tls.certFile`, as PEM. Read like it, as a credential, and given
          as a string for the same reason.
        '';
      };
    };

    adminGroup = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "keefast-operators";
      description = ''
        A group whose members may unlock, lock and check the host over its admin socket
        (${adminSocket}). The group is made if it doesn't exist. Without one, only root and the
        service's user can, as with `sudo -u keefast-agent keefast-agent unlock`.
      '';
    };

    unattendedUnlock.keyFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/etc/keefast/agents.keyx";
      description = ''
        Unlock at startup with this Key file, without anyone typing the Master key: for a Vault
        whose Master key is the Key file alone. **The Vault then loses its protection at rest:**
        whoever can read this file can open it. The host warns about this each time it starts.
        Read by systemd as a credential, so it may be readable by root alone; give it as a
        string, so it stays out of the Nix store.
      '';
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Open `listen`'s port in the firewall, for Agents on other machines.";
    };

    extraArgs = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "More arguments for `keefast-agent serve`.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = (cfg.tls.certFile == null) == (cfg.tls.keyFile == null);
        message = "services.keefast-agent.tls needs both certFile and keyFile, or neither.";
      }
      {
        assertion = loopback || tls;
        message = ''
          services.keefast-agent.listen = "${cfg.listen}" is open to other machines, and Agents'
          tokens would cross the network in the clear: set services.keefast-agent.tls, or listen
          on loopback (127.0.0.1 or [::1]) behind a proxy that does TLS, such as `tailscale serve`.
        '';
      }
    ];

    users.users.keefast-agent = {
      isSystemUser = true;
      group = "keefast-agent";
      home = stateDir;
      description = "keefast Headless host";
    };
    users.groups = {
      keefast-agent = { };
    }
    // lib.optionalAttrs (cfg.adminGroup != null) { ${cfg.adminGroup} = { }; };

    # `keefast-agent unlock`, `lock` and `status` find the admin socket without being told.
    environment.systemPackages = [ cfg.package ];
    environment.variables.KEEFAST_AGENT_ADMIN_SOCKET = adminSocket;

    networking.firewall.allowedTCPPorts = lib.optional cfg.openFirewall port;

    systemd.services.keefast-agent = {
      description = "keefast Headless host";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        ExecStart = lib.concatStringsSep " " (
          [
            (lib.getExe cfg.package)
            (utils.escapeSystemdExecArgs args)
          ]
          ++ credentialArgs
        );
        Restart = "on-failure";

        User = "keefast-agent";
        Group = "keefast-agent";
        # To hand the admin socket to the admin group, the service must be in it.
        SupplementaryGroups = lib.optional (cfg.adminGroup != null) cfg.adminGroup;
        LoadCredential = credentials;

        StateDirectory = "keefast-agent";
        StateDirectoryMode = "0700";
        # The admin group passes through to the socket, which is itself 0660; others can't
        # open it.
        RuntimeDirectory = "keefast-agent";
        RuntimeDirectoryMode = if cfg.adminGroup != null then "0711" else "0700";
        ReadWritePaths = lib.optional (vaultDir != null) "-${vaultDir}";
        UMask = "0077";

        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        ProcSubset = "pid";
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_INET"
          "AF_INET6"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        RemoveIPC = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        CapabilityBoundingSet = "";
        AmbientCapabilities = "";
        SystemCallArchitectures = "native";
        # chown is in @privileged, and the admin socket is chowned to the admin group.
        SystemCallFilter = [
          "@system-service"
          "~@privileged @resources"
          "@chown"
        ];
        SystemCallErrorNumber = "EPERM";
      };
    };
  };
}
