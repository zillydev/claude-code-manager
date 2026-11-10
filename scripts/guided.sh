#!/usr/bin/env bash
# The guided-mode sandbox: a fixed Claude Code config dir and app config dir that a visible dev
# window runs on. paths|env|login-check [--login]|reset|copy <id> [--cwd DIR]|settings <json>|release|status
set -euo pipefail

CHECKOUT="$(cd "$(dirname "$0")/.." && pwd)"
# Fixed path: the sandbox login lives in the keychain under it, so every worktree shares one login.
SANDBOX="${XDG_CONFIG_HOME:-$HOME/.config}/claude-code-manager/guided"
SB_CLAUDE="$SANDBOX/claude"
SB_APP="$SANDBOX/app"
OWNER="$SANDBOX/owner"
# Never CLAUDE_CONFIG_DIR: inside a guided run it already points at the sandbox.
REAL_ROOT="${CLAUDE_REAL_ROOT:-$HOME/.claude}"
MIRRORED=(settings.json CLAUDE.md plugins skills hooks agents commands)

log() { printf '\033[1;36m[guided]\033[0m %s\n' "$*" >&2; }

sandbox_claude() {
  local scrub=() v
  for v in $(compgen -e); do
    if [[ "$v" == CLAUDE* ]]; then scrub+=(-u "$v"); fi
  done
  env "${scrub[@]+"${scrub[@]}"}" CLAUDE_CONFIG_DIR="$SB_CLAUDE" claude "$@"
}

logged_in() {
  local out
  out="$(sandbox_claude auth status 2>/dev/null)" || true
  python3 -I -c 'import json, sys; sys.exit(0 if json.loads(sys.argv[1] or "{}").get("loggedIn") is True else 1)' \
    "$out" 2>/dev/null
}

login_check() {
  mkdir -p "$SB_CLAUDE"
  logged_in && return 0
  [[ "${1:-}" == --login ]] || return 1
  echo "Guided mode needs its own Claude login (one time only)."
  sandbox_claude auth login || true
  logged_in || { log "not logged in"; return 1; }
}

# Owner is the checkout that last reset the sandbox; one whose folder is gone can be taken over.
claim() {
  local owner=""
  [[ -f "$OWNER" ]] && owner="$(cat "$OWNER")"
  if [[ -n "$owner" && "$owner" != "$CHECKOUT" ]]; then
    if [[ -d "$owner" ]]; then
      log "the sandbox is in use by $owner; it must finish (scripts/guided.sh release) first"
      exit 2
    fi
    log "taking over from $owner, which no longer exists"
  fi
}

# Plugin state and symlinks in the mirror name real-root paths; left alone, the sandbox `claude`
# would update and prune plugins in the real root.
repoint_mirror() {
  python3 -I - "$REAL_ROOT" "$SB_CLAUDE" "${MIRRORED[@]}" <<'PY'
import json, os, sys

real, sandbox, *mirrored = sys.argv[1:]
reals = {real.rstrip("/"), os.path.realpath(real)}

def to_sandbox(path):
    for r in reals:
        if path == r or path.startswith(r + "/"):
            return sandbox + path[len(r):]
    return None

def fix(value):
    if isinstance(value, str):
        return to_sandbox(value) or value
    if isinstance(value, list):
        return [fix(v) for v in value]
    if isinstance(value, dict):
        return {k: fix(v) for k, v in value.items()}
    return value

for name in ("installed_plugins.json", "known_marketplaces.json"):
    path = os.path.join(sandbox, "plugins", name)
    if os.path.isfile(path):
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        with open(path, "w", encoding="utf-8") as f:
            json.dump(fix(data), f, indent=2, ensure_ascii=False)
            f.write("\n")

# A link into a mirrored entry follows its clone; one elsewhere in the real root is dropped.
for name in mirrored:
    for dirpath, dirnames, filenames in os.walk(os.path.join(sandbox, name)):
        for entry in dirnames + filenames:
            link = os.path.join(dirpath, entry)
            if not os.path.islink(link):
                continue
            target = os.path.join(dirpath, os.readlink(link))
            inside = to_sandbox(os.path.normpath(target)) or to_sandbox(os.path.realpath(target))
            if inside is None:
                continue
            os.unlink(link)
            if os.path.relpath(inside, sandbox).split(os.sep)[0] in mirrored:
                os.symlink(inside, link)
            else:
                print(f"dropped {link}: it points at {target} in the real root", file=sys.stderr)
PY
}

# Mirrored setup is cloned with the real root's modes, which may be read-only.
wipe() { chmod -R u+w "$@" 2>/dev/null || true; rm -rf "$@"; }

