#!/usr/bin/env bash
# Smoke-checks a keefast build on X11 and on Wayland: starts it on a headless X server (Xvfb)
# and on a headless Wayland compositor (sway), and fails unless it is still running a few
# seconds later on both. With browser integration on, it also checks that keefast registered a
# native host that is there to start, which for an AppImage means a copy outside the image, and
# that an AppImage carries the library the Tray icon is drawn with.
#
#   scripts/smoke-linux.sh <keefast binary or AppImage> [screenshot dir]
#
# Needs Xvfb and sway; WebKitGTK renders in software there. With a screenshot dir, it
# also saves x11.png and wayland.png there (needs xdotool, ImageMagick's `import` and grim).
set -euo pipefail

app="$(realpath "$1")"
shots="${2:-}"
[[ -z "$shots" ]] || { mkdir -p "$shots"; shots="$(realpath "$shots")"; }
wait_seconds="${KEEFAST_SMOKE_SECONDS:-8}"

work="$(mktemp -d)"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

# Without FUSE, the AppImage runs from a temporary extraction.
export APPIMAGE_EXTRACT_AND_RUN=1
# keefast keeps its config and registrations out of the user's own.
export HOME="$work/home" XDG_CONFIG_HOME="$work/home/.config" XDG_DATA_HOME="$work/home/.local/share"
export XDG_RUNTIME_DIR="$work/runtime"
mkdir -p "$HOME" "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME/keefast" "$XDG_CONFIG_HOME/chromium"
chmod 700 "$XDG_RUNTIME_DIR"
echo "browser_integration = true" >"$XDG_CONFIG_HOME/keefast/config.toml"
manifest="$XDG_CONFIG_HOME/chromium/NativeMessagingHosts/org.keepassxc.keepassxc_browser.json"

# Runs keefast with the given environment, and fails if it exits within the wait.
check() {
  local display="$1"
  shift
  env "$@" "$app" >"$work/$display.log" 2>&1 &
  local pid=$!
  pids+=("$pid")
  running="$pid"
  sleep "$wait_seconds"
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "keefast exited on $display:" >&2
    cat "$work/$display.log" >&2
    exit 1
  fi
  echo "keefast is running on $display"
}

# A display number no one has taken.
display="${KEEFAST_SMOKE_DISPLAY:-99}"
while [[ -e "/tmp/.X11-unix/X$display" || -e "/tmp/.X$display-lock" ]]; do
  display=$((display + 1))
done
Xvfb ":$display" -screen 0 1280x800x24 -nolisten tcp >"$work/xvfb.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do [[ -e "/tmp/.X11-unix/X$display" ]] && break; sleep 0.1; done
check X11 -u WAYLAND_DISPLAY DISPLAY=":$display"
if [[ -n "$shots" ]]; then
  # With no window manager, give the window an input event before the screenshot.
  DISPLAY=":$display" xdotool mousemove 640 400
  sleep 2
  import -display ":$display" -window root "$shots/x11.png"
fi

host="$(sed -n 's/^ *"path": "\(.*\)",$/\1/p' "$manifest" 2>/dev/null || true)"
if [[ -z "$host" || ! -x "$host" ]]; then
  echo "keefast registered no native host it can start: ${host:-no manifest at $manifest}" >&2
  exit 1
fi
if [[ "$app" == *.AppImage && "$host" != "$XDG_DATA_HOME/keefast/"* ]]; then
  echo "the AppImage registered the host inside the image, which won't last: $host" >&2
  exit 1
fi
# The browser starts the host with the extension's origin. With no browser on its stdin it
# soon exits, but it must at least load.
if timeout 5 "$host" chrome-extension://smoke/ </dev/null >/dev/null 2>"$work/host.log"; [[ $? -ge 126 ]] ||
  grep -q "error while loading shared libraries" "$work/host.log"; then
  echo "keefast's native host does not start:" >&2
  cat "$work/host.log" >&2
  exit 1
fi
echo "keefast registered its native host at $host"

if [[ "$app" == *.AppImage ]]; then
  (cd "$work" && "$app" --appimage-extract 'usr/lib/libayatana-appindicator3.so.1' >/dev/null)
  [[ -e "$work/squashfs-root/usr/lib/libayatana-appindicator3.so.1" ]] || {
    echo "the AppImage has no libayatana-appindicator, so it can't show a Tray icon" >&2
    exit 1
  }
  echo "the AppImage carries libayatana-appindicator"
fi

# keefast runs once per user (ADR 0013): the X11 one goes before the Wayland one starts.
kill "$running"
wait "$running" 2>/dev/null || true

# Sway rather than Weston: GTK needs a seat, which headless Weston doesn't have.
printf 'output HEADLESS-1 resolution 1280x800\n' >"$work/sway.config"
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  sway --config "$work/sway.config" >"$work/sway.log" 2>&1 &
pids+=($!)
socket=""
for _ in $(seq 50); do
  socket="$(cd "$XDG_RUNTIME_DIR" && compgen -G 'wayland-[0-9]' | head -n1 || true)"
  [[ -n "$socket" ]] && break
  sleep 0.1
done
[[ -n "$socket" ]] || { echo "sway did not start:" >&2; cat "$work/sway.log" >&2; exit 1; }
check Wayland -u DISPLAY WAYLAND_DISPLAY="$socket"
[[ -z "$shots" ]] || WAYLAND_DISPLAY="$socket" grim "$shots/wayland.png"
