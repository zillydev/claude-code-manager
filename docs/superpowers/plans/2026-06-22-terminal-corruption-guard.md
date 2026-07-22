# Terminal Corruption Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Exactly one `claude` PTY per session app-wide; a second window opening the same session is blocked + redirected, never spawned.

**Architecture:** Re-key the app-global PTY pool by `session_id` (was `{label}|{session_id}`), track the owner window on each handle, and decide spawn/reuse/busy under the pool lock. The frontend shows a redirect overlay on `busy`.

**Tech Stack:** Rust (Tauri commands + `portable-pty`) + React/TS.

## Global Constraints

- **At most one PTY per session, app-wide.** Pool keyed by `session_id` only.
- **Block + redirect** on a cross-window collision (no shared/mirrored terminal, no take-over).
- **Decide + spawn + insert under one lock hold** in `terminal_open` (TOCTOU-free).
- A **blocked** window must never call `terminal_input`/`terminal_resize` (the pool is session-keyed, so it would hit the owner's process).
- Guards claude-code-manager's own windows only (external claude is undetectable).
- Verification: `cargo test`, `tsc --noEmit`, `vitest run`, `vite build` all green.

---

### Task 1: Backend — one PTY per session, block/redirect, focus_window

**Files:**
- Modify: `crates/webui/src/terminal.rs`
- Modify: `crates/webui/src/commands.rs`
- Modify: `crates/webui/src/lib.rs`

**Interfaces:**
- Produces: `terminal::open_decision(Option<&str>, &str) -> terminal::OpenDecision`; `TerminalState` methods keyed by `session_id`; `commands::TerminalOpenResult { status, owner }`; `commands::focus_window`.

- [ ] **Step 1: Write the failing test for `open_decision`**

In `crates/webui/src/terminal.rs`, add to the `tests` module:

```rust
    #[test]
    fn open_decision_spawn_reuse_busy() {
        assert_eq!(open_decision(None, "main"), OpenDecision::Spawn);
        assert_eq!(open_decision(Some("main"), "main"), OpenDecision::Reuse);
        assert_eq!(open_decision(Some("main"), "win-1"), OpenDecision::Busy);
    }
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cargo test -p claude-code-manager-webui open_decision`
Expected: FAIL — `cannot find function open_decision` / `OpenDecision`.

- [ ] **Step 3: Implement `OpenDecision` + `open_decision`, re-key the pool**

In `crates/webui/src/terminal.rs`:

Delete the `key` helper:

```rust
fn key(label: &str, session_id: &str) -> String {
    format!("{label}|{session_id}")
}
```

Add the decision (place above `PtyHandle`):

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum OpenDecision {
    Spawn,
    Reuse,
    Busy,
}

/// Given the current owner of a session's PTY (if any) and the window asking to
/// open it: spawn if free, reuse if the caller already owns it, else refuse.
pub fn open_decision(existing_owner: Option<&str>, requester: &str) -> OpenDecision {
    match existing_owner {
        None => OpenDecision::Spawn,
        Some(o) if o == requester => OpenDecision::Reuse,
        Some(_) => OpenDecision::Busy,
    }
}
```

Add `owner` to `PtyHandle`:

```rust
/// A live PTY: master (resize), writer (input), child (kill), and the window
/// label that owns it (one PTY per session, app-wide).
pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    owner: String,
}
```

Replace the whole `impl TerminalState` block (now keyed by `session_id`):

```rust
impl TerminalState {
    pub fn owner(&self, session_id: &str) -> Option<&str> {
        self.ptys.get(session_id).map(|h| h.owner.as_str())
    }
    pub fn insert(&mut self, session_id: &str, handle: PtyHandle) {
        self.ptys.insert(session_id.to_string(), handle);
    }
    pub fn write(&mut self, session_id: &str, data: &[u8]) -> std::io::Result<()> {
        if let Some(h) = self.ptys.get_mut(session_id) {
            h.writer.write_all(data)?;
            h.writer.flush()?;
        }
        Ok(())
    }
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) {
        if let Some(h) = self.ptys.get(session_id) {
            let _ = h.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }
    pub fn close(&mut self, session_id: &str) {
        if let Some(mut h) = self.ptys.remove(session_id) {
            let _ = h.child.kill();
        }
    }
    /// Kill+drop every PTY owned by a window (window-close cleanup).
    pub fn close_window(&mut self, label: &str) {
        let ids: Vec<String> = self
            .ptys
            .iter()
            .filter(|(_, h)| h.owner == label)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(mut h) = self.ptys.remove(&id) {
                let _ = h.child.kill();
            }
        }
    }
    /// Kill every terminal (app-exit backstop).
    pub fn close_all(&mut self) {
        for (_id, mut h) in self.ptys.drain() {
            let _ = h.child.kill();
        }
    }
    fn remove_key(&mut self, session_id: &str) {
        self.ptys.remove(session_id);
    }
}
```

In `spawn_terminal`, set the owner on the returned handle and key the reader's
self-cleanup by `session_id`. Replace the `let k = key(&label, &session_id);`
line and the `Ok(PtyHandle { ... })` tail:

```rust
    let owner = label.clone();
    let cleanup_id = session_id.clone();
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
        let _ = app.emit_to(label.as_str(), "terminal-exit", json!({ "sessionId": session_id }));
        if let Some(state) = app.try_state::<std::sync::Mutex<TerminalState>>() {
            if let Ok(mut g) = state.lock() {
                g.remove_key(&cleanup_id);
            }
        }
    });

    Ok(PtyHandle { master: pair.master, writer, child, owner })
