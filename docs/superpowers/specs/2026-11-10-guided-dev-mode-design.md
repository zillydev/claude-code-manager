# Guided dev mode — design

## Problem

The user checks every change in `make dev` before `make install`. Today that means using the dev window on their real `~/.claude`: to see a feature they create sessions, flip settings and poke at the terminal, and the test sessions and settings changes are left behind as clutter in the app they use every day.

Features are now usually built by a Claude session in its own worktree, which already self-verifies in the hidden agent copy (`scripts/agent-app.sh`, `scripts/ui`, the `verify-ui` skill). What's missing is the hand-off: the user verifying the change live, with the session that built it setting up exactly what needs to be seen and walking them through it.

## Goals

- **Two ways to run the dev app.** `make dev` is unchanged: the user's real sessions, real settings, current code. A new guided mode runs a visible dev window on a sandbox instead.
- **Sandbox, not real data.** In guided mode the app reads and writes sessions only in a sandbox Claude Code folder and keeps its own settings only in a sandbox config dir. Nothing lands in the user's real `~/.claude` or real app config, including transcripts of a `claude` started in the guided window's terminal tab.
- **The feature's session prepares the test.** Before the hand-off it fills the sandbox with the sessions the test needs (copies of the user's real sessions, which the user explicitly allowed, and/or small hand-written ones) and writes the sandbox app settings the test needs.
- **The user starts it.** The session never opens the window itself; it tells the user the exact command to run (from the feature's worktree) and waits for "it's up".
- **Claude walks the user through it in chat.** One step at a time ("click X; you should see Y"), and after each step Claude looks at the window itself (screenshot/DOM via the bridge) to confirm, or to notice the user is somewhere else.

## Non-goals

- No in-app guide UI: instructions live only in Claude's chat.
- No change to plain `make dev`, `make install`, or the hidden agent copy's behavior.
- No automation of the user's clicks in guided mode — the user drives; Claude watches. (The bridge may still expose input commands; the walkthrough doesn't rely on them.)

## Anti-goals (it fails even if it works)

- Anything from a guided run showing up in the user's real session list, real config, or real Claude Code plugins/settings.
- The session launching or relaunching a window on the user (they hate windows popping up across Spaces).
- Having to log in to Claude Code more than once.
- Losing the prepared test setup when the user relaunches mid-test (e.g. after a fix).

## Decisions

