#!/usr/bin/env bash
# scripts/guided.sh against a fixture HOME, a fixture real root and a stub `claude`.
set -euo pipefail

GUIDED="$(cd "$(dirname "$0")/.." && pwd)/guided.sh"
CHECKOUT="$(cd "$(dirname "$0")/../.." && pwd)"
SID=11111111-2222-3333-4444-555555555555
TMPS=()

for v in $(compgen -e); do
  if [[ "$v" == CLAUDE* ]]; then unset "$v"; fi
done
unset XDG_CONFIG_HOME

cleanup() {
  local d
  for d in "${TMPS[@]+"${TMPS[@]}"}"; do chmod -R u+w "$d" 2>/dev/null || true; rm -rf "$d"; done
}

fail() { printf 'FAIL %s: %s\n' "${FUNCNAME[1]}" "$*" >&2; exit 1; }

# Fresh HOME with a real root holding two sessions (one with a subagent) and setup files.
fixture() {
  HOME="$(mktemp -d)"
  TMPS+=("$HOME")
  export HOME
  REAL="$HOME/.claude"
  SB="$HOME/.config/claude-code-manager/guided"
  local proj="$REAL/projects/-Users-x-my-proj-v2"
  mkdir -p "$proj/$SID/subagents" "$REAL/plugins/market" "$HOME/bin"
  printf '%s\n' \
    '{"type":"summary","summary":"hi"}' \
    '{"type":"user","cwd":"/Users/x/my proj.v2","message":{"content":"héllo"}}' \
    '{"type":"assistant","cwd":"/Users/x/my proj.v2","message":{"content":"ok"}}' >"$proj/$SID.jsonl"
  printf '%s\n' '{"type":"user","cwd":"/Users/x/my proj.v2","isSidechain":true}' >"$proj/$SID/subagents/agent-a1.jsonl"
  printf '%s\n' '{"type":"user","cwd":"/elsewhere"}' >"$proj/99999999-0000-0000-0000-000000000000.jsonl"
  echo '{"theme":"dark"}' >"$REAL/settings.json"
  echo '# mine' >"$REAL/CLAUDE.md"
  echo '{}' >"$REAL/plugins/market/plugin.json"
  cat >"$HOME/bin/claude" <<'STUB'
#!/usr/bin/env bash
env | grep '^CLAUDE' | sort >"$HOME/claude-env.log"
case "$1 $2" in
  "auth status")
    if grep -q stub-in "$CLAUDE_CONFIG_DIR/.claude.json" 2>/dev/null; then
      echo '{"loggedIn": true}'
    else
      echo '{"loggedIn": false}'; exit 1
    fi ;;
  "auth login") echo '{"stub-in":1}' >"$CLAUDE_CONFIG_DIR/.claude.json" ;;
  *) exit 9 ;;
esac
STUB
  chmod +x "$HOME/bin/claude"
  PATH="$HOME/bin:$BASE_PATH"
}
BASE_PATH="$PATH"

paths_follow_xdg_then_home() {
  fixture
  local out
  out="$("$GUIDED" paths)"
  [[ "$out" == "root=$SB"$'\n'"claude=$SB/claude"$'\n'"app=$SB/app" ]] || fail "home: $out"
  out="$(XDG_CONFIG_HOME="" "$GUIDED" paths | head -1)"
  [[ "$out" == "root=$SB" ]] || fail "empty xdg: $out"
  out="$(XDG_CONFIG_HOME="$HOME/xdg" CLAUDE_CODE_MANAGER_CONFIG_DIR=/nope "$GUIDED" paths | head -1)"
  [[ "$out" == "root=$HOME/xdg/claude-code-manager/guided" ]] || fail "xdg: $out"
  echo "ok paths_follow_xdg_then_home"
}

