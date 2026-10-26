#!/usr/bin/env bash
# Watches the network activity of Konvert on Linux and macOS (the Windows
# counterpart is scripts/monitor-network.ps1). Konvert is expected to show nothing:
# the UI talks to the Rust core through in-process IPC and FFmpeg only reads and
# writes local files.
#
# Linux: runs the app under strace (following every child: WebKitGTK processes,
#   FFmpeg) and reports any IPv4/IPv6 connect, send or bind, DNS lookups included.
#   Local sockets (D-Bus, X11/Wayland) are allowed.
# macOS: polls lsof for Internet sockets of the app, its children and the WebKit
#   helper processes (WKWebView runs them as XPC services, outside the app's tree).
#
# Exit code: 0 if no network activity was seen, 1 otherwise.
#
# Usage: bash scripts/monitor-network.sh [--seconds N] <konvert executable> [args...]
#   e.g. bash scripts/monitor-network.sh target/release/konvert some-file.wav
#        bash scripts/monitor-network.sh Konvert.app/Contents/MacOS/konvert

set -euo pipefail

SECONDS_TO_WATCH=30
if [[ "${1:-}" == "--seconds" ]]; then SECONDS_TO_WATCH="$2"; shift 2; fi
[[ $# -ge 1 ]] || { echo "Usage: $0 [--seconds N] <konvert executable> [args...]" >&2; exit 2; }
APP="$1"; shift

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# All descendants of a process (pgrep -P is available on Linux and macOS).
descendants() {
  local child
  for child in $(pgrep -P "$1" || true); do echo "$child"; descendants "$child"; done
}

stop_tree() {
  local pids
  pids="$1 $(descendants "$1" | tr '\n' ' ')"
  kill -TERM $pids 2>/dev/null || true
  sleep 2
  kill -KILL $pids 2>/dev/null || true
}

case "$(uname -s)" in
  Linux)
    command -v strace >/dev/null || { echo "strace is required (apt install strace)" >&2; exit 2; }
    LOG="$WORK/strace.log"
    strace -f -qq -e trace=connect,bind,sendto,sendmsg,sendmmsg -e signal=none -o "$LOG" \
      "$APP" "$@" &
    TRACER=$!
    echo "Watching $APP for $SECONDS_TO_WATCH s (strace)..."
    sleep "$SECONDS_TO_WATCH"
    APP_PID="$(pgrep -P "$TRACER" | head -n1 || true)"
    [[ -n "$APP_PID" ]] && stop_tree "$APP_PID"
    wait "$TRACER" 2>/dev/null || true
    CALLS="$(grep -cE '^[0-9]+ +(connect|bind|send)' "$LOG" || true)"
    HITS="$(grep -E 'AF_INET6?' "$LOG" || true)"
    echo "System calls seen: ${CALLS:-0} (local sockets are allowed)."
    ;;
  Darwin)
    "$APP" "$@" &
    APP_PID=$!
    echo "Watching $APP for $SECONDS_TO_WATCH s (lsof)..."
    : > "$WORK/hits"
    end=$((SECONDS + SECONDS_TO_WATCH))
    while (( SECONDS < end )); do
      pids="$(printf '%s\n' "$APP_PID" $(descendants "$APP_PID") | paste -sd, -)"
      # Sockets of the app tree, then of WebKit's XPC helpers (named after WebKit).
      { lsof -nP -a -i -p "$pids" 2>/dev/null | tail -n +2 || true
        lsof +c 0 -nP -i 2>/dev/null | grep -E '^com\.apple\.WebKit' || true
      } >> "$WORK/hits"
      sleep 0.5
    done
    stop_tree "$APP_PID"
    HITS="$(sort -u "$WORK/hits")"
    ;;
  *) echo "Unsupported system: $(uname -s)" >&2; exit 2 ;;
esac

if [[ -n "$HITS" ]]; then
  echo "NETWORK ACTIVITY DETECTED:"
  echo "$HITS"
  exit 1
fi
echo "OK: no network activity."
