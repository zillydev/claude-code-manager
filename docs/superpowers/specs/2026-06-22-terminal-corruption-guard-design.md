# Terminal Corruption Guard Design

**Date:** 2026-06-22
**Status:** Approved
**TODOS.md item:** Concurrency → "shouldn't allow two claude terminal sessions to open of the same session as it could cause corruption"

## Goal

Guarantee **at most one `claude` PTY per session across the whole app**, so two
windows can never run two `claude --resume <same id>` processes writing the same
`.jsonl` (out-of-order / corrupt). When a window tries to open a session that's
already live elsewhere, **block and redirect** — show "already open in another
window" with a button to switch to it.

## Why the gap exists

The PTY pool is app-global (`Mutex<TerminalState>` shared by all windows) but
keyed `format!("{label}|{session_id}")` (`terminal.rs:37-39`) — the window label
is in the key, so window A's `main|X` and window B's `win-1|X` are different
entries → two PTYs → two processes on one file.

## Behavior

`terminal_open(session_id)` resolves to one of three outcomes:

- **owned by this window** → `reused` (today's "already running" no-op).
- **owned by another window** → `busy` — the caller does not spawn; the terminal
  pane shows *"This session is already open in another window"* + a **Switch to it**
  button (focuses the owner window — on macOS that jumps to its Space).
- **not running anywhere** → `started` — spawn it, owned by this window.

A window closing releases the PTYs it owns (already wired — see below), freeing
those sessions to be opened elsewhere.

## Backend design

### `crates/webui/src/terminal.rs`

- **Re-key the pool by `session_id` alone.** Drop `fn key(label, session_id)`.
  `TerminalState.ptys: HashMap<String /*session_id*/, PtyHandle>`.
- **Track the owner on the handle:** `PtyHandle` gains `owner: String` (the window
  label). `spawn_terminal` sets it from its `label` arg.
- **Pure, tested decision helper:**

  ```rust
  #[derive(Debug, PartialEq, Eq)]
  pub enum OpenDecision { Spawn, Reuse, Busy }

  /// Given the current owner of a session's PTY (if any) and the window asking to
  /// open it, decide whether to spawn, reuse the caller's own PTY, or refuse.
  pub fn open_decision(existing_owner: Option<&str>, requester: &str) -> OpenDecision {
      match existing_owner {
          None => OpenDecision::Spawn,
          Some(o) if o == requester => OpenDecision::Reuse,
          Some(_) => OpenDecision::Busy,
      }
  }
  ```

- **Method changes** (all keyed by `session_id` now; `label` dropped where it was
  only for keying):
  - `owner(&self, session_id) -> Option<&str>` (reads `handle.owner`).
  - `insert(&mut self, session_id, handle)` (owner already on the handle).
  - `write(&mut self, session_id, data)`, `resize(&self, session_id, rows, cols)`,
    `close(&mut self, session_id)`, private `remove_key(session_id)`.
  - `close_window(&mut self, label)` → remove+kill every handle whose
    `owner == label` (was: keys with the `{label}|` prefix).
  - `close_all` unchanged.
  - `has(...)` removed (superseded by `owner`).
- The reader thread's self-cleanup on EOF uses `remove_key(&session_id)`.

### `crates/webui/src/commands.rs`

- **`terminal_open` returns a result and holds the lock across the spawn** (so two
  windows racing the same fresh session can't both spawn — TOCTOU-free):

  ```rust
  #[derive(Serialize)]
  pub struct TerminalOpenResult {
      pub status: String,          // "started" | "reused" | "busy"
      pub owner: Option<String>,   // owner window label, when busy
  }
  ```

  Flow: resolve `(cwd, fresh)` first (file read, no pool access) → lock
  `TerminalState` → `match open_decision(guard.owner(&session_id), &label)`:
  - `Reuse` → return `{ "reused", None }`.
  - `Busy` → return `{ "busy", Some(owner) }`.
  - `Spawn` → `spawn_terminal(...)` then `guard.insert(&session_id, handle)` (under
    the same lock) → return `{ "started", None }`.

- **`terminal_input` / `terminal_resize` / `terminal_close`** drop the `label`
  arg from the state calls (key by `session_id`).
- **New command `focus_window(label, app)`** for the Switch button:

  ```rust
  #[tauri::command]
  pub fn focus_window(label: String, app: AppHandle) -> Result<(), String> {
      if let Some(w) = app.get_webview_window(&label) {
          w.set_focus().map_err(|e| e.to_string())?;
      }
      Ok(())
  }
  ```

  Registered in `lib.rs`'s `generate_handler!`.

### `crates/webui/src/lib.rs`

- The `WindowEvent::Destroyed` handler already calls
  `guard.close_window(win.label())` (lines 89-93) — no change to the call; only
  `close_window`'s implementation changes (owner-based). Register `focus_window`.

## Frontend design

`crates/webui/frontend/src/components/TerminalView.tsx`:

- The activation-effect `terminal_open` invoke now resolves to
  `{ status, owner }`. Replace the `.catch`-only handling with `.then`:
  - `status === "busy"` → `setBlocked({ owner })`.
  - real spawn error → keep the existing red `[failed to start: …]` write (`.catch`).
- New state `const [blocked, setBlocked] = useState<{ owner: string | null } | null>(null)`
  plus a `blockedRef` synced to it.
- **Gate input & resize when blocked:** in `onData` and the `ResizeObserver`
  callback, early-return if `blockedRef.current` — a blocked window must never
  write to or resize the owner's PTY (the pool is now session-keyed, so its
  `terminal_input`/`terminal_resize` would otherwise hit the owner's process).
- **Render:** when `blocked`, show a centered overlay (like the "Starting…" one)
  over the host: *"This session is already open in another window."* + a
  **Switch to it** button (shown when `blocked.owner`) calling
  `invoke("focus_window", { label: blocked.owner })`. Hide the "Starting claude…"
  overlay while `blocked` (gate it on `!blocked`).

The `TerminalOpenResult` is typed inline in `TerminalView` (no shared type needed).

## Edge cases

- **Race (two windows, same fresh session):** the app-global `Mutex` is held
  across decide+spawn+insert → the second caller sees the first's owner → `busy`.
- **Restart after exit:** the reader thread's `remove_key` on EOF frees the slot;
  the existing Restart button's `terminal_open` then `spawn`s (owner = same
  window). Unchanged from today.
- **Owner window closed:** `close_window(label)` kills+removes its PTYs → the
  session frees; another window can now open it.
- **External claude (real Ghostty) on the same session:** undetectable — out of
  scope. The guard covers claude-code-manager's own windows only.

## Testing

- **Unit (Rust):** `open_decision` — `None → Spawn`, `Some(self) → Reuse`,
  `Some(other) → Busy`. Pure; lives with the existing `terminal.rs` tests.
- The stateful command + frontend overlay are verified manually (open the same
  session in two windows → second shows the redirect; Switch focuses the first).
- `cargo test`, `tsc --noEmit`, `vitest run`, `vite build` all green.

## Non-goals

- No cross-window shared/mirrored terminal (that was the rejected "share" option).
- No detection of externally-run claude processes.
- Not the shared-state/caching work (separate concurrency project) — this is only
  the corruption guard.
