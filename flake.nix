{
  description = "keefast: native KDBX password manager (GPUI)";

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
      packages = forAllSystems (
        pkgs:
        let
          keefast = pkgs.callPackage ./nix/package.nix { };
        in
        {
          inherit keefast;
          default = keefast;
        }
      );

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = nixpkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.keefast;
        };
      });

      # On NixOS: `environment.systemPackages = [ keefast.packages.${system}.default ];`, or
      # use the overlay to get `pkgs.keefast`.
      overlays.default = final: _prev: {
        keefast = final.callPackage ./nix/package.nix { };
      };

      devShells = forAllSystems (
        pkgs:
        let
          inherit (self.packages.${pkgs.stdenv.hostPlatform.system}.keefast) runtimeLibs;

          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
              pkg-config
              cmake
              perl
              openssl
              zstd
              alsa-lib
              libxext
              # The Companion extension (extension/).
              nodejs
            ] ++ runtimeLibs;

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
          };
        in
        {
          inherit default;

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
