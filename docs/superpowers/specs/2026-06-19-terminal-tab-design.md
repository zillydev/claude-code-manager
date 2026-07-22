# Embedded terminal tab (resume a session in a real PTY) — design

**Date:** 2026-06-19
**Status:** Approved (v1 scope)

## Problem / goal

Add a **top-level tab bar** to the claude-code-manager window: **Manager** (today's
sidebar + transcript, unchanged) and **Terminal**. The Terminal tab embeds a
**real terminal** that runs `claude --resume <the open session>` in that
session's working directory, so the user can drive the actual interactive Claude
Code TUI **exactly as in Ghostty** — full interactivity, permission prompts,
keybindings — with no custom chat UI to refine. (Supersedes the parked
`feat/chat-webui` custom-chat approach.)

## Approach

Standard integrated-terminal pattern, ported to Tauri: **xterm.js** in the
webview for rendering/input + a real **PTY** in the Rust backend via the
**`portable-pty`** crate (WezTerm) running `claude --resume <id>`. Bytes stream
PTY↔xterm over Tauri IPC/events; a resize call keeps the tty sized. Because
`claude` runs under a real tty, it behaves identically to a normal terminal.

**Synergy:** the Manager tab's existing live-tail keeps *reading* the session's
`.jsonl` while the Terminal's `claude` *writes* it — one writer, no conflict — so
the transcript updates live as the user works in the terminal.

## v1 scope

- Tabs: Manager | Terminal. Manager unchanged.
- Terminal runs `claude --dangerously-skip-permissions --resume <open
  session id>` in the session's launch cwd (`raw::first_cwd`). The bypass flag
  matches the user's shell alias; the entrypoint is the user's `claude-tracked`
  wrapper (`~/.local/bin/claude-tracked`) when present (it exports yabai window
  env for hooks, then `exec claude`), else `claude` on PATH. On exit, show a
  "restart" affordance.
- **One persistent terminal per session** (a pool). Each session you open a
  terminal for keeps its own PTY + xterm instance (xterm kept mounted, hidden
  when inactive) so scrollback and the live `claude` survive tab/session
  switches. Never auto-kills a running `claude` on switch.