env_and_login_check_leave_contents_alone() {
  fixture
  mkdir -p "$SB/claude/projects" "$SB/app"
  echo keep >"$SB/claude/projects/seed.jsonl"
  echo '{"a":1}' >"$SB/app/config.json"
  local out
  out="$("$GUIDED" env)"
  eval "$out"
  [[ "$CLAUDE_CONFIG_DIR" == "$SB/claude" && "$CLAUDE_CODE_MANAGER_CONFIG_DIR" == "$SB/app" && "$CM_GUIDED" == 1 ]] ||
    fail "env: $out"
  unset CLAUDE_CONFIG_DIR CLAUDE_CODE_MANAGER_CONFIG_DIR CM_GUIDED
  if CLAUDE_CODE_SSE_PORT=1 "$GUIDED" login-check; then fail "stub is logged out but check passed"; fi
  [[ "$(cat "$HOME/claude-env.log")" == "CLAUDE_CONFIG_DIR=$SB/claude" ]] || fail "env leak: $(cat "$HOME/claude-env.log")"
  [[ "$(cat "$SB/claude/projects/seed.jsonl")" == keep && "$(cat "$SB/app/config.json")" == '{"a":1}' ]] ||
    fail "contents changed"
  echo "ok env_and_login_check_leave_contents_alone"
}

login_flag_logs_in_then_rechecks() {
  fixture
  local out
  out="$("$GUIDED" login-check --login)" || fail "login did not pass"
  [[ "$out" == *"Guided mode needs its own Claude login (one time only)."* ]] || fail "notice: $out"
  out="$("$GUIDED" login-check --login)" || fail "second check failed"
  [[ -z "$out" ]] || fail "second run spoke: $out"
  echo "ok login_flag_logs_in_then_rechecks"
}

reset_keeps_login_and_mirrors_setup() {
  fixture
  mkdir -p "$SB/claude/projects/p" "$SB/app"
  echo '{"stub-in":1}' >"$SB/claude/.claude.json"
  echo '{}' >"$SB/claude/.credentials.json"
  echo x >"$SB/claude/projects/p/s.jsonl"
  echo x >"$SB/claude/.hidden"
  echo '{}' >"$SB/app/config.json"
  "$GUIDED" reset >/dev/null 2>&1 || fail "reset failed"
  [[ -f "$SB/claude/.claude.json" && -f "$SB/claude/.credentials.json" ]] || fail "login files lost"
  [[ ! -e "$SB/claude/projects" && ! -e "$SB/claude/.hidden" ]] || fail "old contents kept"
  [[ -z "$(ls -A "$SB/app")" ]] || fail "app not emptied"
  cmp -s "$REAL/settings.json" "$SB/claude/settings.json" || fail "settings.json"
  cmp -s "$REAL/CLAUDE.md" "$SB/claude/CLAUDE.md" || fail "CLAUDE.md"
  cmp -s "$REAL/plugins/market/plugin.json" "$SB/claude/plugins/market/plugin.json" || fail "plugins"
  [[ ! -e "$SB/claude/skills" ]] || fail "absent skills/ appeared"
  [[ "$(cat "$SB/owner")" == "$CHECKOUT" ]] || fail "owner: $(cat "$SB/owner")"
  "$GUIDED" login-check || fail "reset logged the user out"
  echo "ok reset_keeps_login_and_mirrors_setup"
}

reset_refuses_while_another_worktree_owns_it() {
  fixture
  local other
  other="$(mktemp -d)"
  TMPS+=("$other")
  mkdir -p "$SB/claude/projects"
  echo "$other" >"$SB/owner"
  echo x >"$SB/claude/projects/s.jsonl"
  local err rc=0
  err="$("$GUIDED" reset 2>&1 >/dev/null)" || rc=$?
  [[ $rc == 2 ]] || fail "exit $rc"
  [[ "$err" == *"$other"* ]] || fail "message: $err"
  [[ -f "$SB/claude/projects/s.jsonl" && "$(cat "$SB/owner")" == "$other" ]] || fail "sandbox touched"
  echo "ok reset_refuses_while_another_worktree_owns_it"
}

reset_takes_over_from_a_deleted_worktree() {
  fixture
  mkdir -p "$SB"
  echo "$HOME/gone-worktree" >"$SB/owner"
  "$GUIDED" reset >/dev/null 2>&1 || fail "refused"
  [[ "$(cat "$SB/owner")" == "$CHECKOUT" ]] || fail "owner not taken over"
  echo "ok reset_takes_over_from_a_deleted_worktree"
}

