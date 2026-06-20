# Embedded Terminal Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Manager | Terminal tab bar; the Terminal tab runs the real interactive `claude --resume <open session>` in a PTY (xterm.js + portable-pty), one persistent terminal per session.

**Architecture:** Rust `terminal.rs` owns a pool of PTYs keyed by `(window label, session id)`, each running `claude --resume <id>` in the session's launch cwd; a reader thread streams base64 chunks to the window. Frontend xterm.js panes (one per session, kept mounted) render output and send input/resize.

**Tech Stack:** Rust (`portable-pty`, `base64`), React/TS (`@xterm/xterm`, `@xterm/addon-fit`).

## Global Constraints

- Terminal command = `claude --resume <session_id>`, launched directly, cwd = `raw::first_cwd` of the session.
- PTY pool keyed by `format!("{label}|{session_id}")` (per window + session).
- Output framed as base64 per chunk; frontend decodes to `Uint8Array` for `term.write` (preserves partial UTF-8). Input is a UTF-8 string written as bytes.
- One persistent terminal per session; xterm instances stay mounted (hidden when inactive). Do NOT close a PTY on tab/session switch — only on window close, app exit, or explicit close.
- I (assistant) will NOT launch the webui — PTY/xterm behavior is verified manually. Unit test covers `terminal_argv`.
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Create** `crates/webui/src/terminal.rs` — `terminal_argv` (pure), `PtyHandle`, `TerminalState`, `spawn_terminal`.
- **Modify** `crates/webui/src/commands.rs` — `terminal_open`/`terminal_input`/`terminal_resize`/`terminal_close`.
- **Modify** `crates/webui/src/lib.rs` — module, manage state, register commands, kill window's terminals on Destroyed.
- **Modify** `crates/webui/Cargo.toml` — `portable-pty`, `base64`.
- **Frontend create** `components/TerminalView.tsx`, `components/TerminalPane.tsx`.
- **Frontend modify** `App.tsx` (tab bar + content switch), `main.tsx` (xterm css), `package.json` (xterm deps).

---

## Task 1: Backend — PTY pool + commands

**Files:**
- Create: `crates/webui/src/terminal.rs`
- Modify: `crates/webui/src/commands.rs`, `crates/webui/src/lib.rs`, `crates/webui/Cargo.toml`

**Interfaces:**
- Produces: `terminal_argv(session_id: &str) -> Vec<String>`; `TerminalState` (`has`/`insert`/`write`/`resize`/`close`/`close_window`/`remove`); `spawn_terminal(app, label, session_id, cwd, rows, cols) -> std::io::Result<PtyHandle>`; commands `terminal_open`/`terminal_input`/`terminal_resize`/`terminal_close`; events `terminal-output {sessionId,data}`, `terminal-exit {sessionId}`.
- Consumes: `paths`, `raw::{read_raw, first_cwd}` (already imported in commands.rs).

- [ ] **Step 1: Add deps to `crates/webui/Cargo.toml`**

Under `[dependencies]` (after `uuid`):

```toml
portable-pty = "0.8"
base64 = "0.22"
```

- [ ] **Step 2: Create `terminal.rs` with the pure builder + state + spawn**