- **Sandbox location: one permanent folder, outside any worktree** (builder's call on the exact path; e.g. `~/.ccm-guided/`), holding two subfolders:
  - a Claude Code config dir (used as `CLAUDE_CONFIG_DIR`), and
  - an app config dir (used as `CLAUDE_CODE_MANAGER_CONFIG_DIR`).
  It is permanent because macOS ties Claude Code's login (keychain) to the config-dir path: verified 2026-11-10 that `claude` with a fresh `CLAUDE_CONFIG_DIR` says "Not logged in · Please run /login". One fixed path means the user logs in once, ever, and every worktree's guided run shares that login.
- **One guided test at a time** (user's answer: they never need to verify two features at once). The sandbox records which worktree's session prepared it; a second session's reset refuses and tells its user the sandbox is in use (and by which worktree) instead of wiping another walkthrough. There is a single guided bridge socket for the same reason. Releasing it: the owning session resets or explicitly releases it when the walkthrough ends (builder's call on the mechanism; a stale owner whose worktree no longer exists can be taken over).
- **First run logs in.** If the sandbox `claude` isn't logged in, guided mode (or a setup command it points to) runs `claude` with the sandbox `CLAUDE_CONFIG_DIR` so the user can `/login` once.
- **Reset is a deliberate step, never a launch side effect.** Launching guided mode never wipes anything, so quitting or Rebuild & Relaunch (⌘⇧R) mid-test resumes exactly where the user was. The sandbox is reset only when the feature's session prepares a test (at hand-off, or when it re-seeds after a fix and says so). Reset clears sessions, project data and app settings but must keep whatever holds the login — the builder must verify which files in a Claude Code config dir are login state (the keychain entry is keyed by path; `.claude.json` holds account info) so a reset never logs the user out.
- **App honours `CLAUDE_CONFIG_DIR`.** `claude_code_manager_core::store::paths::default_claude_root()` returns `~/.claude` unconditionally; it becomes `CLAUDE_CONFIG_DIR` when set, falling back to `~/.claude`. The two spots in `crates/app/src/hooks.rs` that build `$HOME/.claude/...` themselves follow the same rule. Every other `~/.claude` reader goes through `default_claude_root()` (as of `main` 41b8b4a); the builder should re-grep. Because `claude` itself reads `CLAUDE_CONFIG_DIR`, a session started in the guided window's terminal inherits it and writes its transcript into the sandbox, and `claude plugin` calls from Settings act on the sandbox too.
- **Delete, fork and the status-plugin toggle work in guided mode.** The `refuse_in_agent` guard stays tied to `CM_AGENT` only; guided mode is sandboxed, so these are safe to test there.
- **Bridge without the ghost.** Today `CM_AGENT=1` turns on both the unix-socket bridge and the invisible/never-activating window. Guided mode needs the bridge with a normal, visible, normally-activating window. Split the switch (e.g. one env var for the bridge, one for the ghost); `CM_AGENT` keeps meaning both. `scripts/ui` must be able to target the guided window (builder's call how — e.g. a flag or env var naming the guided socket).
- **Launcher: `scripts/dev.sh`, guided variant.** Guided mode reuses `dev.sh`'s loop so ⌘⇧R works and the mode survives relaunch (the loop's exported env is inherited by each relaunch). It sets the two sandbox dirs and the bridge switch, and picks its own free Vite port so it can run alongside a normal `make dev`. It strips inherited `CLAUDE*` variables first (same reason as `scripts/agent-app.sh`: a launch from inside a Claude Code session would otherwise leak that session's markers into the terminal tab's `claude`) — then sets `CLAUDE_CONFIG_DIR`.
- **Command name** (builder's call, keep it short): e.g. `make dev-guided`, run from the feature's worktree so it builds that worktree's code: `make -C <worktree> dev-guided`.
- **Visibly different window.** The guided window's title says so (e.g. "Claude Code Manager — guided") so the user always knows which window is on real data.
- **Preparing a test.** The session needs a small, scriptable way to: reset the sandbox; copy a real session (by id) into the sandbox; and write sandbox app settings (`config.json`, `session-meta.json`). Builder's call whether this is a script (e.g. `scripts/guided.sh reset|copy <id>|...`) or documented steps in the skill; copying must only ever read the real `~/.claude`.
- **Copying real sessions is a standing permission** (user's answer: "copying is fine"). The session picks which real sessions to copy on its own and names them in its hand-off message; no per-copy confirmation.
- **Copies point at a scratch folder, not the real project** (my call). A copied session keeps its content but its working directory is rewritten to a scratch folder (every line's `cwd`, and the project folder name re-encoded to match), so resuming, forking or opening it in the guided terminal can never edit the real repo or write its `.claude/settings.local.json`. A `claude --resume` of the copy must still work.
- **The sandbox `claude` mirrors the user's Claude Code setup** (my call). Each prepare copies, read-only from the real `~/.claude`, the global `settings.json`, `CLAUDE.md` and the installed plugins' configuration into the sandbox config dir, so `claude` behaves as it does for the user (hooks, plugins such as `ccm-status`, permissions). Reset clears sessions and projects but these are re-copied at every prepare rather than preserved. The builder decides the exact file list and verifies it doesn't disturb login state.
- **The hand-off is part of the `verify-ui` skill.** After self-verifying in the hidden copy, the skill's new last step: prepare the sandbox, give the user the exact command, wait for "it's up", then walk the steps — one per message, each with what to do and what they should see — checking the window via the bridge after each; on a bug, fix it, re-seed if needed, say so, and ask the user to ⌘⇧R. When done, release the sandbox.
- **When the hand-off applies** (my call). `CLAUDE.md`'s definition of done: any change the user can see in the app ends with a guided hand-off. Backend-only changes skip it. Changes that only show with real data or real installs (behaviour across many real sessions, performance, update check, `make install`) say so explicitly and hand off with plain `make dev` steps instead of guided mode.

## Left to the builder

Exact sandbox path, env var names for the bridge/ghost split, how `scripts/ui` selects the guided window, the prepare tooling's shape, and the make target name — within the decisions above.

## Testing

- Unit: `default_claude_root()` with and without `CLAUDE_CONFIG_DIR`; the hooks paths follow it.
- Script-level: reset keeps login files and removes sessions/settings; copy reads only from the real root and lands in the sandbox layout; guided `dev.sh` exports the sandbox dirs and strips inherited `CLAUDE*` (stub binary, as in the rebuild-relaunch tests).
- In the hidden agent copy pointed at a sandbox (the agent copy can take the same env): the sidebar shows only sandbox sessions; a copied session opens; a new session's `claude` writes its transcript into the sandbox and not `~/.claude`.
- With the user (the only GUI check): first-run login once; a full guided walkthrough of a real feature; ⌘⇧R mid-test keeps the setup; plain `make dev` still shows real data.
