# Multi-window support — design

**Date:** 2026-06-14
**Status:** Approved

## Problem

The user works on several features at once across multiple terminal sessions in
multiple macOS Spaces, and wants at least one Claude Manager window available in
each Space. Today the app opens exactly one window and has no way to spawn more.

## Goal

Let the user open additional, fully independent Claude Manager windows via the
native macOS menu and `Cmd+N`. Each window is self-contained: its own selected
session, find state, and live tail, with no cross-talk between windows. The user
places windows on Spaces using normal macOS gestures (drag to a Space, or
right-click the Dock icon → Options → Assign To).

## Non-goals

- Persisting/restoring the set of open windows across app restarts (YAGNI).
- Keeping the app alive with zero windows open. Closing the last window quits the
  app (current Tauri default); re-launching is trivial.
- Cloning the current window's session into a new window. New windows start fresh.

## Design

### 1. Window creation — native menu + Cmd+N

Replace Tauri's implicit default menu with an explicit application menu built in
the app `setup`/builder, then handle its events. The menu includes the **standard
macOS set** plus a custom **New Window** item, because installing a custom menu
*replaces* the OS default, and we must not regress clipboard/edit shortcuts.

Menu structure (macOS):

- **App menu** (named after the app): About, ─, Services, ─, Hide, Hide Others,
  Show All, ─, Quit — via `PredefinedMenuItem`.
- **File**: **New Window** (`CmdOrCtrl+N`, custom item id `new_window`),
  Close Window (`PredefinedMenuItem::close_window`, `Cmd+W`).
- **Edit**: Undo, Redo, ─, Cut, Copy, Paste, Select All — all
  `PredefinedMenuItem`s, so `Cmd+C/V/X/A/Z` keep working in the find bar and text
  inputs.
- **Window**: Minimize, Fullscreen — `PredefinedMenuItem`s.

The `new_window` handler builds a new `WebviewWindow` via
`WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))`
with the same dimensions/min-size as the configured first window, a unique
`label` from an atomic counter, and a small cascade offset so windows don't stack
exactly on top of each other.

**Window-label allocation** is the one piece of extractable pure logic: a helper
that, given the current set of in-use labels (or a monotonic counter), returns the
next free label (`win-1`, `win-2`, …; the bootstrap window keeps its configured
`main` label). This helper gets a unit test.

### 2. Per-window live-tail isolation (the core refactor)

The only shared backend state that breaks under multiple windows is the file
watcher. Today:

- `WatcherState` holds a single `Option<FileWatcher>`.
- `open_session` emits live-tail turns with `app.emit("session-updated", …)`, a
  **global broadcast** every window's listener receives.

Two windows would (a) clobber each other's single watcher slot and (b) cross-feed
each other's streamed turns. Fix by scoping per window label:

- `WatcherState` becomes a map keyed by window label:
  `watchers: HashMap<String, watcher::FileWatcher>` behind the existing `Mutex`.
- `open_session(window: WebviewWindow, …)` and `stop_watching(window: WebviewWindow, …)`
  take the calling window (Tauri injects it), and key the watcher by
  `window.label()`. Opening a session in a window drops only *that* window's prior
  watcher.
- The tail loop is handed the window label and emits with
  `app.emit_to(&label, "session-updated", &new_turns)` so only the originating
  window receives its turns. (`emit_to` dispatches solely to the target window's
  listeners, so no other window sees the event.)

Frontend: `useTranscript` subscribes via
`getCurrentWebviewWindow().listen("session-updated", …)` instead of the global
`listen`, making the per-window scoping explicit on both ends.

Everything else is already isolated for free:

- Frontend UI state (selected session, find bar, jump-to-match, Details/Compact
  toggles) is React state, per-webview.
- Read-only commands (`list_sessions`, `search_transcripts`) and copy-on-edit
  mutations (`fork_session`, `commit_session`, `rename_session`, `delete_session`,
  `delete_message`) are stateless and work unchanged from any window. An edit made
  in one window shows up in another after that window's existing `refresh()`.

### 3. Window-close cleanup

Closing a window must drop its watcher (and let its tail thread exit when the
channel sender drops). Two layers:

1. Frontend unmount already calls `stop_watching`; with per-window keying it now
   removes only the closing window's watcher.
2. Because a hard window close may not reliably run the JS unmount, also register
   an `on_window_event` handler that, on `WindowEvent::Destroyed`, removes the
   watcher for that window's label. This guarantees no leaked watcher/thread.

### 4. Data flow summary

```
Cmd+N / File→New Window
   └─ menu event "new_window"
        └─ WebviewWindowBuilder(label = next free)  ──>  new window loads index.html (fresh state)

open_session(window, …)            [per window]
   └─ watchers[window.label()] = FileWatcher
        └─ tail_loop(label) ── app.emit_to(label, "session-updated") ──> that window only
                                                                          └─ getCurrentWebviewWindow().listen(...)

window closed
   └─ frontend unmount → stop_watching(window) → watchers.remove(label)
   └─ (backstop) on_window_event Destroyed → watchers.remove(label)
```

## Affected files

- `crates/webui/src/lib.rs` — build the application menu, register the
  `new_window` menu-event handler and `on_window_event` cleanup handler;
  introduce the window-label allocator + atomic counter state.
- `crates/webui/src/commands.rs` — `WatcherState` → keyed map; `open_session` and
  `stop_watching` take `WebviewWindow`; `tail_loop` emits with `emit_to(label, …)`.
- `crates/webui/frontend/src/hooks/useSessions.ts` — `useTranscript` listens via
  `getCurrentWebviewWindow().listen`.
- Possibly `crates/webui/tauri.conf.json` — only if a menu/window setting needs
  declaring; menu is built in Rust, so likely no change.

No `claude-manager-core` changes expected.

## Testing & verification

- **Unit test:** the window-label allocator (next-free-label given in-use set /
  counter).
- **Regression:** existing 61 core + 14 frontend tests stay green; `tsc` clean;
  webui builds.
- **Manual (the substantive verification, since menu/window/event glue needs a
  running app):**
  1. Launch the app; press `Cmd+N` → a second independent window opens.
  2. Open a *different* live session in each window; confirm each window only
     appends its own streamed turns (no cross-feed).
  3. Confirm `Cmd+C/V/A` work in the find bar in both windows (Edit menu intact).
  4. Close one window; confirm the other keeps tailing and the app survives.
  5. Drag a window to another Space; confirm it stays there and remains usable.
