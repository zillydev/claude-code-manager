# Create a new session from a directory — design

**Date:** 2026-06-21
**Status:** Approved

## Problem

Add a way to **start a new Claude Code session** from the webui, in a chosen
working directory, dropping straight into its terminal. Two ways to pick the
directory: **paste a directory path**, or **use the currently-selected project**
(when the unified project selector has a concrete directory).

## Approach (decided empirically)

A new session = spawn a **fresh** `claude --session-id <uuid>` in the chosen cwd,
in a terminal (reusing the embedded-terminal PTY machinery). `claude` creates the
`.jsonl` itself, so we don't reverse-engineer its file format or path encoding.

Two alternatives were tested and **rejected**:

- **Pre-create the file + `--resume`:** `claude --resume` rejects an empty/hand-
  made file (`No conversation found`); a valid file needs CC's internal line
  structure (`queue-operation`/`attachment`/`user`/`assistant`/`last-prompt` with
  valid uuids+timestamps) — brittle.
- **Polling for the file before opening it in Manager:** unnecessary. A new
  session legitimately has no content until the first turn, so the Manager
  transcript is *meant* to be empty until then — nothing to poll for. The Terminal
  is the live view; the session appears in the sidebar/Manager after the first
  message via the existing refresh/open path.

## Prerequisite bug fix — `encode_project_dir`

`crates/core/src/store/paths.rs` `encode_project_dir` currently does
`cwd.replace('/', "-")`. **CC maps *every* non-alphanumeric char to `-`**
(verified: `/`, `.`, `_`, and space all → `-`; letters/digits preserved, case
kept). So our `session_file_path` computes the wrong directory for any cwd with a
dot/underscore/space. Fix:

```rust
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}
```

This is required so we can compute the new session's file path correctly, and it
also fixes a latent surgery bug (`session_file_path` for dotted/spaced cwds).

## Backend

### `encode_project_dir` (fix + tests) — `crates/core/src/store/paths.rs`

Unit-test the verified cases: `/Users/x/my.app` → `-Users-x-my-app`,
`/tmp/a_b c` → `-tmp-a-b-c`, `/Users/x/claude-manager` → `-Users-x-claude-manager`
(existing `-` preserved), `UPPER` kept.

### Fresh-vs-resume spawn — `crates/webui/src/terminal.rs`

Parameterize the argv by mode:

```rust
pub fn claude_args(session_id: &str, fresh: bool) -> Vec<String> {
    let flag = if fresh { "--session-id" } else { "--resume" };
    vec!["--dangerously-skip-permissions".into(), flag.into(), session_id.into()]
}
pub fn terminal_argv(session_id: &str, fresh: bool) -> Vec<String> { /* program + claude_args */ }
pub fn spawn_terminal(app, label, session_id, cwd, rows, cols, fresh: bool) -> io::Result<PtyHandle>
```

### Commands — `crates/webui/src/commands.rs`

- **`new_session(cwd: String) -> Result<{ session_id, file_path }, String>`:**
  validate `Path::new(&cwd).is_dir()` (else error); `let id = Uuid::new_v4();`
  `let file_path = paths::session_file_path(&root, &cwd, id)`; return both as
  strings. (Does NOT spawn — the terminal owns the spawn, so output isn't lost to
  a reconnect.)
- **`terminal_open` gains `new_cwd: Option<String>`:** if `Some(cwd)` → fresh
  spawn (`spawn_terminal(..., cwd, ..., fresh = true)`); if `None` → today's
  resume path (`first_cwd(read_raw(file_path))`, `fresh = false`). The existing
  "already running → no-op" guard stays first, so reconnects never re-spawn.

## Frontend

### State (App.tsx)

Add `terminalTarget: { sessionId: string; filePath: string; cwd?: string } | null`
— the session the Terminal tab shows. Set it:

- when an **existing** session is opened (sidebar select) → `{ sessionId, filePath, cwd: undefined }` (resume);
- when a **new** session is created → `{ sessionId, filePath, cwd }` (fresh).

`TerminalPane` is driven by `terminalTarget` instead of `transcript` directly.
The `cwd` flows through `TerminalPane` → `TerminalView`, which passes it as
`new_cwd` to `terminal_open` (so the first spawn is fresh; reconnects no-op).

### "+ New session" UI (Sidebar.tsx)

A **"+ New session"** button in the sidebar header opens a small popover:

- **Paste a directory path** — a text input + Create; trims input, requires
  non-empty.
- **Use the selected project** — shown only when the unified `filter` resolves to
  a concrete cwd: `dir:<cwd>` → that cwd; `name:<basename>` → that group's single
  cwd from `projectGroups` (unique names only); `""` → option hidden. One click
  creates the session there.

Sidebar takes an `onNewSession(cwd: string)` prop. App's handler: `invoke
("new_session", { cwd })` → set `terminalTarget` `{ session_id, file_path, cwd }`
→ `setActiveTab("terminal")` → `refresh()` the sidebar. Backend `is_dir`
validation surfaces as an inline error in the popover on reject.

### Manager view of a new session

Left empty/unchanged until the user's first turn writes the file; it then appears
in the sidebar (refresh), and clicking it opens it normally (read + live-tail).
No special plumbing.

## Edge cases

- **Close + reopen a new session's terminal** after it has a file: the reopen
  would pass `new_cwd` again (fresh) on a session that now exists. v1 doesn't
  guard this (closing a terminal isn't surfaced in the UI yet); acceptable.
- **Pasted path that isn't a directory:** `new_session` errors; the popover shows
  it. No partial state (nothing spawned).

## Testing

- **Unit (Rust):** `encode_project_dir` (the verified cases); `claude_args(id,
  fresh)` → `--session-id` vs `--resume`; `terminal_argv(id, fresh)` tail.
- **Frontend:** tsc + existing vitest green.
- **Manual (I won't launch the webui):** "+ New session" → paste a real dir →
  Terminal tab opens a fresh `claude` there; send a message → the `.jsonl` appears
  and the session shows in the sidebar (correct project dir); "use selected
  project" creates in the selected dir; a bogus path errors in the popover.

## Non-goals

- No native folder picker (paste a path, per the request; avoids the dialog
  plugin).
- No auto-open of the new session in Manager before its first turn.
- No per-terminal close button (separate TODO).
