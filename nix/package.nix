# keefast and its native-messaging host, for `nix build` and NixOS.
{
  lib,
  rustPlatform,
  makeWrapper,
  pkg-config,
  cmake,
  perl,
  openssl,
  zstd,
  zlib,
  alsa-lib,
  libxext,
  vulkan-loader,
  wayland,
  libxkbcommon,
  libGL,
  libx11,
  libxcb,
  libxcursor,
  libxi,
  libxrandr,
  fontconfig,
  freetype,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;

  # Libraries GPUI loads at runtime (dlopen), so they must be on LD_LIBRARY_PATH.
  runtimeLibs = [
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
in
rustPlatform.buildRustPackage {
  pname = "keefast";
  inherit (cargoToml.workspace.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../assets
      ../packaging
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package"
    "keefast"
    "--package"
    "keefast-native-host"
  ];
  # CI runs the tests; they need the fixtures, which the package leaves out.
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    cmake
    perl
    makeWrapper
  ];
  buildInputs = [
    openssl
    zstd
    zlib
    alsa-lib
    libxext
  ]
  ++ runtimeLibs;

  # keefast finds keefast-native-host next to itself, and the wrapper keeps it there.
  postInstall = ''
    wrapProgram $out/bin/keefast --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
    install -Dm644 packaging/linux/keefast.desktop -t $out/share/applications
    install -Dm644 assets/icon.svg $out/share/icons/hicolor/scalable/apps/keefast.svg
  '';

  passthru = { inherit runtimeLibs; };

  meta = {
    description = "Fast, native password manager for KeePass (KDBX) Vault files";
    homepage = cargoToml.workspace.package.repository;
    license = lib.licenses.mit;
    mainProgram = "keefast";
    platforms = lib.platforms.linux;
  };
}