copy_rewrites_cwd_and_subagents() {
  fixture
  local dir="$SB/scratch/my proj.v2" enc out
  enc="$(printf %s "$dir" | sed 's/[^a-zA-Z0-9]/-/g')"
  out="$(CLAUDE_CONFIG_DIR="$HOME/not-real" "$GUIDED" copy "$SID")" || fail "copy failed"
  [[ "$out" == "$SB/claude/projects/$enc/$SID.jsonl" && -f "$out" ]] || fail "path: $out"
  [[ -d "$dir" ]] || fail "scratch dir not created"
  python3 -I - "$out" "$SB/claude/projects/$enc/$SID/subagents/agent-a1.jsonl" "$dir" <<'PY' || fail "cwd not rewritten"
import json, sys
*files, want = sys.argv[1:]
for f in files:
    lines = [json.loads(l) for l in open(f, encoding="utf-8")]
    assert all(l["cwd"] == want for l in lines if "cwd" in l), (f, lines)
    assert sum("cwd" in l for l in lines) >= 1
assert json.loads(open(files[0], encoding="utf-8").readline()) == {"type": "summary", "summary": "hi"}
PY
  grep -q 'héllo' "$out" || fail "non-ascii escaped"
  out="$("$GUIDED" copy "$SID" --cwd "$HOME/w d")" || fail "copy --cwd failed"
  [[ "$out" == "$SB/claude/projects/$(printf %s "$HOME/w d" | sed 's/[^a-zA-Z0-9]/-/g')/$SID.jsonl" ]] || fail "--cwd path: $out"
  grep -q "\"cwd\":\"$HOME/w d\"" "$out" || fail "--cwd not applied"
  if "$GUIDED" copy 00000000-dead-beef-0000-000000000000 2>/dev/null; then fail "missing id copied"; fi
  echo "ok copy_rewrites_cwd_and_subagents"
}

mirror_points_plugins_at_the_sandbox() {
  fixture
  mkdir -p "$REAL/plugins/cache/m/p" "$REAL/plugins/marketplaces/m" "$REAL/skills" "$REAL/projects/x"
  printf '{"version":2,"plugins":{"p@m":[{"installPath":"%s/plugins/cache/m/p","scope":"user"}]}}\n' "$REAL" \
    >"$REAL/plugins/installed_plugins.json"
  printf '{"m":{"installLocation":"%s/plugins/marketplaces/m","source":{"source":"github"}}}\n' "$REAL" \
    >"$REAL/plugins/known_marketplaces.json"
  ln -s "$REAL/plugins/cache/m/p" "$REAL/skills/abs"
  ln -s ../plugins/cache/m "$REAL/skills/rel"
  ln -s "$REAL/projects/x" "$REAL/skills/outside"
  "$GUIDED" reset >/dev/null 2>&1 || fail "reset"
  python3 -I - "$SB/claude" "$REAL" "$(cd "$REAL" && pwd -P)" <<'PY2' || fail "plugin state still names the real root"
import json, sys
sb, *reals = sys.argv[1:]
def strings(v):
    if isinstance(v, str): yield v
    elif isinstance(v, dict): yield from (s for x in v.values() for s in strings(x))
    elif isinstance(v, list): yield from (s for x in v for s in strings(x))
inst = json.load(open(sb + "/plugins/installed_plugins.json"))
known = json.load(open(sb + "/plugins/known_marketplaces.json"))
assert inst["plugins"]["p@m"][0]["installPath"] == sb + "/plugins/cache/m/p", inst
assert known["m"]["installLocation"] == sb + "/plugins/marketplaces/m", known
for s in [*strings(inst), *strings(known)]:
    assert not any(s.startswith(r) for r in reals), s
PY2
  [[ "$(readlink "$SB/claude/skills/abs")" == "$SB/claude/plugins/cache/m/p" ]] || fail "abs link: $(readlink "$SB/claude/skills/abs")"
  [[ "$(readlink "$SB/claude/skills/rel")" == ../plugins/cache/m ]] || fail "rel link changed"
  [[ ! -e "$SB/claude/skills/outside" && ! -L "$SB/claude/skills/outside" ]] || fail "link out of the mirror kept"
  echo "ok mirror_points_plugins_at_the_sandbox"
}