```rust
use std::collections::HashMap;
use std::io::{Read, Write};

use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

/// The program + args for a session's terminal. Pure — unit-testable.
pub fn terminal_argv(session_id: &str) -> Vec<String> {
    vec!["claude".into(), "--resume".into(), session_id.into()]
}

fn key(label: &str, session_id: &str) -> String {
    format!("{label}|{session_id}")
}

/// A live PTY: master (resize), writer (input), child (kill).
pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct TerminalState {
    ptys: HashMap<String, PtyHandle>,
}

impl TerminalState {
    pub fn has(&self, label: &str, session_id: &str) -> bool {
        self.ptys.contains_key(&key(label, session_id))
    }
    pub fn insert(&mut self, label: &str, session_id: &str, handle: PtyHandle) {
        self.ptys.insert(key(label, session_id), handle);
    }
    pub fn write(&mut self, label: &str, session_id: &str, data: &[u8]) -> std::io::Result<()> {
        if let Some(h) = self.ptys.get_mut(&key(label, session_id)) {
            h.writer.write_all(data)?;
            h.writer.flush()?;
        }
        Ok(())
    }
    pub fn resize(&self, label: &str, session_id: &str, rows: u16, cols: u16) {
        if let Some(h) = self.ptys.get(&key(label, session_id)) {
            let _ = h.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }
    pub fn close(&mut self, label: &str, session_id: &str) {
        if let Some(mut h) = self.ptys.remove(&key(label, session_id)) {
            let _ = h.child.kill();
        }
    }
    pub fn close_window(&mut self, label: &str) {
        let prefix = format!("{label}|");
        let keys: Vec<String> =
            self.ptys.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        for k in keys {
            if let Some(mut h) = self.ptys.remove(&k) {
                let _ = h.child.kill();
            }
        }
    }
    fn remove_key(&mut self, k: &str) {
        self.ptys.remove(k);
    }
}

/// Spawn `claude --resume <id>` under a PTY in `cwd`; stream its output to the
/// window as base64 `terminal-output` events; emit `terminal-exit` on EOF.
pub fn spawn_terminal(
    app: AppHandle,
    label: String,
    session_id: String,
    cwd: String,
    rows: u16,
    cols: u16,
) -> std::io::Result<PtyHandle> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let argv = terminal_argv(&session_id);
    let mut cmd = CommandBuilder::new(&argv[0]);
    for a in &argv[1..] {
        cmd.arg(a);
    }
    cmd.cwd(&cwd);

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let k = key(&label, &session_id);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    let _ = app.emit_to(
                        label.as_str(),
                        "terminal-output",
                        json!({ "sessionId": session_id, "data": data }),
                    );
                }
            }
        }
        let _ = app.emit_to(
            label.as_str(),
            "terminal-exit",
            json!({ "sessionId": session_id }),
        );
        if let Some(state) = app.try_state::<std::sync::Mutex<TerminalState>>() {
            if let Ok(mut g) = state.lock() {
                g.remove_key(&k);
            }
        }
    });

    Ok(PtyHandle { master: pair.master, writer, child })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_argv_resumes_the_session() {
        assert_eq!(terminal_argv("abc-123"), vec!["claude", "--resume", "abc-123"]);
    }
}
```

- [ ] **Step 3: Register the module in `lib.rs`**

Add `mod terminal;` with the other modules at the top of `crates/webui/src/lib.rs`.

- [ ] **Step 4: Run the unit test (verifies the crate compiles + the builder)**

Run: `cargo test -p claude-manager-webui terminal_argv`
Expected: PASS (and the crate compiles with the new deps).

- [ ] **Step 5: Add the commands to `commands.rs`**

Add `use crate::terminal::{self, TerminalState};` near the top imports. Append:

```rust
/// Start (or no-op if already running) the terminal for a session in this window.
#[tauri::command]
pub fn terminal_open(
    session_id: String,
    file_path: String,
    rows: u16,
    cols: u16,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    let label = window.label().to_string();
    {
        let state = app.state::<Mutex<TerminalState>>();
        if state.lock().map_err(|e| e.to_string())?.has(&label, &session_id) {
            return Ok(());
        }
    }
    let lines = read_raw(std::path::Path::new(&file_path)).map_err(|e| e.to_string())?;
    let cwd = first_cwd(&lines).ok_or("could not determine the session's working directory")?;
    let handle = terminal::spawn_terminal(app.clone(), label.clone(), session_id.clone(), cwd, rows, cols)
        .map_err(|e| format!("failed to start terminal: {e}"))?;
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .insert(&label, &session_id, handle);
    Ok(())
}

#[tauri::command]
pub fn terminal_input(
    session_id: String,
    data: String,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    let label = window.label().to_string();
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .write(&label, &session_id, data.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn terminal_resize(
    session_id: String,
    rows: u16,
    cols: u16,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    let label = window.label().to_string();
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .resize(&label, &session_id, rows, cols);
    Ok(())
}

#[tauri::command]
pub fn terminal_close(
    session_id: String,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    let label = window.label().to_string();
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .close(&label, &session_id);
    Ok(())
}
```

- [ ] **Step 6: Wire `lib.rs` — manage state, register, cleanup**

After the existing `.manage(Mutex::new(commands::WatcherState::default()))`:

```rust
        .manage(Mutex::new(terminal::TerminalState::default()))
```

In the `WindowEvent::Destroyed` arm, after the watcher cleanup:

```rust
                if let Some(ts) = win.try_state::<Mutex<terminal::TerminalState>>() {
                    if let Ok(mut guard) = ts.lock() {
                        guard.close_window(win.label());
                    }
                }
```

Add to `invoke_handler![...]`:

```rust
            commands::terminal_open,
            commands::terminal_input,
            commands::terminal_resize,
            commands::terminal_close,
```

- [ ] **Step 7: Build the backend**

Run: `cargo build -p claude-manager-webui`
Expected: compiles (fetches `portable-pty`, `base64`).

- [ ] **Step 8: Commit**

