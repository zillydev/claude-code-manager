---
name: verify-ui
description: Use before handing any claude-manager change to the user for verification, and whenever you need to see or click the real app UI. Starts a hidden copy of the app (scripts/agent-app.sh) and drives it with scripts/ui — screenshots, DOM, clicks, typing, console errors — without touching the user's own window or focus.
---

# Verify a change in the real app

The user does only the final check. Before you stop and ask for it, exercise the change yourself in the hidden agent copy of the app and fix what you find.

## The agent app

`scripts/agent-app.sh start` builds the debug binary and launches it with `CM_AGENT=1` (see `crates/app/src/agent_bridge.rs`):

- never activates, no Dock icon, its window is fully transparent, ignores the mouse and sits behind every window, so it never steals focus or moves the user's Space;
- runs its own Vite server from **this checkout**, so frontend edits hot-reload into it;
- uses a throwaway config dir (`/tmp/ccm-agent-<hash>/config`), so the user's config, bookmarks, session meta and status file are untouched.

It reads the user's real `~/.claude` sessions. Treat them as read-only: never press delete, fork, rename, surgery or anything that writes a session file, and don't start claude in the terminal tab unless the change is about it. As a backstop the agent copy refuses delete, fork and the status-plugin toggle (`agent_bridge::refuse_in_agent`), showing an error instead; guard any new command that writes `~/.claude` the same way.

| command | when |
|---|---|
| `scripts/agent-app.sh start` | once; a no-op when already running |
| `scripts/agent-app.sh restart` | after any Rust change (frontend changes hot-reload) |
| `scripts/agent-app.sh restart --sandbox DIR` (also `start`) | run on a sandbox Claude config dir (`CLAUDE_CONFIG_DIR=DIR`, e.g. from `scripts/guided.sh`) instead of the real `~/.claude` |
| `scripts/agent-app.sh stop` | when done |
| `scripts/agent-app.sh logs [n]` | backend stderr, panics |

Never run `make dev` yourself — that launches the user's own window.

### Parallel sessions

Each worktree runs its own copy: the state dir, socket, Vite port and build all follow the checkout path, so a worktree's `scripts/ui` only ever drives that worktree's copy, even while other sessions build or drive theirs. A fresh worktree's first `start` clones the newest sibling checkout's `target/` and `node_modules` (APFS copy-on-write, no extra disk) and is ready in about 15 s instead of a full cold build. Each running copy holds about 1 GB of memory, so `stop` it when you finish.

## Driving it: `scripts/ui`

Targets are a CSS selector, `text=Label` (innermost visible element with that text) or `X Y` in CSS px.

```sh
scripts/ui snap --width 1000          # PNG path (2x pixels); open it with Read and look
scripts/ui text [selector]            # visible text, cheaper than a screenshot
scripts/ui find 'text=Compact'        # matches with boxes
scripts/ui click 'text=Settings'      # real, trusted clicks (scrolls into view first)
scripts/ui click 'input[placeholder^="Filter"]'; scripts/ui type 'query'
scripts/ui key Escape; scripts/ui key cmd+k; scripts/ui key shift+Tab
scripts/ui key cmd+a; scripts/ui key Backspace   # ⌘ keys the page doesn't take run the app menu item, as in the real app
scripts/ui wait 'text=Loading…' --gone --timeout 30
scripts/ui eval 'document.querySelectorAll(".row").length'
scripts/ui eval --body 'const r = await something(); return r'
scripts/ui logs [--clear]             # console errors/warnings, uncaught errors, rejections
scripts/ui resize 900 700             # layout at other sizes
scripts/ui windows; scripts/ui -w win-1 snap
```

Known limits: hover is synthesized in JS (React enter/over handlers run, CSS `:hover` does not; hover-revealed buttons still take clicks); `document.hasFocus()` is false; right-click and native menus/dialogs are not driven.

## The loop

1. `scripts/agent-app.sh start` (or `restart` after Rust edits), then `scripts/ui logs --clear`.
2. Walk the feature the way the user would: every new control, the empty/loading/error states you can reach, and one neighbouring feature it could break.
3. After each step, `scripts/ui snap` and **look at the PNG**: layout, overflow, clipping, alignment, light text on light ground. Check `scripts/ui logs` for errors.
4. Fix, re-run, repeat until clean. A blank screen or a render loop shows up here, not in vitest.
5. Close the agent-app pass with a short report (not the user hand-off; that is the next section, "Hand off to the user"): what you exercised, what you fixed, the final screenshot(s), and only what you could not check (native menus, dialogs, real hover styling, multi-monitor, the PTY with a live claude).

## Hand off to the user

After the agent-app pass, the user still checks the feature by hand in a visible dev window on a sandbox (`${XDG_CONFIG_HOME:-$HOME/.config}/claude-code-manager/guided`, holding `claude/` and `app/`), so their real sessions and config are never touched. You prepare it and walk them through; they run the app. One guided test at a time.

Skip it for backend-only changes (nothing visible to click). When a change only shows on real data, say so and give plain `make dev` steps for the user to run on their own data instead.

1. `scripts/guided.sh reset`. If it refuses (exit 2), it names the worktree that holds the sandbox: tell the user which one and stop.
2. Prepare the sandbox: `scripts/guided.sh copy <session-id> [--cwd DIR]` for each real session the test needs (a standing permission; name those sessions in the hand-off message), hand-write small sessions for exact cases, and `scripts/guided.sh settings '<json>'` for app settings.
3. Tell the user to run exactly `make -C <this worktree's absolute path> dev-guided` (absolute: their terminal is rarely in the repo root) and wait for "it's up". Never run it yourself, as with `make dev`. The first run asks for a one-time `claude auth login` into the sandbox; launching never wipes anything.
4. Walk one step per message ("do X; you should see Y"). After each, `scripts/ui --guided snap` and look at their window before the next step.
5. On a bug: fix it, then `scripts/guided.sh reset` and re-prepare only if the setup changed (say so); for Rust changes ask for ⌘⇧R, which rebuilds and keeps the sandbox. After any `reset` while their window is open, also ask for ⌘⇧R (or a relaunch): the running app still holds the old config and session state and would write it back.
6. When done, or when abandoning the hand-off early, `scripts/guided.sh release`.