nothing_writes_the_real_root() {
  fixture
  local before
  before="$(find "$REAL" -exec stat -f '%N %z %m' {} + | sort)"
  chmod -R a-w "$REAL"
  "$GUIDED" reset >/dev/null 2>&1 || fail "reset"
  "$GUIDED" reset >/dev/null 2>&1 || fail "second reset over read-only mirror"
  "$GUIDED" copy "$SID" >/dev/null || fail "copy"
  "$GUIDED" settings '{"density":"compact"}' || fail "settings"
  chmod -R u+w "$REAL"
  [[ "$(find "$REAL" -exec stat -f '%N %z %m' {} + | sort)" == "$before" ]] || fail "real root changed"
  echo "ok nothing_writes_the_real_root"
}

settings_merges() {
  fixture
  "$GUIDED" settings '{"density":"compact","a":1}'
  "$GUIDED" settings '{"theme":"light","a":2}'
  python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); assert d=={"density":"compact","a":2,"theme":"light"}, d' \
    "$SB/app/config.json" || fail "merge"
  if "$GUIDED" settings '[1]' 2>/dev/null; then fail "accepted a non-object"; fi
  echo "ok settings_merges"
}

release_and_status() {
  fixture
  [[ "$("$GUIDED" status)" == *"owner=free"* ]] || fail "status before reset"
  "$GUIDED" reset >/dev/null 2>&1
  [[ "$("$GUIDED" status)" == *"owner=$CHECKOUT"* ]] || fail "status after reset"
  "$GUIDED" release 2>/dev/null
  [[ ! -e "$SB/owner" ]] || fail "release kept owner"
  echo /somewhere/else >"$SB/owner"
  "$GUIDED" release 2>/dev/null
  [[ "$(cat "$SB/owner")" == /somewhere/else ]] || fail "released someone else's sandbox"
  echo "ok release_and_status"
}

copy_rejects_unsafe_ids() {
  fixture
  local id rc
  for id in '../x' '*' "../../projects/-Users-x-my-proj-v2/$SID"; do
    rc=0
    "$GUIDED" copy "$id" --cwd "$HOME/w" >/dev/null 2>&1 || rc=$?
    [[ $rc == 3 ]] || fail "id $id: exit $rc"
  done
  [[ ! -e "$SB/claude/projects" && ! -e "$HOME/w" ]] || fail "rejected id still wrote output"
  echo "ok copy_rejects_unsafe_ids"
}

reset_drops_the_mirror_when_repointing_fails() {
  fixture
  mkdir -p "$REAL/plugins/cache/m/p"
  echo '{"version":2,' >"$REAL/plugins/installed_plugins.json"
  printf '{"m":{"installLocation":"%s/plugins/marketplaces/m"}}\n' "$REAL" >"$REAL/plugins/known_marketplaces.json"
  local rc=0
  "$GUIDED" reset >/dev/null 2>&1 || rc=$?
  [[ $rc != 0 ]] || fail "reset passed over malformed plugin state"
  if [[ -e "$SB/claude/plugins" ]] && grep -rqF -e "$REAL" -e "$(cd "$REAL" && pwd -P)" "$SB/claude/plugins"; then
    fail "sandbox plugins still name the real root"
  fi
  [[ ! -e "$SB/owner" ]] || fail "failed reset claimed the sandbox"
  echo "ok reset_drops_the_mirror_when_repointing_fails"
}

reset_clears_scratch() {
  fixture
  mkdir -p "$SB/scratch/old proj"
  echo x >"$SB/scratch/old proj/f"
  "$GUIDED" reset >/dev/null 2>&1 || fail "reset"
  [[ ! -e "$SB/scratch" ]] || fail "scratch kept: $(ls -A "$SB/scratch")"
  echo "ok reset_clears_scratch"
}

for t in paths_follow_xdg_then_home env_and_login_check_leave_contents_alone login_flag_logs_in_then_rechecks \
  reset_keeps_login_and_mirrors_setup mirror_points_plugins_at_the_sandbox reset_refuses_while_another_worktree_owns_it \
  reset_takes_over_from_a_deleted_worktree copy_rewrites_cwd_and_subagents nothing_writes_the_real_root \
  settings_merges release_and_status copy_rejects_unsafe_ids reset_drops_the_mirror_when_repointing_fails \
  reset_clears_scratch; do
  (trap cleanup EXIT; "$t")
done
echo "all ok"
