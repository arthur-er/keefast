#!/usr/bin/env bash
# Serves the desktop UI over its mock backend (crates/app/ui, `vite --mode mock`) in the
# background, on this worktree's own port, so several worktrees can each run one.
#
#   scripts/ui-mock.sh start    start it (or say where it already runs) and print its URL
#   scripts/ui-mock.sh stop     stop the one this worktree started
#   scripts/ui-mock.sh status   say whether it runs, and where
#   scripts/ui-mock.sh port     print this worktree's port
#
# The port is kept in .t3/ui-mock-port (scripts/setup-worktree.sh writes it), the server's
# process ID in .t3/ui-mock.pid and its output in .t3/ui-mock.log. `stop` kills that process
# group only: never `pkill -f "vite --mode mock"`, which also matches the shell that ran it and
# other worktrees' servers.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
state="$root/.t3"
port_file="$state/ui-mock-port"
pid_file="$state/ui-mock.pid"
log_file="$state/ui-mock.log"

listening() { ss -Hltn "sport = :$1" | grep -q .; }

# A port from 1430 to 2429 derived from the worktree's path, the next free one if that is taken.
# 1420 is Tauri's (`pnpm dev`) and 1421 the plain `pnpm dev:mock` default.
port() {
  if [[ -s "$port_file" ]]; then
    cat "$port_file"
    return
  fi
  local sum candidate
  sum="$(printf '%s' "$root" | cksum | cut -d' ' -f1)"
  candidate=$((1430 + sum % 1000))
  while listening "$candidate"; do candidate=$((candidate + 1)); done
  mkdir -p "$state"
  echo "$candidate" > "$port_file"
  echo "$candidate"
}

running_pid() {
  [[ -s "$pid_file" ]] || return 1
  local pid
  pid="$(cat "$pid_file")"
  if kill -0 "$pid" 2>/dev/null; then
    echo "$pid"
  else
    rm -f "$pid_file"
    return 1
  fi
}

start() {
  local p pid
  p="$(port)"
  if pid="$(running_pid)"; then
    echo "already running (pid $pid): http://localhost:$p/"
    return
  fi
  if listening "$p"; then
    echo "port $p is taken by a process this worktree didn't start; free it or edit $port_file" >&2
    exit 1
  fi
  if [[ ! -d "$root/crates/app/ui/node_modules" ]]; then
    echo "crates/app/ui/node_modules is missing: run scripts/setup-worktree.sh first" >&2
    exit 1
  fi
  local pnpm=(pnpm)
  if ! command -v pnpm >/dev/null; then pnpm=(nix develop "$root" -c pnpm); fi
  mkdir -p "$state"
  # setsid makes the server its own process group, so `stop` ends pnpm, vite and esbuild together.
  (cd "$root/crates/app/ui" && PORT="$p" exec setsid "${pnpm[@]}" exec vite --mode mock) \
    > "$log_file" 2>&1 < /dev/null &
  echo $! > "$pid_file"
  for _ in $(seq 60); do
    if curl -sf -o /dev/null "http://localhost:$p/"; then
      echo "running (pid $(cat "$pid_file")): http://localhost:$p/  (log: .t3/ui-mock.log)"
      return
    fi
    if ! running_pid > /dev/null; then
      echo "the mock server exited; its log, .t3/ui-mock.log:" >&2
      tail -20 "$log_file" >&2
      exit 1
    fi
    sleep 0.5
  done
  echo "the mock server didn't answer on :$p within 30 s; see .t3/ui-mock.log" >&2
  exit 1
}

stop() {
  local pid
  if ! pid="$(running_pid)"; then
    echo "not running"
    return
  fi
  kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid"
  for _ in $(seq 20); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.25
  done
  kill -KILL -- "-$pid" 2>/dev/null || true
  rm -f "$pid_file"
  echo "stopped (pid $pid)"
}

case "${1:-}" in
  start) start ;;
  stop) stop ;;
  status)
    if pid="$(running_pid)"; then
      echo "running (pid $pid): http://localhost:$(port)/"
    else
      echo "not running"
    fi
    ;;
  port) port ;;
  *)
    echo "usage: scripts/ui-mock.sh start|stop|status|port" >&2
    exit 2
    ;;
esac
