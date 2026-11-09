#!/usr/bin/env bash
# A hidden copy of the app for a coding agent to drive with scripts/ui (src/agent_bridge.rs),
# with its own Vite server and throwaway config dir. One per checkout: start|stop|restart|status|logs
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT/crates/app"
FRONTEND_DIR="$APP_DIR/frontend"
APP_BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/claude-code-manager-app"
# Short and per checkout: the socket path must stay under macOS's 104-byte limit.
STATE="/tmp/ccm-agent-$(printf %s "$ROOT" | shasum | cut -c1-8)"
CONFIG_DIR="$STATE/config"
SOCK="$CONFIG_DIR/agent.sock"
# Byte-identical to scripts/dev.sh's default, so this build and `make dev` share one binary.
export TAURI_CONFIG='{"build":{"devUrl":"http://localhost:5173"}}'

log() { printf '\033[1;35m[agent-app]\033[0m %s\n' "$*" >&2; }

alive() { [[ -f "$STATE/$1.pid" ]] && kill -0 "$(cat "$STATE/$1.pid")" 2>/dev/null; }

stop() {
  for name in app vite; do
    if alive "$name"; then
      kill "$(cat "$STATE/$name.pid")" 2>/dev/null || true
      log "stopped $name"
    fi
    rm -f "$STATE/$name.pid"
  done
  rm -f "$SOCK"
}

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'
}

# The checkout of this repo (main or another worktree) that built the app most recently.
donor() {
  git -C "$ROOT" worktree list --porcelain | sed -n 's/^worktree //p' | while IFS= read -r w; do
    if [[ "$w" != "$ROOT" && -x "$w/target/debug/claude-code-manager-app" ]]; then
      printf '%s\t%s\n' "$(stat -f %m "$w/target/debug/claude-code-manager-app")" "$w"
    fi
  done | sort -rn | head -1 | cut -f2
}

# A fresh worktree starts from the donor's caches: APFS clones are instant and copy-on-write,
# and cargo then rebuilds only the workspace crates instead of every dependency.
seed() {
  local from lock=crates/app/frontend/package-lock.json
  [[ -z "${CARGO_TARGET_DIR:-}" && ! -e "$ROOT/target" ]] || [[ ! -e "$FRONTEND_DIR/node_modules" ]] || return 0
  from="$(donor)"
  [[ -n "$from" ]] || return 0
  if [[ -z "${CARGO_TARGET_DIR:-}" && ! -e "$ROOT/target" ]]; then
    log "cloning $from/target"
    cp -c -Rp "$from/target" "$ROOT/target" 2>/dev/null || rm -rf "$ROOT/target"
  fi
  if [[ ! -e "$FRONTEND_DIR/node_modules" && -d "$from/crates/app/frontend/node_modules" ]] &&
    cmp -s "$from/$lock" "$ROOT/$lock"; then
    log "cloning $from node_modules"
    cp -c -Rp "$from/crates/app/frontend/node_modules" "$FRONTEND_DIR/node_modules" 2>/dev/null ||
      rm -rf "$FRONTEND_DIR/node_modules"
  fi
}

build() {
  mkdir -p "$STATE"
  seed
  if [[ ! -x "$FRONTEND_DIR/node_modules/.bin/vite" ]]; then
    log "installing frontend dependencies"
    npm --prefix "$FRONTEND_DIR" install --no-audit --no-fund >"$STATE/npm.log" 2>&1
  fi
  log "building"
  if ! (cd "$ROOT" && cargo build -p claude-code-manager-app >"$STATE/build.log" 2>&1); then
    tail -40 "$STATE/build.log" >&2
    log "build failed (full log: $STATE/build.log)"
    if alive app; then log "the running copy keeps its previous build"; fi
    exit 1
  fi
}

start() {
  if alive app; then
    log "already running (scripts/agent-app.sh restart to rebuild)"
    return 0
  fi
  stop
  mkdir -p "$STATE"
  rm -rf "$CONFIG_DIR"
  mkdir -p "$CONFIG_DIR"

  build

  local port
  port="$(free_port)"
  (cd "$FRONTEND_DIR" && CM_PORT="$port" exec nohup node_modules/.bin/vite >"$STATE/vite.log" 2>&1) &
  echo $! >"$STATE/vite.pid"
  for _ in $(seq 120); do
    curl -s -o /dev/null "http://localhost:$port" && break
    alive vite || { tail -20 "$STATE/vite.log" >&2; log "vite exited"; exit 1; }
    sleep 0.5
  done
  echo "$port" >"$STATE/port"

  if command -v yabai >/dev/null; then
    yabai -m rule --remove ccm-agent >/dev/null 2>&1 || true
    yabai -m rule --add label=ccm-agent title='^CCM Agent$' manage=off >/dev/null 2>&1 || true
  fi

  # The app's terminals start claude, which would otherwise inherit the launching Claude Code
  # session's markers: no transcript, that session's permission mode and messaging socket.
  local scrub=() v
  for v in $(compgen -e); do
    if [[ "$v" == CLAUDE* ]]; then scrub+=(-u "$v"); fi
  done
  env "${scrub[@]}" CM_AGENT=1 CM_AGENT_URL="http://localhost:$port" CLAUDE_CODE_MANAGER_CONFIG_DIR="$CONFIG_DIR" \
    nohup "$APP_BIN" >"$STATE/app.log" 2>&1 &
  echo $! >"$STATE/app.pid"

  for _ in $(seq 60); do
    [[ -S "$SOCK" ]] && break
    alive app || { tail -20 "$STATE/app.log" >&2; log "app exited"; exit 1; }
    sleep 0.5
  done
  [[ -S "$SOCK" ]] || { log "no bridge socket after 30s (log: $STATE/app.log)"; exit 1; }
  "$ROOT/scripts/ui" wait '#root > *' --timeout 30 >/dev/null
  # Hidden pages freeze rAF and throttle timers; ready means the ghost window is ordered in.
  for _ in $(seq 40); do
    [[ "$("$ROOT/scripts/ui" eval 'document.visibilityState')" == visible ]] && break
    sleep 0.25
  done
  log "ready: vite :$port, config $CONFIG_DIR"
}

case "${1:-}" in
  start) start ;;
  stop) stop ;;
  restart) build; stop; start ;;
  status)
    if alive app; then echo "running (vite :$(cat "$STATE/port"), state $STATE)"; else echo "stopped"; exit 1; fi
    ;;
  logs) tail -n "${2:-50}" "$STATE/app.log" ;;
  *) echo "usage: $0 start|stop|restart|status|logs [n]" >&2; exit 3 ;;
esac