- Input / output / resize wired. Cleanup on window close + app exit.
- Entrypoint: `claude-tracked` wrapper if present, else `claude` on PATH; always
  with `--dangerously-skip-permissions` (matches the user's alias).

## Architecture

### Backend (`crates/webui/src/terminal.rs`)

- Dep: `portable-pty` (+ `base64` for output framing).
- `pub fn terminal_argv(session_id: &str) -> Vec<String>` → `["claude",
  "--resume", <id>]` — **pure, unit-tested** seam.
- `TerminalState` managed state: `Mutex<HashMap<String, PtyHandle>>` keyed by
  `format!("{label}|{session_id}")` (per window + session — multi-window safe).
- `PtyHandle { master: Box<dyn MasterPty + Send>, writer: Box<dyn Write + Send>,
  child: Box<dyn Child + Send + Sync> }` (master kept for resize; writer for
  input; child to kill).
- `spawn`: `native_pty_system().openpty(PtySize{rows, cols, ..})` → build a
  `CommandBuilder` from `terminal_argv`, `cmd.cwd(cwd)`, `slave.spawn_command` →
  child; `master.try_clone_reader()` into a thread that reads chunks, base64-
  encodes, and `emit_to(label, "terminal-output", {sessionId, data})`; on EOF,
  `emit_to(label, "terminal-exit", {sessionId})` and remove the pool entry.
  `master.take_writer()` stored for input.

### Commands (`crates/webui/src/commands.rs`)

- `terminal_open(session_id, file_path, rows, cols, window, app)` — if the
  (label, session_id) key already has a live handle, no-op (return); else resolve
  `cwd = first_cwd(read_raw(file_path))`, spawn, store the handle.
- `terminal_input(session_id, data: String, window, app)` — `writer.write_all(data.as_bytes())`.
- `terminal_resize(session_id, rows, cols, window, app)` — `master.resize(PtySize{..})`.
- `terminal_close(session_id, window, app)` — `child.kill()`, drop the handle.

### lib.rs

- `mod terminal;`, `.manage(Mutex::new(terminal::TerminalState::default()))`,
  register the four commands, and in the `Destroyed` arm kill all of that
  window's terminals (iterate keys prefixed `"{label}|"`).

### Frontend

- Deps: `@xterm/xterm`, `@xterm/addon-fit`.
- `components/Tabs.tsx` (or inline in `App.tsx`): a `Manager | Terminal` bar
  driving an `activeTab` state. Manager content is the existing layout; Terminal
  content is `<TerminalPane sessionId={transcript?.id ?? null} filePath=… />`.
- `components/TerminalPane.tsx`: keeps a set of session ids that have a terminal;
  renders one `TerminalView` per id (all mounted), shows only the active id; if
  no session open, a placeholder.
- `components/TerminalView.tsx` (per session): on mount creates an xterm +
  FitAddon, fits, calls `terminal_open(sessionId, filePath, rows, cols)`,
  subscribes to `terminal-output` (filtering by `sessionId`) → `term.write(bytes)`
  (base64→Uint8Array), wires `term.onData → terminal_input`, and a ResizeObserver
  → fit → `terminal_resize`. On `terminal-exit`, shows an overlay "session ended
  — Restart" that re-calls `terminal_open`.

### Data flow

`xterm.onData(str) → invoke("terminal_input",{sessionId,data}) → writer`.
`reader thread → emit_to(label,"terminal-output",{sessionId, data: base64}) →
TerminalView (matching sessionId) → term.write(Uint8Array)`.
`ResizeObserver → fit.fit() → invoke("terminal_resize",{sessionId,rows,cols}) →
master.resize`.

## Components & files

- **Rust create:** `crates/webui/src/terminal.rs`.
- **Rust modify:** `commands.rs` (4 commands), `lib.rs` (module, manage,
  register, Destroyed cleanup), `Cargo.toml` (`portable-pty`, `base64`).
- **Frontend create:** `components/TerminalView.tsx`, `components/TerminalPane.tsx`
  (and a small tab bar — inline in `App.tsx` or `components/Tabs.tsx`).
- **Frontend modify:** `App.tsx` (tab state + render switch), `package.json`
  (`@xterm/xterm`, `@xterm/addon-fit`), `index.html` or a CSS import for xterm's
  stylesheet (`@xterm/xterm/css/xterm.css`).

## Testing

- **Unit (Rust):** `terminal_argv` → `["claude","--resume",<id>]`.
- **Frontend:** typecheck + existing vitest stay green; xterm behavior is manual.
- **Manual (I will NOT launch the webui — hand the user steps):**
  1. Open session A in Manager → Terminal tab → `claude --resume A` runs
     interactively; type a message, see the TUI respond; permission prompts work.
  2. While in the terminal, switch to Manager → the transcript updates live (the
     live-tail reading what the terminal's `claude` writes).
  3. Open session B → its own terminal; switch back to A → A's `claude` still
     live (not restarted).
  4. Resize the window → the TUI reflows.
  5. Close the window → terminals are killed (no orphan `claude` processes:
     `pgrep -fl 'claude --resume'`).

## Open considerations

- **Output framing:** PTY output is raw bytes; base64 per chunk → `Uint8Array` →
  `term.write` lets xterm reassemble partial UTF-8 across reads (a UTF-8-lossy
  string per chunk could corrupt multibyte glyphs). Input via `onData` is
  complete UTF-8 strings → write as bytes.
- **Multi-window:** keying by `{label}|{session_id}` means each window has its
  own terminal for a session; two windows resuming the *same* session would be
  two `claude` writers (an edge case — same hazard as opening it in Ghostty too;
  not guarded in v1).
- **Persistence:** PTYs live for the app's lifetime; xterm scrollback is held
  client-side by keeping instances mounted. App restart loses terminals (no
  daemon) — acceptable v1.
- **Claude not on PATH / spawn failure:** `terminal_open` returns an error the
  TerminalView surfaces as an overlay.

## Non-goals (v1) / phases

- **v2:** shell mode + a "resume here" action; configurable command/binary;
  auto-restart-on-exit toggle; xterm theming to match the app; split panes.
- **Out of scope:** reconnecting to terminals after an app restart (needs a
  persistent pty daemon); sharing one PTY across windows (tmux-style).
