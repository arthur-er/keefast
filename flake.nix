{
  description = "keefast: KDBX password manager (Tauri)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # On NixOS: `environment.systemPackages = [ keefast.packages.${system}.default ];`, or
      # use the overlay to get `pkgs.keefast`. `nix run` starts keefast.
      overlays.default = final: _prev: {
        keefast = final.callPackage ./nix/package.nix { };
        keefast-agent = final.callPackage ./nix/agent-package.nix { };
      };

      # A Headless host as a system service: `imports = [ keefast.nixosModules.default ];` and
      # `services.keefast-agent.enable = true;` (docs/headless-host.md, "On NixOS").
      nixosModules.default = ./nix/module.nix;

      packages = forAllSystems (
        pkgs:
        let
          inherit (self.overlays.default pkgs pkgs) keefast keefast-agent;
        in
        {
          inherit keefast keefast-agent;
          default = keefast;
        }
      );

      # `nix flake check`: the NixOS module in a VM (needs KVM), and the module refusing to
      # listen where other machines reach it without TLS.
      checks = forAllSystems (pkgs: {
        headless-host = import ./nix/tests/headless-host.nix {
          inherit pkgs;
          module = self.nixosModules.default;
        };
        headless-host-needs-tls = import ./nix/tests/needs-tls.nix {
          inherit pkgs nixpkgs;
          module = self.nixosModules.default;
        };
      });

      devShells = forAllSystems (
        pkgs:
        let
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
              pkg-config
              openssl
              # The desktop app (crates/app): Tauri on WebKitGTK, and its web UI (crates/app/ui),
              # which the Companion extension (extension/) shares the Node toolchain with.
              cargo-tauri
              nodejs
              pnpm
              webkitgtk_4_1
              libsoup_3
              gtk3
              glib
              glib-networking
              librsvg
              cairo
              pango
              gdk-pixbuf
              atk
              dbus
              # The Tray icon's library, which keefast loads at runtime (ADR 0013).
              libayatana-appindicator
              # Links Plugins' WebAssembly components (`keefast-plugin build`, plugins/card/build.sh).
              lld
            ];

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            # WebKitGTK's TLS and the GSettings schemas GTK's file dialogs need.
            GIO_MODULE_DIR = "${pkgs.glib-networking}/lib/gio/modules/";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.libayatana-appindicator ];
            shellHook = ''
              export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
            '';
          };
        in
        {
          inherit default;

          # Just enough to build Plugins and keefast-plugin, with the same rustc as the default
          # shell, so card.wasm builds byte for byte the same (`plugins/card/build.sh --check`).
          plugins = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              lld
            ];
          };

          # The KeePass clients that write the round-trip corpus (scripts/make-fixtures.sh) and
          # check keefast's saves against it (scripts/check-interop.sh).
          corpus = pkgs.mkShell {
            inputsFrom = [ default ];
            packages = with pkgs; [
              keepassxc
              mono
              nodejs
              (python3.withPackages (python: [ python.pykeepass ]))
            ];

            KEEPASSLIB = "${pkgs.keepass}/lib/dotnet/keepass";
          };
        }
      );
    };
}
