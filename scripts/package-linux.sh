#!/usr/bin/env bash
# Packages release builds of keefast, keefast-native-host and keefast-agent as a tarball and an
# AppImage.
#
#   pnpm --dir crates/app/ui install --frozen-lockfile && pnpm --dir crates/app/ui build
#   cargo build --release --locked -p keefast -p keefast-native-host -p keefast-agent --features keefast/custom-protocol
#   scripts/package-linux.sh [binary dir, default target/release] [output dir, default dist]
#
# Writes keefast-<version>-linux-<arch>.tar.gz and keefast-<version>-<arch>.AppImage.
#
# The tarball is a prefix tree (bin/, share/), so it runs where it is unpacked, or installs with
# `tar -xzf keefast-*.tar.gz --strip-components=1 -C ~/.local`. keefast finds its native host
# next to itself, so the binaries always ship together. keefast-agent (ADR 0012) is the command
# Agents run (`keefast-agent run`) and the Headless host (`keefast-agent serve`); it needs no
# WebKitGTK or display. It uses the system's WebKitGTK 4.1
# (libwebkit2gtk-4.1-0 on Debian and Ubuntu, webkit2gtk4.1 on Fedora), and for the Tray icon its
# libayatana-appindicator (libayatana-appindicator3-1, libayatana-appindicator-gtk3).
#
# The AppImage is made by Tauri's bundler (`cargo tauri build`, from the devShell or
# `cargo install tauri-cli --locked`), which bundles WebKitGTK with its helper processes, as plain
# linuxdeploy can't. The native host goes into the image's bin next to keefast; keefast copies it
# out when it registers it with the browsers, as the image is mounted somewhere new each run.
# keefast-agent goes there too, and keefast copies it out at every start, so Agent configs that
# name it keep working between runs. The
# bundler downloads its AppImage tools (linuxdeploy and its GTK plugin) itself.
#
# The Tray icon (ADR 0013) is drawn by libayatana-appindicator, which keefast loads at runtime
# (dlopen) rather than links, so the bundler wouldn't see it: the script puts it in the image's
# usr/lib itself, where linuxdeploy adds its own dependencies. It must be installed
# (libayatana-appindicator3-1 on Debian and Ubuntu).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin_dir="$(realpath "${1:-$root/target/release}")"
out_dir="${2:-$root/dist}"

arch="$(uname -m)"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n1)"
[[ -n "$version" ]] || { echo "no version in Cargo.toml" >&2; exit 1; }

for binary in keefast keefast-native-host keefast-agent; do
  [[ -x "$bin_dir/$binary" ]] || {
    echo "$bin_dir/$binary is missing: build the web UI, then" >&2
    echo "  cargo build --release --locked -p keefast -p keefast-native-host -p keefast-agent --features keefast/custom-protocol" >&2
    exit 1
  }
done

work="$(mktemp -d)"
trap 'rm -rf "$work" "$root/crates/app/binaries"' EXIT
mkdir -p "$out_dir"
out_dir="$(realpath "$out_dir")"

# The tarball.
name="keefast-$version-linux-$arch"
tree="$work/$name"
install -Dm755 "$bin_dir/keefast" "$bin_dir/keefast-native-host" "$bin_dir/keefast-agent" -t "$tree/bin"
install -Dm644 "$root/packaging/linux/keefast.desktop" -t "$tree/share/applications"
install -Dm644 "$root/assets/icon.svg" "$tree/share/icons/hicolor/scalable/apps/keefast.svg"
install -Dm644 "$root/LICENSE" "$root/README.md" -t "$tree/share/doc/keefast"
tar -C "$work" --owner=0 --group=0 --numeric-owner -czf "$out_dir/$name.tar.gz" "$name"
echo "$out_dir/$name.tar.gz"

# The AppImage. Tauri takes the host and keefast-agent as "external binaries", named for the target triple.
triple="$(rustc -vV | sed -n 's/^host: //p')"
appindicator="$("$(command -v ldconfig || echo /sbin/ldconfig)" -p | awk '$1 == "libayatana-appindicator3.so.1" { print $NF; exit }')"
[[ -n "$appindicator" ]] || {
  echo "libayatana-appindicator3.so.1 is missing: install libayatana-appindicator3-1 for the Tray icon" >&2
  exit 1
}
config="$(printf '{"build":{"beforeBuildCommand":""},"bundle":{"externalBin":["binaries/keefast-native-host","binaries/keefast-agent"],"linux":{"appimage":{"files":{"/usr/lib/libayatana-appindicator3.so.1":"%s"}}}}}' "$(realpath "$appindicator")")"
mkdir -p "$root/crates/app/binaries"
install -m755 "$bin_dir/keefast-native-host" "$root/crates/app/binaries/keefast-native-host-$triple"
install -m755 "$bin_dir/keefast-agent" "$root/crates/app/binaries/keefast-agent-$triple"
(
  cd "$root/crates/app"
  # The web UI is already built; the bundler builds keefast again only if it changed.
  APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true cargo tauri build --bundles appimage \
    --config "$config"
)
built="$(find "$root/target/release/bundle/appimage" -maxdepth 1 -name '*.AppImage' -newer "$work" | head -n1)"
[[ -n "$built" ]] || { echo "the bundler made no AppImage" >&2; exit 1; }
image="$out_dir/keefast-$version-$arch.AppImage"
cp "$built" "$image"
echo "$image"