reset() {
  claim
  mkdir -p "$SB_CLAUDE" "$SB_APP"
  local f name src dst
  for f in "$SB_CLAUDE"/* "$SB_CLAUDE"/.[!.]* "$SB_CLAUDE"/..?*; do
    [[ -e "$f" || -L "$f" ]] || continue
    name="${f##*/}"
    [[ "$name" == .claude.json || "$name" == .credentials.json ]] || wipe "$f"
  done
  for f in "$SB_APP"/* "$SB_APP"/.[!.]* "$SB_APP"/..?*; do
    if [[ -e "$f" || -L "$f" ]]; then wipe "$f"; fi
  done
  wipe "$SANDBOX/scratch"
  for name in "${MIRRORED[@]}"; do
    src="$REAL_ROOT/$name" dst="$SB_CLAUDE/$name"
    [[ -e "$src" ]] || continue
    # -H: a symlinked settings.json must land as a file, or the sandbox would write through it.
    cp -c -R -H "$src" "$dst" 2>/dev/null || { wipe "$dst"; cp -R -H "$src" "$dst"; }
    chmod -R u+w "$dst"
  done
  if ! repoint_mirror; then
    # A half-repointed mirror would still name real-root paths; drop it rather than leave that.
    for name in "${MIRRORED[@]}"; do wipe "$SB_CLAUDE/$name"; done
    log "could not repoint the mirrored setup at the sandbox; removed it, reset failed"
    exit 1
  fi
  printf '%s\n' "$CHECKOUT" >"$OWNER"
  log "reset; owned by $CHECKOUT"
}

copy() {
  local id="${1:-}" dir="" src
  [[ -n "$id" ]] || { echo "usage: $0 copy <session-id> [--cwd DIR]" >&2; exit 3; }
  # Ids feed a glob and a path: no wildcards or `..`.
  [[ "$id" =~ ^[0-9A-Za-z-]+$ ]] || { log "invalid session id: $id"; exit 3; }
  shift
  if [[ "${1:-}" == --cwd ]]; then dir="${2:?--cwd needs a folder}"; fi
  local found=("$REAL_ROOT"/projects/*/"$id".jsonl)
  src="${found[0]}"
  [[ -f "$src" ]] || { log "no session $id under $REAL_ROOT/projects"; exit 1; }
  if [[ -z "$dir" ]]; then
    dir="$(python3 -I -c '
import json, sys
for line in open(sys.argv[1], encoding="utf-8"):
    try:
        cwd = json.loads(line).get("cwd")
    except (ValueError, AttributeError):
        continue
    if isinstance(cwd, str) and cwd:
        print(cwd.rstrip("/").rsplit("/", 1)[-1] or "root")
        break
' "$src")"
    [[ -n "$dir" ]] || { log "session $id records no cwd; pass --cwd"; exit 1; }
    dir="$SANDBOX/scratch/$dir"
  fi
  mkdir -p "$dir"
  dir="$(cd "$dir" && pwd)"
  python3 -I - "$src" "$dir" "$SB_CLAUDE/projects" "$id" <<'PY'
import json, os, re, sys

src, cwd, projects, sid = sys.argv[1:]
dest = os.path.join(projects, re.sub(r"[^A-Za-z0-9]", "-", cwd))

def rewrite(src_path, dest_path):
    os.makedirs(os.path.dirname(dest_path), exist_ok=True)
    with open(src_path, encoding="utf-8") as fin, open(dest_path, "w", encoding="utf-8") as fout:
        for line in fin:
            try:
                obj = json.loads(line)
            except ValueError:
                fout.write(line)
                continue
            if isinstance(obj, dict) and "cwd" in obj:
                obj["cwd"] = cwd
                line = json.dumps(obj, ensure_ascii=False, separators=(",", ":")) + "\n"
            fout.write(line)

rewrite(src, os.path.join(dest, sid + ".jsonl"))
subagents = os.path.join(os.path.dirname(src), sid, "subagents")
if os.path.isdir(subagents):
    for name in sorted(os.listdir(subagents)):
        if name.endswith(".jsonl"):
            rewrite(os.path.join(subagents, name), os.path.join(dest, sid, "subagents", name))
print(os.path.join(dest, sid + ".jsonl"))
PY
}

settings() {
  [[ -n "${1:-}" ]] || { echo "usage: $0 settings '<json object>'" >&2; exit 3; }
  mkdir -p "$SB_APP"
  python3 -I - "$SB_APP/config.json" "$1" <<'PY'
import json, os, sys

path, patch = sys.argv[1], json.loads(sys.argv[2])
if not isinstance(patch, dict):
    sys.exit("settings: expected a JSON object")
config = {}
if os.path.exists(path):
    with open(path, encoding="utf-8") as f:
        config = json.load(f)
config.update(patch)
with open(path + ".tmp", "w", encoding="utf-8") as f:
    json.dump(config, f, indent=2)
    f.write("\n")
os.replace(path + ".tmp", path)
PY
}

release() {
  if [[ -f "$OWNER" && "$(cat "$OWNER")" == "$CHECKOUT" ]]; then
    rm -f "$OWNER"
    log "released"
  elif [[ -f "$OWNER" ]]; then
    log "not released: owned by $(cat "$OWNER")"
  fi
}

status() {
  echo "root=$SANDBOX"
  echo "claude=$SB_CLAUDE"
  echo "app=$SB_APP"
  if [[ -f "$OWNER" ]]; then echo "owner=$(cat "$OWNER")"; else echo "owner=free"; fi
  if ! command -v claude >/dev/null; then echo "login=unknown (no claude on PATH)"
  elif logged_in; then echo "login=yes"
  else echo "login=no"; fi
}

case "${1:-}" in
  paths) printf 'root=%s\nclaude=%s\napp=%s\n' "$SANDBOX" "$SB_CLAUDE" "$SB_APP" ;;
  env)
    mkdir -p "$SB_CLAUDE" "$SB_APP"
    printf 'export CLAUDE_CONFIG_DIR=%q\nexport CLAUDE_CODE_MANAGER_CONFIG_DIR=%q\nexport CM_GUIDED=1\n' "$SB_CLAUDE" "$SB_APP"
    ;;
  login-check) login_check "${2:-}" ;;
  reset) reset ;;
  copy) shift; copy "$@" ;;
  settings) settings "${2:-}" ;;
  release) release ;;
  status) status ;;
  *) echo "usage: $0 paths|env|login-check [--login]|reset|copy <id> [--cwd DIR]|settings <json>|release|status" >&2; exit 3 ;;
esac
