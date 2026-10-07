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
      devShells = forAllSystems (
        pkgs:
        let
          # Libraries GPUI loads at runtime (dlopen), so they must be on LD_LIBRARY_PATH.
          runtimeLibs = with pkgs; [
            vulkan-loader
            wayland
            libxkbcommon
            libGL
            libx11
            libxcb
            libxcursor
            libxi
            libxrandr
            fontconfig
            freetype
          ];

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
