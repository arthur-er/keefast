# The NixOS module refuses, when the system is built, a listen address other machines reach
# without TLS, as `keefast-agent serve` does when it starts; and accepts one with TLS.
{
  pkgs,
  nixpkgs,
  module,
}:

let
  failedAssertions =
    agent:
    let
      system = nixpkgs.lib.nixosSystem {
        inherit (pkgs.stdenv.hostPlatform) system;
        modules = [
          module
          {
            services.keefast-agent = {
              enable = true;
              vault = "/var/lib/keefast-agent/agents.kdbx";
            }
            // agent;
            boot.loader.grub.enable = false;
            fileSystems."/" = {
              device = "/dev/null";
              fsType = "ext4";
            };
            system.stateVersion = "25.11";
          }
        ];
      };
    in
    map (assertion: assertion.message) (
      builtins.filter (assertion: !assertion.assertion) system.config.assertions
    );

  plain = failedAssertions { listen = "0.0.0.0:7807"; };
  withTls = failedAssertions {
    listen = "[::]:7807";
    tls.certFile = "/var/lib/acme/agents/fullchain.pem";
    tls.keyFile = "/var/lib/acme/agents/key.pem";
  };
  loopback = failedAssertions { listen = "[::1]:7807"; };
  needsTls = builtins.any (pkgs.lib.hasInfix "in the clear") plain;
in
assert pkgs.lib.assertMsg needsTls "0.0.0.0 without TLS was accepted: ${toString plain}";
assert pkgs.lib.assertMsg (withTls == [ ]) "[::] with TLS was refused: ${toString withTls}";
assert pkgs.lib.assertMsg (loopback == [ ]) "[::1] was refused: ${toString loopback}";
pkgs.runCommand "keefast-agent-needs-tls" { } "touch $out"
