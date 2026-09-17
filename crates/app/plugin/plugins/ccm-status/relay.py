#!/usr/bin/env python3
# claude-code-manager hook relay (bundled with the app as the ccm-status plugin).
import json, os, sys, time

d = os.environ.get("CLAUDE_CODE_MANAGER_CONFIG_DIR")
if not d:
    xdg = os.environ.get("XDG_CONFIG_HOME")
    base = xdg if xdg else os.path.join(os.environ.get("HOME", "."), ".config")
    d = os.path.join(base, "claude-code-manager")
OUT = os.path.join(d, "hook-events.ndjson")

try:
    p = json.load(sys.stdin)
except Exception:
    sys.exit(0)
line = json.dumps({
    "ts": round(time.time(), 2),
    "event": p.get("hook_event_name") or "",
    "sid": p.get("session_id") or "",
    "tool": p.get("tool_name") or "",
    "notif": p.get("notification_type") or "",
    "src": p.get("source") or "",
}, separators=(",", ":"))
os.makedirs(d, exist_ok=True)
try:
    if os.path.getsize(OUT) > 512_000:
        tail = open(OUT).readlines()[-1500:]
        open(OUT, "w").writelines(tail)
except OSError:
    pass
with open(OUT, "a") as f:
    f.write(line + "\n")
