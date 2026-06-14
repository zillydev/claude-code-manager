# Multi-window Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user open additional, fully independent Claude Manager windows (native macOS File → New Window / `Cmd+N`) so they can park one in each macOS Space, with each window's live tail isolated from the others.

**Architecture:** A new `window.rs` module provides a pure window-label allocator (unit-tested) and a `spawn_window` helper. `lib.rs` installs an explicit application menu (standard macOS App/File/Edit/Window set plus a custom New Window item) and a window-destroyed cleanup handler. `commands.rs` changes the single-slot file watcher into a per-window-label map and scopes live-tail events to the originating window via `emit_to`. The frontend listens to live-tail events through the current webview window instead of globally.

**Tech Stack:** Rust, Tauri v2 (`tauri::menu`, `WebviewWindowBuilder`, `Emitter::emit_to`), React 19 + TypeScript, `@tauri-apps/api/webviewWindow`.

---

## File Structure

- **Create** `crates/webui/src/window.rs` — pure `next_window_label(existing)` allocator (tested) + `spawn_window(app)` helper.
- **Modify** `crates/webui/src/commands.rs` — `WatcherState` becomes a `HashMap<String, FileWatcher>` keyed by window label; `open_session` / `stop_watching` take the calling `WebviewWindow`; `tail_loop` emits with `emit_to(label, …)`.
- **Modify** `crates/webui/src/lib.rs` — declare `mod window;`; build the application menu; register `on_menu_event` (New Window) and `on_window_event` (Destroyed → drop that window's watcher).
- **Modify** `crates/webui/frontend/src/hooks/useSessions.ts` — `useTranscript` subscribes via `getCurrentWebviewWindow().listen(...)`.

No `claude-manager-core` changes. No `tauri.conf.json` changes (menu is built in Rust).

---

## Task 1: Window-label allocator (pure logic, TDD)

**Files:**
- Create: `crates/webui/src/window.rs`
- Modify: `crates/webui/src/lib.rs:1` (add `mod window;`)
- Test: `crates/webui/src/window.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Create `window.rs` with the failing test and a stub**

Create `crates/webui/src/window.rs`:

```rust
//! Multi-window helpers: allocate a fresh window label and spawn an
//! independent webview window loading the same frontend.

use std::collections::HashSet;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Return the first `win-N` label (N starting at 1) not already in `existing`.
/// The bootstrap window keeps its configured `main` label, so `win-N` never
/// collides with it. Reusing freed labels keeps numbers from growing without
/// bound as windows open and close.
pub fn next_window_label(existing: &HashSet<String>) -> String {
    todo!()
}

/// Open a new, independent Claude Manager window loading the same frontend.
/// Windows cascade slightly so they don't stack exactly on top of each other.
pub fn spawn_window(app: &AppHandle) {
    let existing: HashSet<String> = app.webview_windows().keys().cloned().collect();
    let label = next_window_label(&existing);
    let offset = existing.len() as f64 * 28.0;

    if let Err(e) = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("claude-manager")
        .inner_size(1200.0, 800.0)
        .min_inner_size(800.0, 600.0)
        .position(120.0 + offset, 120.0 + offset)
        .resizable(true)
        .build()
    {
        eprintln!("failed to open new window: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_label_is_win_1() {
        let existing = HashSet::new();
        assert_eq!(next_window_label(&existing), "win-1");
    }

    #[test]
    fn skips_taken_labels_including_main() {
        let existing: HashSet<String> =
            ["main", "win-1", "win-2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(next_window_label(&existing), "win-3");
    }

    #[test]
    fn reuses_freed_label_in_the_middle() {
        // win-2 was closed; the next window reuses it instead of jumping to win-4.
        let existing: HashSet<String> =
            ["main", "win-1", "win-3"].iter().map(|s| s.to_string()).collect();
        assert_eq!(next_window_label(&existing), "win-2");
    }
}
```

Add the module declaration at the top of `crates/webui/src/lib.rs` (line 1 currently reads `mod commands;`):

```rust
mod commands;
mod window;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p claude-manager-webui next_window_label skips_taken_labels reuses_freed -- --nocapture`

Note: `cargo test` for this crate requires `crates/webui/frontend/dist` to exist (the Tauri context embeds it). It already does; if a `clean` wiped it, run `npm --prefix crates/webui/frontend run build` first.

Expected: the three `window::tests::*` tests FAIL/panic with `not yet implemented` (`todo!()`).

- [ ] **Step 3: Implement `next_window_label`**

Replace the `todo!()` body:

```rust
pub fn next_window_label(existing: &HashSet<String>) -> String {
    (1..)
        .map(|n| format!("win-{n}"))
        .find(|label| !existing.contains(label))
        .expect("an unbounded range always yields a free label")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p claude-manager-webui`
Expected: PASS — including `window::tests::first_label_is_win_1`, `skips_taken_labels_including_main`, `reuses_freed_label_in_the_middle`.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/src/window.rs crates/webui/src/lib.rs
git commit -m "feat(webui): window-label allocator + spawn_window helper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Per-window file-watcher isolation

**Files:**
- Modify: `crates/webui/src/commands.rs` (`WatcherState`, `open_session`, `stop_watching`, `tail_loop`)

There is no unit test here — these are Tauri command handlers that need a running app (the calling `WebviewWindow` is injected by Tauri at runtime). Correctness is checked by compilation in Step 4 and by the manual two-window test in Task 5. This is honest: the logic is glue, and a fake `AppHandle`/`WebviewWindow` cannot be constructed in a unit test.

- [ ] **Step 1: Make `WatcherState` a per-label map**

In `crates/webui/src/commands.rs`, add `use std::collections::HashMap;` near the other `use std::...` lines, then replace the struct and its `Default` impl (currently lines ~17-26):

```rust
/// Mutable state shared between tauri commands — one live-tail watcher per
/// window, keyed by the window's label, so multiple windows tail different
/// sessions without clobbering each other.
pub struct WatcherState {
    pub watchers: HashMap<String, watcher::FileWatcher>,
}

impl Default for WatcherState {
    fn default() -> Self {
        Self { watchers: HashMap::new() }
    }
}
```

- [ ] **Step 2: Scope `open_session` to the calling window**

Change the `open_session` signature to take the calling `WebviewWindow`, key the watcher by its label, and hand the label to the tail loop. Replace the whole `open_session` function (currently lines ~42-80) with:

```rust
#[tauri::command]
pub fn open_session(
    file_path: String,
    project_label: String,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<RenderedSession, String> {
    let session =
        store::open_session(file_path.clone().into(), project_label).map_err(|e| e.to_string())?;
    let rendered = render_session(&session);

    let label = window.label().to_string();

    // Start file watcher for live tail, scoped to THIS window.
    let state = app.state::<Mutex<WatcherState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;

    // Drop any previous watcher for this window only.
    guard.watchers.remove(&label);

    let (tx, rx) = channel::<()>();
    let bytes_read = session.bytes_read;
    let fp = file_path.clone();

    match watcher::watch_file(std::path::Path::new(&file_path), tx) {
        Ok(w) => {
            guard.watchers.insert(label.clone(), w);

            // Spawn a thread that reads from rx, debounces, tails the file,
            // renders new events, and emits them to the originating window.
            let app_clone = app.clone();
            std::thread::spawn(move || {
                tail_loop(rx, &fp, bytes_read, app_clone, label);
            });
        }
        Err(_) => {
            // Watcher failure is non-fatal; live tail just won't work.
        }
    }

    Ok(rendered)
}
```

- [ ] **Step 3: Scope `stop_watching` and `tail_loop`**

Replace `stop_watching` (currently lines ~82-88) with:

```rust
#[tauri::command]
pub fn stop_watching(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    let state = app.state::<Mutex<WatcherState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.watchers.remove(window.label());
    Ok(())
}
```

Change the `tail_loop` signature to accept the window label and emit only to that window. Update the function signature (currently lines ~90-95) to add a `label` parameter:

```rust
fn tail_loop(
    rx: std::sync::mpsc::Receiver<()>,
    file_path: &str,
    initial_offset: u64,
    app: AppHandle,
    label: String,
) {
```

…and change the emit call inside it (currently `let _ = app.emit("session-updated", &new_turns);`) to:

```rust
            let _ = app.emit_to(label.as_str(), "session-updated", &new_turns);
```

(`emit_to` is on the `tauri::Emitter` trait, already imported via `use tauri::{AppHandle, Emitter, Manager};`.)

- [ ] **Step 4: Build to verify the crate compiles**

Run: `cargo build -p claude-manager-webui`
Expected: compiles cleanly. (No warnings about unused `Emitter`/`Manager`.)

- [ ] **Step 5: Commit**

```bash
git add crates/webui/src/commands.rs
git commit -m "feat(webui): isolate live-tail watcher + events per window

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Application menu + New Window + window cleanup

**Files:**
- Modify: `crates/webui/src/lib.rs`

This wires up the menu and handlers. Like Task 2 it needs a running app to exercise fully; correctness is checked by compilation (Step 2) and the manual test in Task 5.

- [ ] **Step 1: Build the menu and register handlers**

Replace the entire contents of `crates/webui/src/lib.rs` with:

```rust
mod commands;
mod window;

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Manager, WindowEvent};

/// Build the application menu: the standard macOS App/Edit/Window items
/// (installing a custom menu replaces the OS default, so the standard
/// clipboard/edit shortcuts must be re-declared) plus a custom New Window item.
fn build_menu(handle: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let app_menu = Submenu::with_items(
        handle,
        "App",
        true,
        &[
            &PredefinedMenuItem::about(handle, None, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::services(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::hide(handle, None)?,
            &PredefinedMenuItem::hide_others(handle, None)?,
            &PredefinedMenuItem::show_all(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::quit(handle, None)?,
        ],
    )?;

    let new_window =
        MenuItem::with_id(handle, "new_window", "New Window", true, Some("CmdOrCtrl+N"))?;
    let file_menu = Submenu::with_items(
        handle,
        "File",
        true,
        &[&new_window, &PredefinedMenuItem::close_window(handle, None)?],
    )?;

    let edit_menu = Submenu::with_items(
        handle,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(handle, None)?,
            &PredefinedMenuItem::redo(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::cut(handle, None)?,
            &PredefinedMenuItem::copy(handle, None)?,
            &PredefinedMenuItem::paste(handle, None)?,
            &PredefinedMenuItem::select_all(handle, None)?,
        ],
    )?;

    let window_menu = Submenu::with_items(
        handle,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(handle, None)?,
            &PredefinedMenuItem::fullscreen(handle, None)?,
        ],
    )?;

    Menu::with_items(handle, &[&app_menu, &file_menu, &edit_menu, &window_menu])
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(commands::WatcherState::default()))
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "new_window" {
                window::spawn_window(app);
            }
        })
        .on_window_event(|win, event| {
            // When a window is destroyed, drop its live-tail watcher (the tail
            // thread exits when the channel sender is dropped). A backstop for
            // hard closes where the frontend's stop_watching may not run.
            if let WindowEvent::Destroyed = event {
                if let Some(state) = win.try_state::<Mutex<commands::WatcherState>>() {
                    if let Ok(mut guard) = state.lock() {
                        guard.watchers.remove(win.label());
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::open_session,
            commands::stop_watching,
            commands::search_transcripts,
            commands::open_for_edit,
            commands::commit_session,
            commands::fork_session,
            commands::delete_session,
            commands::rename_session,
            commands::delete_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build -p claude-manager-webui`
Expected: compiles cleanly.

If a `PredefinedMenuItem` constructor (e.g. `fullscreen`) is not available in the installed Tauri version, the compiler will name it; drop that one line from the Window submenu (it is a non-essential nicety) and rebuild. All other items (`minimize`, the Edit items, App items, `close_window`) are core and must stay.

- [ ] **Step 3: Commit**

```bash
git add crates/webui/src/lib.rs
git commit -m "feat(webui): app menu with New Window (Cmd+N) + per-window cleanup

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Frontend — scope live-tail listener to the current window

**Files:**
- Modify: `crates/webui/frontend/src/hooks/useSessions.ts`

- [ ] **Step 1: Import the current-window helper**

In `crates/webui/frontend/src/hooks/useSessions.ts`, change the event import (line 3) from:

```ts
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
```

to:

```ts
import { type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
```

- [ ] **Step 2: Subscribe via the current window**

In the `open` callback of `useTranscript`, replace the global `listen` call (currently lines ~54-67) with a window-scoped subscription. Replace:

```ts
      // Subscribe to live tail events.
      const unlisten = await listen<RenderedTurn[]>(
        "session-updated",
        (event) => {
          setTranscript((prev) => {
            if (!prev) return prev;
            return {
              ...prev,
              turns: [...prev.turns, ...event.payload],
              event_count: prev.event_count + event.payload.length,
            };
          });
        }
      );
      unlistenRef.current = unlisten;
```

with:

```ts
      // Subscribe to live tail events for THIS window only. The backend emits
      // with emit_to(<this window's label>), so each window receives only the
      // turns for the session it opened — no cross-window cross-feed.
      const unlisten = await getCurrentWebviewWindow().listen<RenderedTurn[]>(
        "session-updated",
        (event) => {
          setTranscript((prev) => {
            if (!prev) return prev;
            return {
              ...prev,
              turns: [...prev.turns, ...event.payload],
              event_count: prev.event_count + event.payload.length,
            };
          });
        }
      );
      unlistenRef.current = unlisten;
```

- [ ] **Step 3: Type-check and run frontend tests**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test`
Expected: `tsc` clean (no unused-`listen` error, since it's removed from the import); all frontend tests PASS (segments + search — they don't touch this hook).

- [ ] **Step 4: Commit**

```bash
git add crates/webui/frontend/src/hooks/useSessions.ts
git commit -m "feat(webui): scope live-tail listener to the current window

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Run the whole Rust test suite**

Run: `cargo test`
Expected: all workspace tests PASS — the existing 61 core tests plus the 3 new `window::tests::*` tests.

- [ ] **Step 2: Type-check + build the frontend**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test && npm run build`
Expected: `tsc` clean; 14 frontend tests PASS; `vite build` succeeds.

- [ ] **Step 3: Manual two-window verification**

Launch the app: `make webui` (from repo root).

Verify, checking each off:
1. Press `Cmd+N` (or File → New Window) → a second window opens, cascaded, with a fresh/empty session list selection.
2. In window A open one live session; in window B open a *different* live session that is actively being written. Confirm each window appends only its own streamed turns (no cross-feed between windows).
3. In each window's find bar (`Cmd+F`), confirm `Cmd+A`, `Cmd+C`, `Cmd+V` work (Edit menu intact).
4. Close window B (`Cmd+W`); confirm window A keeps tailing its session and the app stays alive.
5. Drag a window to another macOS Space; confirm it stays there and remains fully usable (select/open/fork sessions).

- [ ] **Step 4: Report results**

Report the exact `cargo test` and frontend test counts and the manual-check outcomes. Do not claim success without the output. Do not merge — the user directs all merges explicitly.

---

## Notes for the implementer

- **Do not** mutate `claude-manager-core` — all changes are in `crates/webui`.
- **Do not** change the copy-on-edit invariant: `fork`/`commit`/`rename`/`delete` are unchanged and remain correct from any window.
- `emit_to(label, …)` dispatches only to the window whose label matches; combined with `getCurrentWebviewWindow().listen`, this gives strict per-window isolation. Do not revert either half — both are required.
- The bootstrap window (from `tauri.conf.json`) has label `main`; spawned windows are `win-1`, `win-2`, … so they never collide.