```

(`session_id` and `label` move into the thread; `owner`/`cleanup_id` are the
pre-thread clones.)

- [ ] **Step 4: Run the test, verify it passes**

Run: `cargo test -p claude-code-manager-webui open_decision`
Expected: PASS. (The crate won't fully build yet — commands.rs still uses the old signatures; fixed next. The unit test compiles within the lib regardless — if the crate fails to build, proceed to Step 5 first, then re-run.)

- [ ] **Step 5: Update `commands.rs` — `terminal_open` result, decision, lock-held spawn**

In `crates/webui/src/commands.rs`, replace the `terminal_open` body (the part from
`let label = ...` through the final `Ok(())`) with:

```rust
    let label = window.label().to_string();
    // Resolve cwd/fresh BEFORE locking (file read, no pool access).
    // `new_cwd` Some → brand-new session: spawn fresh (`--session-id`) there.
    // None → resume an existing session, cwd from its first cwd event.
    let (cwd, fresh) = match new_cwd {
        Some(c) => (c, true),
        None => {
            let lines = read_raw(std::path::Path::new(&file_path)).map_err(|e| e.to_string())?;
            let cwd =
                first_cwd(&lines).ok_or("could not determine the session's working directory")?;
            (cwd, false)
        }
    };

    let state = app.state::<Mutex<TerminalState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    match terminal::open_decision(guard.owner(&session_id), &label) {
        terminal::OpenDecision::Reuse => {
            Ok(TerminalOpenResult { status: "reused".into(), owner: None })
        }
        terminal::OpenDecision::Busy => {
            let owner = guard.owner(&session_id).map(str::to_string);
            Ok(TerminalOpenResult { status: "busy".into(), owner })
        }
        terminal::OpenDecision::Spawn => {
            // Spawn under the lock so two windows racing the same fresh session
            // can't both spawn (the owner is recorded before the lock is released).
            let handle =
                terminal::spawn_terminal(app.clone(), label.clone(), session_id.clone(), cwd, rows, cols, fresh)
                    .map_err(|e| format!("failed to start terminal: {e}"))?;
            guard.insert(&session_id, handle);
            Ok(TerminalOpenResult { status: "started".into(), owner: None })
        }
    }
```

Change the `terminal_open` signature's return type to
`Result<TerminalOpenResult, String>` (was `Result<(), String>`).

Add the result type just above `terminal_open` (or near the other command structs):

```rust
#[derive(Serialize)]
pub struct TerminalOpenResult {
    /// "started" | "reused" | "busy".
    pub status: String,
    /// Owner window label when `busy`, so the frontend can offer "switch to it".
    pub owner: Option<String>,
}
```

Update the three keyed calls (drop the `&label` arg):

```rust
        .write(&session_id, data.as_bytes())   // terminal_input
```
```rust
        .resize(&session_id, rows, cols);        // terminal_resize
```
```rust
        .close(&session_id);                     // terminal_close