```bash
git add crates/webui/src/terminal.rs crates/webui/src/commands.rs crates/webui/src/lib.rs crates/webui/Cargo.toml Cargo.lock
git commit -m "feat(webui): PTY pool + terminal_open/input/resize/close commands

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Frontend — tabs + xterm terminal

**Files:**
- Create: `crates/webui/frontend/src/components/TerminalView.tsx`, `crates/webui/frontend/src/components/TerminalPane.tsx`
- Modify: `crates/webui/frontend/src/App.tsx`, `crates/webui/frontend/src/main.tsx`, `crates/webui/frontend/package.json`

**Interfaces:**
- Consumes: commands `terminal_*`; events `terminal-output`/`terminal-exit` (Task 1); `RenderedSession.id` + `.source_path`.

- [ ] **Step 1: Add xterm deps + css import**

In `crates/webui/frontend`: `npm install @xterm/xterm @xterm/addon-fit`

In `crates/webui/frontend/src/main.tsx`, add after `import "./index.css";`:

```tsx
import "@xterm/xterm/css/xterm.css";
```

- [ ] **Step 2: Create `components/TerminalView.tsx`**

```tsx
import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

function b64ToBytes(b64: string): Uint8Array {
  const s = atob(b64);
  const a = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
  return a;
}

export default function TerminalView({
  sessionId,
  filePath,
  active,
}: {
  sessionId: string;
  filePath: string;
  active: boolean;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [exited, setExited] = useState(false);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const term = new Terminal({
      fontSize: 13,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      theme: { background: "#0a0a0a", foreground: "#e6e6e6" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    try {
      fit.fit();
    } catch {
      /* host may be hidden (0-size) on first mount; refit on activate */
    }
    termRef.current = term;
    fitRef.current = fit;

    const w = getCurrentWebviewWindow();
    const unsubs: Array<() => void> = [];
    let disposed = false;

    (async () => {
      unsubs.push(
        await w.listen<{ sessionId: string; data: string }>("terminal-output", (e) => {
          if (e.payload.sessionId === sessionId) term.write(b64ToBytes(e.payload.data));
        }),
        await w.listen<{ sessionId: string }>("terminal-exit", (e) => {
          if (e.payload.sessionId === sessionId) {
            term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
            setExited(true);
          }
        }),
      );
      if (disposed) {
        unsubs.forEach((u) => u());
        return;
      }
      await invoke("terminal_open", {
        sessionId,
        filePath,
        rows: term.rows,
        cols: term.cols,
      }).catch((err) => term.write(`\r\n\x1b[31m[failed to start: ${err}]\x1b[0m\r\n`));
    })();

    const onData = term.onData((d) => {
      invoke("terminal_input", { sessionId, data: d }).catch(() => {});
    });

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
        invoke("terminal_resize", { sessionId, rows: term.rows, cols: term.cols }).catch(() => {});
      } catch {
        /* hidden */
      }
    });
    ro.observe(host);

    return () => {
      disposed = true;
      unsubs.forEach((u) => u());
      onData.dispose();
      ro.disconnect();
      term.dispose();
    };
  }, [sessionId, filePath]);

  // When this terminal becomes active it was likely hidden (0-size) — refit.
  useEffect(() => {
    if (active && fitRef.current && termRef.current) {
      try {
        fitRef.current.fit();
        invoke("terminal_resize", {
          sessionId,
          rows: termRef.current.rows,
          cols: termRef.current.cols,
        }).catch(() => {});
        termRef.current.focus();
      } catch {
        /* ignore */
      }
    }
  }, [active, sessionId]);

  const restart = () => {
    const t = termRef.current;
    if (!t) return;
    t.clear();
    invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols })
      .then(() => setExited(false))
      .catch(() => {});
  };

  return (
    <div className="relative h-full w-full" style={{ display: active ? "block" : "none" }}>
      <div ref={hostRef} className="h-full w-full" />
      {exited && (
        <button
          onClick={restart}
          className="absolute top-2 right-2 px-3 py-1 rounded bg-[var(--color-accent)] text-white text-xs"
        >
          Restart
        </button>
      )}
    </div>
  );
}
```

- [ ] **Step 3: Create `components/TerminalPane.tsx`**

```tsx
import { useEffect, useState } from "react";
import TerminalView from "./TerminalView";

/** Keeps one mounted xterm per session opened, so each session's live terminal
 *  and scrollback persist across tab/session switches. Shows the active one. */
