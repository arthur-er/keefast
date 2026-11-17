# keefast-agent alone (ADR 0012): the Headless host and the commands Agents and operators run,
# for servers and the NixOS module (nix/module.nix). It never links Tauri or WebKit, so it
# builds without the desktop app's GUI libraries or its web UI.
{
  lib,
  rustPlatform,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "keefast-agent";
  version = cargoToml.workspace.package.version;

  # Every workspace member's manifest is there for Cargo to read the workspace; only
  # keefast-agent and the crates it uses are built. The desktop app's web UI is left out.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      (lib.fileset.difference ../crates (
        lib.fileset.unions [
          ../crates/app/ui
          (lib.fileset.maybeMissing ../crates/app/gen)
        ]
      ))
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package"
    "keefast-agent"
  ];
  # CI runs the tests; they need the fixtures, which the package leaves out.
  doCheck = false;

  meta = {
    description = "keefast's Headless host: serves a KeePass (KDBX) Vault's secrets to AI agents";
    homepage = cargoToml.workspace.package.repository;
    license = lib.licenses.mit;
    mainProgram = "keefast-agent";
    platforms = lib.platforms.unix;
  };
}
