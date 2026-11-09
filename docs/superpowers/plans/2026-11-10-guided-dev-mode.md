# Guided Dev Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A second way to run the dev app — `make dev-guided` — that opens a normal, visible dev window on a sandbox (its own Claude Code folder and app settings) which the feature's Claude session prepares and then watches while it walks the user through the change in chat.

**Architecture:** The app learns to read sessions from `CLAUDE_CONFIG_DIR` (as `claude` itself does). The agent bridge's single `CM_AGENT` switch splits into "bridge on" (agent or guided) and "ghost" (agent only). A new `scripts/guided.sh` owns the sandbox: paths, login check, ownership, reset, copying sessions, settings. `scripts/dev.sh` gains a guided variant that takes its environment from `guided.sh env`; `scripts/ui --guided` drives the guided window. The `verify-ui` skill gains the hand-off.

**Tech Stack:** Rust (Tauri v2, objc2), bash, Python 3 (stdlib only, as `scripts/ui` already uses), Make.

**Spec:** `docs/superpowers/specs/2026-11-10-guided-dev-mode-design.md`

## Global Constraints

- Sandbox root: `${XDG_CONFIG_HOME:-$HOME/.config}/claude-code-manager/guided` (empty `XDG_CONFIG_HOME` counts as unset), resolved from that rule only — never from `CLAUDE_CODE_MANAGER_CONFIG_DIR`. Subfolders: `claude/` (→ `CLAUDE_CONFIG_DIR`), `app/` (→ `CLAUDE_CODE_MANAGER_CONFIG_DIR`).
- Launching guided mode never wipes anything; only `guided.sh reset` does.
- One guided test at a time: `reset` refuses while another existing worktree owns the sandbox.
- The real `~/.claude` (or a real `CLAUDE_CONFIG_DIR`) is only ever read by guided tooling.
- Guided launches strip every inherited `CLAUDE*` variable before setting their own.
- `TAURI_CONFIG` stays byte-identical to `scripts/dev.sh`'s default, `{"build":{"devUrl":"http://localhost:5173"}}`, whatever port Vite really uses; the real port reaches the app at runtime via `CM_DEV_URL`.
- Guided window title: `Claude Code Manager — guided`. Agent window title stays `CCM Agent`.
- `CM_AGENT` keeps its current meaning (bridge + ghost + Prohibited + `refuse_in_agent`). Guided mode is NOT `CM_AGENT`: delete/fork/status-plugin are allowed there.
- Code comments: 1–2 lines, the non-obvious why only, describing current state (`~/.claude/CLAUDE.md`).
- Work on a feature branch in its own worktree; squash-merge to `main` only after the user verifies.

## Review Focus

- A sandbox owned by a worktree that has since been deleted → `reset` takes it over instead of refusing forever. (Task 3 test `reset_takes_over_from_a_deleted_worktree`.)
- `CLAUDE_CONFIG_DIR` set but empty → the app falls back to `~/.claude`, like the other env lookups. (Task 1 test `claude_root_ignores_empty_config_dir`.)
- A copied session whose original cwd contains spaces/dots, and one with a subagent folder (`<id>/subagents/*.jsonl`) → lands under the scratch-dir encoding with every `cwd` rewritten, subagents included. (Task 3 test `copy_rewrites_cwd_and_subagents`.)
- The real root is read-only to every subcommand → run them against a fixture root with write permission removed and expect success. (Task 3 test `nothing_writes_the_real_root`.)
- Relaunch/quit mid-test → `guided.sh env` and `login-check` never touch sandbox contents. (Task 3 test `env_and_login_check_leave_contents_alone`.)

---

### Task 1: The app reads sessions from `CLAUDE_CONFIG_DIR` when set

**Files:**
- Modify: `crates/core/src/store/paths.rs:26-29` (+ tests in its `mod tests`)
- Modify: `crates/app/src/hooks.rs:243-246` (`claude_settings_path`)

**Interfaces:**
- Produces: `pub fn claude_root_from(config_dir: Option<std::ffi::OsString>, home: Option<PathBuf>) -> Option<PathBuf>`; `default_claude_root()` = `claude_root_from(env::var_os("CLAUDE_CONFIG_DIR"), dirs::home_dir())`.

- [ ] **Step 1: Write failing tests** in `paths.rs` `mod tests`:
  - `claude_root_prefers_config_dir`: `claude_root_from(Some("/sb".into()), Some("/h".into())) == Some("/sb".into())`
  - `claude_root_falls_back_to_home`: `claude_root_from(None, Some("/h".into())) == Some("/h/.claude".into())`
  - `claude_root_ignores_empty_config_dir`: `claude_root_from(Some("".into()), Some("/h".into())) == Some("/h/.claude".into())`