```

(`terminal_input`/`terminal_resize`/`terminal_close` no longer need `label`; you
may leave the `let label = window.label()...` lines — remove them to avoid an
unused-variable warning, and drop the now-unused `window` param if the linter
flags it. Keep `window`/`app` as needed for `app.state`.)

Add the `focus_window` command (near `terminal_close`):

```rust
/// Focus a window by label — used by the "switch to it" button when a session
/// is already open in another window. On macOS this jumps to that window's Space.
#[tauri::command]
pub fn focus_window(label: String, app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(&label) {
        w.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}
```

(`AppHandle` and `Manager` (for `get_webview_window`) are already imported in
`commands.rs`; if `get_webview_window` is unresolved, add `use tauri::Manager;`.)

- [ ] **Step 6: Register `focus_window` in `lib.rs`**

In `crates/webui/src/lib.rs` `generate_handler![...]`, add after `commands::session_activity,`:

```rust
            commands::session_activity,
            commands::focus_window,
```

(The `WindowEvent::Destroyed` → `close_window(win.label())` call is unchanged.)

- [ ] **Step 7: Build + test the whole crate**

Run: `cargo test -p claude-code-manager-webui && cargo build -p claude-code-manager-webui`
Expected: tests pass (incl. `open_decision_spawn_reuse_busy`); webui builds with no errors. Fix any unused-variable warnings (drop unused `label` bindings).

- [ ] **Step 8: Full workspace test**

Run: `cargo test`
Expected: all green (core + tui + webui).

- [ ] **Step 9: Commit**

```bash
git add crates/webui/src/terminal.rs crates/webui/src/commands.rs crates/webui/src/lib.rs
git commit -m "feat(webui): one claude PTY per session app-wide (block + redirect)"
```

---

### Task 2: Frontend — busy overlay, gate input/resize, Switch button

**Files:**
- Modify: `crates/webui/frontend/src/components/TerminalView.tsx`

**Interfaces:**
- Consumes: `terminal_open` now returns `{ status: "started" | "reused" | "busy"; owner: string | null }`; `focus_window` command.

- [ ] **Step 1: Add `blocked` state + ref**

In `TerminalView.tsx`, next to `const [started, setStarted] = useState(false);` add:

```tsx
  const [blocked, setBlocked] = useState<{ owner: string | null } | null>(null);
  const blockedRef = useRef(false);
  blockedRef.current = blocked !== null;
```

- [ ] **Step 2: Gate input & resize when blocked**

In the `onData` handler, early-return when blocked:

```tsx
    const onData = term.onData((d) => {
      if (blockedRef.current) return;
      invoke("terminal_input", { sessionId, data: d }).catch(() => {});
    });
```

In the `ResizeObserver` callback, after the 0-size guard, add:

```tsx
      if (host.clientWidth === 0 || host.clientHeight === 0) return;
      if (blockedRef.current) return;
```

- [ ] **Step 3: Handle the `terminal_open` result in the activation effect**

Replace the activation-effect `invoke("terminal_open", {...}).catch(...)` call with:

```tsx
        invoke<{ status: string; owner: string | null }>("terminal_open", {
          sessionId,
          filePath,
          rows: t.rows,
          cols: t.cols,
          newCwd: cwd ?? null,
        })
          .then((res) => {
            if (res.status === "busy") setBlocked({ owner: res.owner });
          })
          .catch((err) => t.write(`\r\n\x1b[31m[failed to start: ${err}]\x1b[0m\r\n`));
```

- [ ] **Step 4: Reset `blocked` on restart**

In `restart()`, clear it before re-opening:

```tsx
  const restart = () => {
    const t = termRef.current;
    if (!t) return;
    t.clear();
    setStarted(false);
    setBlocked(null);
    invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols })
      .then(() => setExited(false))
      .catch(() => {});
  };
```

- [ ] **Step 5: Render the redirect overlay; hide "Starting…" when blocked**

Gate the "Starting claude…" overlay on `!blocked` and add the blocked overlay:

```tsx
      {!started && !exited && !blocked && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-[var(--color-text-dim)] pointer-events-none">
          <Spinner size="md" />
          <span className="text-sm">Starting claude…</span>
        </div>
      )}
      {blocked && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-[var(--color-text-dim)] px-6 text-center">
          <span className="text-sm">This session is already open in another window.</span>
          {blocked.owner && (
            <button
              onClick={() => invoke("focus_window", { label: blocked.owner }).catch(() => {})}
              className="px-3 py-1 rounded bg-[var(--color-accent)] text-white text-xs"
            >
              Switch to it
            </button>
          )}
        </div>
      )}
```

- [ ] **Step 6: Verify**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; 26 tests pass; build succeeds.

- [ ] **Step 7: Commit**

```bash
git add crates/webui/frontend/src/components/TerminalView.tsx
git commit -m "feat(webui): terminal 'already open in another window' redirect overlay"
```

---

## Self-Review

**Spec coverage:**
- One PTY per session (re-key by session_id) → Task 1 step 3. ✓
- Owner tracking + decision (spawn/reuse/busy) → Task 1 steps 1,3,5. ✓
- Lock-held spawn (TOCTOU-free) → Task 1 step 5. ✓
- Window-close releases owned PTYs → `close_window` owner-based (step 3); call unchanged in lib.rs (step 6). ✓
- `focus_window` + register → Task 1 steps 5,6. ✓
- Frontend busy overlay + Switch + gate input/resize → Task 2. ✓
- `open_decision` unit test → Task 1 steps 1,2,4. ✓

**Placeholder scan:** none — complete code in every step.

**Type consistency:** `TerminalOpenResult { status: String, owner: Option<String> }` (Rust) ↔ `{ status: string; owner: string | null }` (TS). `open_decision(Option<&str>, &str) -> OpenDecision`. `owner(session_id) -> Option<&str>`. `insert(session_id, handle)`. `write/resize/close(session_id, …)`. `close_window(label)` (owner-based). All call sites match.
