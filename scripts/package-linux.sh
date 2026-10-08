#!/usr/bin/env bash
# Packages release builds of keefast and keefast-native-host as a tarball and an AppImage.
#
#   cargo build --release --locked -p keefast -p keefast-native-host
#   scripts/package-linux.sh [binary dir, default target/release] [output dir, default dist]
#
# Writes keefast-<version>-linux-<arch>.tar.gz and keefast-<version>-<arch>.AppImage. The
# tarball is a prefix tree (bin/, share/), so it runs where it is unpacked, or installs with
# `tar -xzf keefast-*.tar.gz --strip-components=1 -C ~/.local`. keefast finds its native host
# next to itself, so the two binaries always ship together.
#
# The AppImage tools (linuxdeploy, appimagetool and the AppImage runtime) are downloaded at
# pinned versions and checked against their SHA-256 into $KEEFAST_TOOLS_DIR (default
# ~/.cache/keefast-packaging). They run extracted, so FUSE is not needed to build.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin_dir="$(realpath "${1:-$root/target/release}")"
out_dir="${2:-$root/dist}"
tools_dir="${KEEFAST_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/keefast-packaging}"

arch="$(uname -m)"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n1)"
[[ -n "$version" ]] || { echo "no version in Cargo.toml" >&2; exit 1; }

for binary in keefast keefast-native-host; do
  [[ -x "$bin_dir/$binary" ]] || {
    echo "$bin_dir/$binary is missing: build it with" >&2
    echo "  cargo build --release --locked -p keefast -p keefast-native-host" >&2
    exit 1
  }
done

# The AppImage tools, pinned, with their SHA-256 per architecture.
linuxdeploy_release="https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1"
appimagetool_release="https://github.com/AppImage/appimagetool/releases/download/1.9.1"
runtime_release="https://github.com/AppImage/type2-runtime/releases/download/20251108"
case "$arch" in
  x86_64)
    linuxdeploy_sha=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
    appimagetool_sha=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
    runtime_sha=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
    ;;
  aarch64)
    linuxdeploy_sha=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff
    appimagetool_sha=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
    runtime_sha=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444
    ;;
  *) echo "no AppImage tools pinned for $arch" >&2; exit 1 ;;
esac

# Downloads a tool once, and refuses one whose checksum is wrong.
fetch() {
  local url="$1" sha="$2" path
  path="$tools_dir/$(basename "$url")"
  if ! echo "$sha  $path" | sha256sum --check --status 2>/dev/null; then
    mkdir -p "$tools_dir"
    curl --fail --silent --show-error --location --output "$path.part" "$url"
    echo "$sha  $path.part" | sha256sum --check --quiet
    mv "$path.part" "$path"
  fi
  chmod +x "$path"
  echo "$path"
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out_dir"
out_dir="$(realpath "$out_dir")"

# The tarball.
name="keefast-$version-linux-$arch"
tree="$work/$name"
install -Dm755 "$bin_dir/keefast" "$bin_dir/keefast-native-host" -t "$tree/bin"
install -Dm644 "$root/packaging/linux/keefast.desktop" -t "$tree/share/applications"
install -Dm644 "$root/assets/icon.svg" "$tree/share/icons/hicolor/scalable/apps/keefast.svg"
install -Dm644 "$root/LICENSE" "$root/README.md" -t "$tree/share/doc/keefast"
tar -C "$work" --owner=0 --group=0 --numeric-owner -czf "$out_dir/$name.tar.gz" "$name"
echo "$out_dir/$name.tar.gz"

# The AppImage: linuxdeploy bundles the libraries a desktop can't be expected to have, and
# points AppRun at keefast. The host is in the image's bin too; keefast copies it out when it
# registers it with the browsers, as the image is mounted somewhere new each run.
linuxdeploy="$(fetch "$linuxdeploy_release/linuxdeploy-$arch.AppImage" "$linuxdeploy_sha")"
appimagetool="$(fetch "$appimagetool_release/appimagetool-$arch.AppImage" "$appimagetool_sha")"
runtime="$(fetch "$runtime_release/runtime-$arch" "$runtime_sha")"
export APPIMAGE_EXTRACT_AND_RUN=1

appdir="$work/keefast.AppDir"
"$linuxdeploy" --appdir "$appdir" \
  --executable "$tree/bin/keefast" \
  --executable "$tree/bin/keefast-native-host" \
  --desktop-file "$tree/share/applications/keefast.desktop" \
  --icon-file "$tree/share/icons/hicolor/scalable/apps/keefast.svg"
install -Dm644 "$root/LICENSE" "$root/README.md" -t "$appdir/usr/share/doc/keefast"

image="$out_dir/keefast-$version-$arch.AppImage"
ARCH="$arch" "$appimagetool" --no-appstream --runtime-file "$runtime" "$appdir" "$image"
echo "$image"