- [ ] **Step 2:** `cargo test -p claude-code-manager-core claude_root` → FAIL (function not defined).
- [ ] **Step 3:** Implement `claude_root_from`; rewrite `default_claude_root` on top of it; make `hooks::claude_settings_path()` return `default_claude_root()` joined with `settings.json` (keep its `"."` fallback when no root). Update the `default_claude_root` doc line to say it honours `CLAUDE_CONFIG_DIR`.
- [ ] **Step 4:** `cargo test --workspace` → all pass. Re-grep `crates/` for `".claude"` / `HOME` uses outside tests; any other real-root construction goes through `default_claude_root()` (the `$HOME/.claude/hooks/claude-state.sh` strings in `hooks.rs` tests are legacy fixture data — leave them).
- [ ] **Step 5:** Commit `feat: read sessions from CLAUDE_CONFIG_DIR when it is set`.

### Task 2: Split the bridge from the ghost (agent vs guided modes)

**Files:**
- Modify: `crates/app/src/agent_bridge.rs` (mode, prepare, plugin, start, title)
- Modify: `crates/app/src/lib.rs:98-120` (`run`)
- Modify: `crates/app/src/window.rs:29-40` (`build_window`)
- Modify: `scripts/agent-app.sh` (`CM_AGENT_URL` → `CM_DEV_URL`)