export default function TerminalPane({
  sessionId,
  filePath,
}: {
  sessionId: string | null;
  filePath: string | null;
}) {
  // sessionId -> filePath for every session a terminal has been opened for.
  const [opened, setOpened] = useState<Record<string, string>>({});

  useEffect(() => {
    if (sessionId && filePath) {
      setOpened((m) => (m[sessionId] ? m : { ...m, [sessionId]: filePath }));
    }
  }, [sessionId, filePath]);

  if (!sessionId) {
    return (
      <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
        Open a session in the Manager tab to start its terminal.
      </div>
    );
  }

  return (
    <div className="h-full w-full bg-[#0a0a0a]">
      {Object.entries(opened).map(([sid, fp]) => (
        <TerminalView key={sid} sessionId={sid} filePath={fp} active={sid === sessionId} />
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Add the tab bar + content switch in `App.tsx`**

Add the import:

```tsx
import TerminalPane from "./components/TerminalPane";
```

Add tab state in `App` (near the other `useState`s):

```tsx
  const [activeTab, setActiveTab] = useState<"manager" | "terminal">("manager");
```

Wrap the current return. The current top-level element is `<div className="flex h-screen overflow-hidden">…</div>`. Restructure to a column with a tab bar above a content area; the existing layout becomes the Manager content (change its `h-screen` to `h-full`):

```tsx
  return (
    <div className="flex flex-col h-screen overflow-hidden">
      <div className="flex items-center gap-1 px-2 py-1 border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
        {(["manager", "terminal"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setActiveTab(t)}
            className={
              "px-3 py-1 text-sm rounded " +
              (activeTab === t
                ? "bg-[var(--color-accent)] text-white"
                : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]")
            }
          >
            {t === "manager" ? "Manager" : "Terminal"}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-hidden">
        <div className="flex h-full overflow-hidden" style={{ display: activeTab === "manager" ? "flex" : "none" }}>
          {/* ===== existing Manager layout goes here (sidebar + drag handle + <main>) ===== */}
        </div>
        {activeTab === "terminal" && (
          <TerminalPane
            sessionId={transcript?.id ?? null}
            filePath={transcript?.source_path ?? null}
          />
        )}
      </div>
    </div>
  );
```

Move the existing children of the old root `<div className="flex h-screen overflow-hidden">` (the `{collapsed ? … : …}` sidebar block and the `<main>…</main>`) inside the new Manager `<div className="flex h-full overflow-hidden" style=…>`. Keep the Manager mounted via `display` toggle (not unmounted) so the open session + live-tail survive tab switches.

- [ ] **Step 5: Typecheck + tests**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run`
Expected: tsc clean; existing 20 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/components/TerminalView.tsx crates/webui/frontend/src/components/TerminalPane.tsx crates/webui/frontend/src/App.tsx crates/webui/frontend/src/main.tsx crates/webui/frontend/package.json crates/webui/frontend/package-lock.json
git commit -m "feat(webui): Manager/Terminal tabs + xterm terminal pane

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Verification

**Files:** none.

- [ ] **Step 1: Full automated suite + build**

Run: `cargo test && cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npm run build`
Expected: Rust green (incl. `terminal_argv`); tsc clean; 20 frontend tests; vite build clean. Report counts.

- [ ] **Step 2: Hand the user manual verification steps**

I will NOT launch the webui. Steps (frontend hot-reloads if the dev app is up; Rust changes need a `make webui` restart):

  1. Open session A (Manager) → click **Terminal** tab → `claude --resume A` runs interactively; type, see the TUI respond; a permission prompt works; arrow keys / ctrl-c behave.
  2. Switch to **Manager** while a turn runs → the transcript updates live (live-tail reading what the terminal's claude writes).
  3. Open session B → Terminal → B's own terminal; switch back to A's terminal → A's claude still live (not restarted), scrollback intact.
  4. Resize the window → the TUI reflows (FitAddon + resize).
  5. Close the window → no orphan processes: `pgrep -fl 'claude --resume'` returns nothing for the closed window's sessions.

- [ ] **Step 3: Report**

Report automated results + the manual steps. Do not merge — the user directs merges.

---

## Notes for the implementer

- `commands.rs` `terminal_*` are light (spawn returns immediately; streaming is on its own thread) → plain `#[tauri::command]` (sync), no `spawn_blocking`.
- `portable-pty` `MasterPty::try_clone_reader`/`take_writer`/`resize` take `&self`; the reader is moved to the thread, the master is kept in `PtyHandle` for resize. `Child::kill` is `&mut self` → `close`/`close_window` use `remove` then `kill`.
- React `StrictMode` double-invokes effects in dev: `terminal_open` is a no-op if the PTY already exists, and the effect cleanup disposes the xterm + unsubscribes (but does NOT `terminal_close` — the PTY must persist across unmount/tab-switch). The PTY is closed only on window Destroyed / app exit / explicit `terminal_close`.
- If `portable-pty = "0.8"` fails to resolve a method, check the installed version's API (`try_clone_reader`/`take_writer`/`openpty`/`CommandBuilder` are stable across recent 0.8/0.9); adjust the version pin.
- xterm event payloads are JSON (base64 string); decode with `atob` → `Uint8Array` → `term.write` so partial UTF-8 across chunks is reassembled by xterm.
