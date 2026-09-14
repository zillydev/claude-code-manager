#!/usr/bin/env bash
# `make dev`: Vite + the debug app, relaunched whenever it exits with
# RELAUNCH_CODE (its Develop → Rebuild & Relaunch, after a successful build).
# Nothing watches files, so edits never restart the window on their own.
set -euo pipefail

PORT="${PORT:-5173}"
RELAUNCH_CODE=75 # dev_reload::RELAUNCH_EXIT_CODE
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT/crates/app"
FRONTEND_DIR="$APP_DIR/frontend"
APP_BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/claude-code-manager-app"

export CM_PORT="$PORT"
# Must match byte-for-byte between builds, or tauri-build reruns and the app
# crate recompiles; the app's own rebuild inherits this exact value.
export TAURI_CONFIG="{\"build\":{\"devUrl\":\"http://localhost:$PORT\"}}"
export CM_DEV_LAUNCHER=1

vite_pid=""
app_pid=""
cleanup() {
  if [[ -n "$app_pid" ]]; then kill "$app_pid" 2>/dev/null || true; fi
  if [[ -n "$vite_pid" ]]; then kill "$vite_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

log() { printf '\033[1;36m[dev]\033[0m %s\n' "$*"; }

if [[ ! -x "$FRONTEND_DIR/node_modules/.bin/vite" ]]; then
  log "installing frontend dependencies"
  npm --prefix "$FRONTEND_DIR" install
fi

# exec so the pid is vite itself, not a wrapper that would outlive a kill.
(cd "$FRONTEND_DIR" && exec node_modules/.bin/vite) &
vite_pid=$!

log "waiting for vite on :$PORT"
for _ in $(seq 120); do
  if ! kill -0 "$vite_pid" 2>/dev/null; then
    log "vite exited (port $PORT in use? try: make dev PORT=5174)"
    exit 1
  fi
  if curl -s -o /dev/null "http://localhost:$PORT"; then break; fi
  sleep 0.5
done

log "building"
(cd "$APP_DIR" && cargo build)

while true; do
  (cd "$APP_DIR" && exec "$APP_BIN") &
  app_pid=$!
  code=0
  wait "$app_pid" || code=$?
  app_pid=""
  if [[ "$code" -ne "$RELAUNCH_CODE" ]]; then
    exit "$code"
  fi
  log "relaunching"
done