**Interfaces:**
- Produces (top level of `agent_bridge.rs`, outside the cfg'd `imp`):
  - `#[derive(Clone, Copy, PartialEq, Debug)] pub enum Mode { Off, Agent, Guided }`
  - `pub fn mode() -> Mode` — `Off` unless `cfg!(all(debug_assertions, target_os = "macos"))`; then `mode_from(env::var_os("CM_AGENT"), env::var_os("CM_GUIDED"))`.
  - `fn mode_from(agent: Option<OsString>, guided: Option<OsString>) -> Mode` — `CM_AGENT` wins over `CM_GUIDED`.
  - `pub fn window_title(mode: Mode) -> &'static str` — `Agent` → `"CCM Agent"`, `Guided` → `"Claude Code Manager — guided"`, `Off` → `"Claude Code Manager"`.
  - `enabled()` keeps its name and meaning: `mode() == Mode::Agent` (callers: `refuse_in_agent`, ghost decisions).
- Consumes: env `CM_DEV_URL` (replaces `CM_AGENT_URL`), `CM_GUIDED`.

- [ ] **Step 1: Write failing tests** (top-level `mod tests` in `agent_bridge.rs`): `mode_from(None,None)==Off`; `mode_from(Some(..),None)==Agent`; `mode_from(None,Some(..))==Guided`; `mode_from(Some(..),Some(..))==Agent`; `window_title(Mode::Guided)=="Claude Code Manager — guided"`; `refusal(false, …)` stays Ok (existing test).
- [ ] **Step 2:** `cargo test -p claude-code-manager-app agent_bridge` → FAIL.
- [ ] **Step 3:** Implement. Behaviour per mode:
  - `prepare(context, mode)`: both modes set every config window's title to `window_title(mode)` and apply `CM_DEV_URL` to `build.dev_url`; only `Agent` sets `visible=false`/`focus=false`.
  - `plugin(mode)`: init script in both; the `on_page_load` ghosting only in `Agent`.
  - `start(app, mode)`: `set_activation_policy(Prohibited)` only in `Agent`; the socket listener in both.
  - `lib.rs::run`: compute `let mode = agent_bridge::mode();` once; call prepare/plugin/start when `mode != Mode::Off`.
  - `window::build_window`: title `window_title(mode())`; `.visible/.focused(false)` only when `Agent`.
  - Non-macOS / release stubs take the same new parameters.
  - `agent-app.sh`: export `CM_DEV_URL` instead of `CM_AGENT_URL`.
- [ ] **Step 4:** `cargo test -p claude-code-manager-app` and `cargo clippy -p claude-code-manager-app` clean. Then `scripts/agent-app.sh restart` and confirm with `scripts/ui eval 'document.visibilityState'` → `visible`, `scripts/ui windows` shows the page URL on the agent's Vite port, and the app is still inactive (it must still be a ghost).
- [ ] **Step 5:** Commit `feat: the agent bridge can run without hiding the window (guided mode)`.

### Task 3: `scripts/guided.sh` — the sandbox

**Files:**
- Create: `scripts/guided.sh` (bash entry; JSON/JSONL work in an embedded or sibling `python3 -I` helper)
- Create: `scripts/tests/guided_test.sh` (self-contained; runs every subcommand against a fixture `HOME`)

**Interfaces:**
- Produces subcommands (all print to stdout, errors to stderr, non-zero exit on failure):
  - `guided.sh paths` → three lines `root=…`, `claude=…`, `app=…`.
  - `guided.sh env` → `export` lines for: `CLAUDE_CONFIG_DIR=<claude>`, `CLAUDE_CODE_MANAGER_CONFIG_DIR=<app>`, `CM_GUIDED=1`; creates the two folders if missing; touches nothing else.
  - `guided.sh login-check [--login]` → exit 0 if `CLAUDE_CONFIG_DIR=<claude> claude auth status` reports `"loggedIn": true`; otherwise exit 1, or with `--login` print `Guided mode needs its own Claude login (one time only).`, run `claude auth login` with the sandbox dir, and re-check. Runs `claude` with every inherited `CLAUDE*` variable stripped.
  - `guided.sh reset` → ownership check, then: delete everything in `claude/` except `.claude.json` and `.credentials.json`; delete everything in `app/`; mirror (read-only, `cp -c -R` with plain-copy fallback) from the real root: `settings.json`, `CLAUDE.md`, `plugins/`, `skills/`, `hooks/`, `agents/`, `commands/` (each only if present); write `root/owner` = this checkout's absolute path.
  - `guided.sh copy <session-id> [--cwd DIR]` → find `<real root>/projects/*/<id>.jsonl`; default `DIR` = `root/scratch/<basename of the session's first cwd>` (created); write `claude/projects/<encode(DIR)>/<id>.jsonl` with every line's `cwd` set to `DIR`; same for `<id>/subagents/*.jsonl` if present. `encode` = every non-alphanumeric char → `-` (same rule as `paths::encode_project_dir`). Prints the new path.
  - `guided.sh settings '<json>'` → shallow-merge the JSON object into `app/config.json` (create if missing).
  - `guided.sh release` → remove `root/owner` if this checkout owns it.
  - `guided.sh status` → paths, owner (or `free`), login state.
- Real root for reading = `${CLAUDE_REAL_ROOT:-$HOME/.claude}` (`CLAUDE_REAL_ROOT` exists only for tests); never derived from `CLAUDE_CONFIG_DIR`, which may already point at the sandbox.

- [ ] **Step 1: Write the failing test script** `scripts/tests/guided_test.sh` (fixture `HOME=$(mktemp -d)`, `XDG_CONFIG_HOME` unset, a fixture real root with two sessions, a fake `claude` on `PATH` for `login-check`). Cases, each a function printing `ok <name>` or failing loudly:
  - `paths_follow_xdg_then_home` (with and without `XDG_CONFIG_HOME`; empty counts as unset)
  - `env_and_login_check_leave_contents_alone` (seed a file in `claude/projects` and `app/`; run `env` and `login-check`; both files remain)
  - `reset_keeps_login_and_mirrors_setup` (`.claude.json` survives, `projects/` gone, `settings.json`/`CLAUDE.md`/`plugins/` mirrored, `owner` written)
  - `reset_refuses_while_another_worktree_owns_it` (owner = another existing dir → exit 2, message names it, sandbox untouched)
  - `reset_takes_over_from_a_deleted_worktree` (owner = nonexistent path → succeeds)
  - `copy_rewrites_cwd_and_subagents` (original cwd `/Users/x/my proj.v2`; every line's `cwd` == scratch dir; file under the encoded scratch dir; subagent file copied and rewritten)
  - `nothing_writes_the_real_root` (`chmod -R a-w` the fixture real root; run reset, copy, settings; all succeed)
  - `settings_merges` (two calls; both keys present)
- [ ] **Step 2:** `bash scripts/tests/guided_test.sh` → FAIL (script missing).
- [ ] **Step 3:** Implement `scripts/guided.sh` to the interface above.
- [ ] **Step 4:** `bash scripts/tests/guided_test.sh` → every case `ok`, exit 0.
- [ ] **Step 5:** Commit `feat: scripts/guided.sh owns the guided-mode sandbox`.

### Task 4: Launch and watch: `make dev-guided`, `scripts/ui --guided`

**Files:**
- Modify: `scripts/dev.sh` (guided branch before the Vite start)
- Modify: `Makefile` (`dev-guided` target + `.PHONY`)
- Modify: `scripts/ui` (`--guided` flag)

**Interfaces:**
- Consumes: `guided.sh env`, `guided.sh login-check --login`, `guided.sh paths` (Task 3); `CM_DEV_URL`, `CM_GUIDED` (Task 2).
- Produces: `make dev-guided` (runs `GUIDED=1 scripts/dev.sh`); `scripts/ui --guided <cmd>` targets `<app>/agent.sock` from `guided.sh paths`.

- [ ] **Step 1:** In `dev.sh`, when `GUIDED=1`: strip inherited `CLAUDE*` vars; `scripts/guided.sh login-check --login || exit 1` before anything starts; `eval "$(scripts/guided.sh env)"`; pick a free port (same helper as `agent-app.sh`) for Vite and export `CM_PORT` to it and `CM_DEV_URL=http://localhost:$port`, leaving `TAURI_CONFIG` at the 5173 string. Plain `make dev` paths unchanged.
- [ ] **Step 2:** `Makefile`: `dev-guided:` → `GUIDED=1 scripts/dev.sh`, with a two-line comment like its neighbours.
- [ ] **Step 3:** `scripts/ui`: `--guided` (global, before the subcommand) sets `SOCK` from `scripts/guided.sh paths`'s `app=` line + `/agent.sock`; usage text gains one line.
- [ ] **Step 4: Verify without a GUI window.** `bash -n scripts/dev.sh`; run `GUIDED=1 bash -x scripts/dev.sh` with `PATH` pointing `claude` at a stub that reports logged out and cancels login → script exits 1 before starting Vite or building, prints the login notice. Check `scripts/ui --guided windows` with no app running → the existing "no agent app at …/guided/app/agent.sock" error names the guided socket.
- [ ] **Step 5:** Commit `feat: make dev-guided runs the dev app on the guided sandbox`.

### Task 5: End-to-end check in the hidden copy, on a sandbox

**Files:**
- Modify: `scripts/agent-app.sh` (`start --sandbox` option: after the `CLAUDE*` scrub, set `CLAUDE_CONFIG_DIR` to the given dir)
- Modify: `.claude/skills/verify-ui/SKILL.md` (one line documenting `--sandbox`)

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces: `scripts/agent-app.sh start --sandbox DIR`.

- [ ] **Step 1:** Implement `--sandbox DIR` (also accepted by `restart`).
- [ ] **Step 2:** Build a throwaway sandbox: `export XDG_CONFIG_HOME=$(mktemp -d)` (so `guided.sh` puts its sandbox there, not in the real guided folder), `scripts/guided.sh reset`, `scripts/guided.sh copy <a real session id>` (reads the real `~/.claude`), and hand-write one two-line session into `$XDG_CONFIG_HOME/claude-code-manager/guided/claude/projects/<encoded scratch dir>/`.
- [ ] **Step 3:** `scripts/agent-app.sh restart --sandbox $XDG_CONFIG_HOME/claude-code-manager/guided/claude`; verify with `scripts/ui`: the footer reads `2/2 sessions`; the copied session opens in Manager; snapshot looks right; `scripts/ui logs` clean. Then `ls ~/.claude/projects | wc -l` is unchanged from before the step.
- [ ] **Step 4:** Stop the copy, delete the throwaway dirs, commit `feat: the agent copy can run on a sandbox`.

### Task 6: The hand-off in the skill, CLAUDE.md and FEATURES.md

**Files:**
- Modify: `.claude/skills/verify-ui/SKILL.md` (new section "Hand off to the user")
- Modify: `CLAUDE.md` (definition of done)
- Modify: `FEATURES.md` (one new entry under Conventions & dev notes, next to the Agent app entry)

- [ ] **Step 1:** Skill section, in this order: (1) `scripts/guided.sh reset` — if it refuses, tell the user which worktree holds the sandbox and stop; (2) prepare: `copy` the real sessions the test needs (standing permission — name them in the hand-off message), hand-write small sessions for exact cases, `settings` for the app; (3) tell the user exactly `make -C <this worktree> dev-guided` and wait for "it's up" — never run it yourself; (4) walk one step per message ("do X; you should see Y"), after each `scripts/ui --guided snap` and look; (5) on a bug: fix, `reset`+re-prepare only if the setup changed (say so), ask for ⌘⇧R; (6) when done, `scripts/guided.sh release`. Plus the rule: backend-only changes skip the hand-off; changes that only show on real data say so and give plain `make dev` steps.
- [ ] **Step 2:** `CLAUDE.md` definition of done gains step 4: the guided hand-off per the skill, with the same exceptions.
- [ ] **Step 3:** `FEATURES.md` entry (new capability): `make dev-guided`, the sandbox path, one-time login, prepare/hand-off, `scripts/ui --guided`.
- [ ] **Step 4:** Commit `docs: guided hand-off in verify-ui, CLAUDE.md and FEATURES.md`.

### Task 7: Live verification with the user (dogfood)

- [ ] **Step 1:** Follow the new skill section on this feature itself: `guided.sh reset`, copy two real sessions, set one non-default setting (e.g. density `compact`), then ask the user to run `make -C <worktree> dev-guided`.
- [ ] **Step 2:** First run: the user completes the one-time login. Walk them through: window title says guided; sidebar shows only the prepared sessions; the setting is applied; a new session in the terminal tab writes into the sandbox (check `ls <sandbox>/claude/projects`) and not `~/.claude`; ⌘⇧R keeps everything; quitting and re-running `make dev-guided` keeps everything and does not ask to log in again.
- [ ] **Step 3:** Confirm `make dev` still shows real data. `guided.sh release`. Report to the user; merge only on their go-ahead.
