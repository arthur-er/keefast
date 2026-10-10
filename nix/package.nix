# keefast and its native-messaging host, for `nix build` and NixOS: the Tauri app with its web UI
# (crates/app/ui) built in, its pnpm dependencies fetched reproducibly.
{
  lib,
  rustPlatform,
  fetchPnpmDeps,
  pnpmConfigHook,
  pnpm_10,
  nodejs,
  pkg-config,
  wrapGAppsHook3,
  openssl,
  webkitgtk_4_1,
  libsoup_3,
  glib-networking,
  gtk3,
  librsvg,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
  version = cargoToml.workspace.package.version;
  pnpm = pnpm_10;
  ui = ../crates/app/ui;
in
rustPlatform.buildRustPackage {
  pname = "keefast";
  inherit version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      (lib.fileset.difference ../crates (
        lib.fileset.unions [
          (lib.fileset.maybeMissing ../crates/app/ui/node_modules)
          (lib.fileset.maybeMissing ../crates/app/ui/dist)
          (lib.fileset.maybeMissing ../crates/app/ui/dist-mock)
          (lib.fileset.maybeMissing ../crates/app/gen)
        ]
      ))
      # Plugins compiled into keefast: their manifests and built components.
      (lib.fileset.difference ../plugins (lib.fileset.maybeMissing ../plugins/card/target))
      ../assets
      ../packaging
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  # The web UI's dependencies, from its lockfile alone. Update the hash after changing it: build
  # with `hash = lib.fakeHash;` and copy the hash Nix reports.
  pnpmDeps = fetchPnpmDeps {
    pname = "keefast-ui";
    inherit version pnpm;
    src = lib.fileset.toSource {
      root = ui;
      fileset = lib.fileset.unions [
        (ui + "/package.json")
        (ui + "/pnpm-lock.yaml")
      ];
    };
    fetcherVersion = 4;
    hash = "sha256-Ddn10uTFop2s6ajjFkZEB8as1pEHc53Xa130Cq3XjgY=";
  };
  pnpmRoot = "crates/app/ui";

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook3
    nodejs
    pnpm
    pnpmConfigHook
  ];
  buildInputs = [
    openssl
    webkitgtk_4_1
    libsoup_3
    glib-networking
    gtk3
    librsvg
  ];

  # The web UI first: the release build serves it from inside the binary.
  preBuild = ''
    pnpm --dir crates/app/ui build
  '';
  cargoBuildFlags = [
    "--package"
    "keefast"
    "--package"
    "keefast-native-host"
    "--features"
    "keefast/custom-protocol"
  ];
  # CI runs the tests; they need the fixtures, which the package leaves out.
  doCheck = false;

  # keefast finds keefast-native-host next to itself; wrapGAppsHook3 gives keefast what
  # WebKitGTK needs at runtime (GIO modules, GSettings schemas), and only keefast: the host is
  # started by browsers and needs none of it.
  dontWrapGApps = true;
  postFixup = ''
    wrapGApp $out/bin/keefast
  '';
  postInstall = ''
    install -Dm644 packaging/linux/keefast.desktop -t $out/share/applications
    install -Dm644 assets/icon.svg $out/share/icons/hicolor/scalable/apps/keefast.svg
  '';

  meta = {
    description = "Fast password manager for KeePass (KDBX) Vault files";
    homepage = cargoToml.workspace.package.repository;
    license = lib.licenses.mit;
    mainProgram = "keefast";
    platforms = lib.platforms.linux;
  };
}
