# Shared State / Single Poller Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One backend poller computes discovery + activity once and pushes to all windows; commands are cache-backed; cross-window `lastViewed` syncs.

**Architecture:** App-global `Shared` cache (single-flight) + a 3s poller thread (spawned in `.setup()`) that emits `activity-updated` (payload) and `sessions-changed` (ping). Windows listen instead of polling.

**Tech Stack:** Rust (Tauri state + background thread) + React/TS (event listeners).

## Global Constraints

- **Do discovery + activity scan once app-wide**, not per window.
- Poller idles when there are **no windows**. Interval **3s**.
- `sessions-changed` is a **ping** (windows re-pull cached `list_sessions`); `activity-updated` carries its (small) payload.
- Single-flight discovery on a cold cache (no startup N× duplication).
- Cross-window `lastViewed` sync must not loop (equality guard).
- Keep the per-window live-tail of the **open transcript** unchanged.
- Verification: `cargo test`, `tsc --noEmit`, `vitest run`, `vite build` all green.

---

### Task 1: Core — `dir_signature` + `signature_of`

**Files:**
- Modify: `crates/core/src/store/mod.rs`

**Interfaces:**
- Produces: `store::dir_signature(&Path) -> u64`; `store::signature_of(Vec<(String, u64)>) -> u64`.

- [ ] **Step 1: Write the failing test**

In `crates/core/src/store/mod.rs` tests module, add:

```rust
    #[test]
    fn signature_of_is_order_independent_and_change_sensitive() {
        let a = signature_of(vec![("/p/x.jsonl".into(), 10), ("/p/y.jsonl".into(), 20)]);
        let b = signature_of(vec![("/p/y.jsonl".into(), 20), ("/p/x.jsonl".into(), 10)]);
        assert_eq!(a, b, "same entries, any order → same signature");

        let c = signature_of(vec![("/p/x.jsonl".into(), 11), ("/p/y.jsonl".into(), 20)]);
        assert_ne!(a, c, "a changed mtime → different signature");

        let d = signature_of(vec![("/p/x.jsonl".into(), 10)]);
        assert_ne!(a, d, "a removed entry → different signature");
    }
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cargo test -p claude-code-manager-core signature_of`
Expected: FAIL — `cannot find function signature_of`.

- [ ] **Step 3: Implement both functions**

In `crates/core/src/store/mod.rs`, add (near `scan_activity`):

```rust
/// Order-independent, change-sensitive hash of a set of `(path, mtime_secs)`.
/// Used to detect when the session set or any file changed (cheap re-discovery
/// trigger for the shared poller).
pub fn signature_of(mut entries: Vec<(String, u64)>) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    entries.sort();
    let mut h = DefaultHasher::new();
    entries.hash(&mut h);
    h.finish()
}

/// Stat-walk `projects/*/*.jsonl` and hash `(path, mtime)` — a cheap probe to
/// detect any add/remove/modify across all sessions without parsing.
pub fn dir_signature(root: &Path) -> u64 {
    let projects = paths::projects_dir(root);
    let mut entries: Vec<(String, u64)> = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&projects) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Ok(inner) = fs::read_dir(&path) {
                for child in inner.flatten() {
                    let cp = child.path();
                    if cp.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let mtime = child
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    entries.push((cp.to_string_lossy().into_owned(), mtime));
                }
            }
        }
    }
    signature_of(entries)
}
```

- [ ] **Step 4: Run it, verify it passes**

Run: `cargo test -p claude-code-manager-core signature_of`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/store/mod.rs
git commit -m "feat(core): dir_signature + signature_of (cheap change probe)"
```

---

### Task 2: Backend — shared cache, poller, cache-backed commands

**Files:**
- Create: `crates/webui/src/shared.rs`
- Modify: `crates/webui/src/commands.rs`
- Modify: `crates/webui/src/lib.rs`

**Interfaces:**
- Produces: `shared::Shared` (managed); `commands::refresh_sessions`. Consumes: `store::{discover, scan_activity, dir_signature}`, `commands::SessionActivity`.

- [ ] **Step 1: Create `shared.rs`**

```rust
// crates/webui/src/shared.rs
use std::path::Path;
use std::sync::Mutex;

use claude_code_manager_core::session::summary::SessionSummary;
use claude_code_manager_core::store;

use crate::commands::SessionActivity;

/// App-global cache shared by every window: one discovery + one activity scan,
/// computed once (single-flight) and refreshed by the background poller.
pub struct Shared {
    data: Mutex<SharedData>,
    /// Serializes `discover()` so concurrent cold-cache callers don't duplicate it.
    compute: Mutex<()>,
}

#[derive(Default)]
struct SharedData {
    sessions: Option<Vec<SessionSummary>>,
    activity: Vec<SessionActivity>,
    file_sig: u64,
}

pub struct PollOutcome {
    pub activity: Vec<SessionActivity>,
    pub sessions_changed: bool,
}

impl Shared {
    pub fn new() -> Self {
        Shared { data: Mutex::new(SharedData::default()), compute: Mutex::new(()) }
    }

    /// Cached sessions; computes once under `compute` on a cold cache.
    pub fn sessions(&self, root: &Path) -> Result<Vec<SessionSummary>, String> {
        if let Some(s) = self.data.lock().unwrap().sessions.clone() {
            return Ok(s);
        }
        let _c = self.compute.lock().unwrap();
        if let Some(s) = self.data.lock().unwrap().sessions.clone() {
            return Ok(s); // another caller filled it while we waited
        }
        self.discover_into(root)
    }

    /// Force a re-discover (manual refresh), bypassing the cache.
    pub fn refresh_sessions(&self, root: &Path) -> Result<Vec<SessionSummary>, String> {
        let _c = self.compute.lock().unwrap();
        self.discover_into(root)
    }

    pub fn activity(&self) -> Vec<SessionActivity> {
        self.data.lock().unwrap().activity.clone()
    }

    /// One poller tick: refresh activity (always) and re-discover iff the dir
    /// signature changed. Returns what to broadcast.
    pub fn poll(&self, root: &Path) -> PollOutcome {
        let activity: Vec<SessionActivity> = store::scan_activity(root, 60)
            .into_iter()
            .map(|(id, t, awaiting)| SessionActivity {
                id: id.to_string(),
                last_event_at: t
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                awaiting_input: awaiting,
            })
            .collect();
        self.data.lock().unwrap().activity = activity.clone();

        let sig = store::dir_signature(root);
        let changed = self.data.lock().unwrap().file_sig != sig;
        let sessions_changed = if changed {
            let _c = self.compute.lock().unwrap();
            let _ = self.discover_into(root);
            true
        } else {
            false
        };
        PollOutcome { activity, sessions_changed }
    }

    /// Discover + record signature + store. Caller must hold `compute`.
    fn discover_into(&self, root: &Path) -> Result<Vec<SessionSummary>, String> {
        let disc = store::discover(root).map_err(|e| e.to_string())?;
        let sig = store::dir_signature(root);
        let mut d = self.data.lock().unwrap();
        d.sessions = Some(disc.sessions.clone());
        d.file_sig = sig;
        Ok(disc.sessions)
    }
}
```

- [ ] **Step 2: Rewire commands in `commands.rs`**

Add `use crate::shared::Shared;` near the other `use` lines.

Replace `list_sessions` with the cache-backed version:

```rust
#[tauri::command]
pub async fn list_sessions(app: AppHandle) -> Result<Vec<SessionSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<SessionSummary>, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        app.state::<Shared>().sessions(&root)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

Replace `session_activity` with the cache read (no scan here — the poller owns it):

```rust
#[tauri::command]
pub fn session_activity(app: AppHandle) -> Result<Vec<SessionActivity>, String> {
    Ok(app.state::<Shared>().activity())
}
```

Add `refresh_sessions` (force + broadcast):

```rust
/// Force a re-discovery (manual ↻) and broadcast so every window re-pulls.
#[tauri::command]
pub async fn refresh_sessions(app: AppHandle) -> Result<Vec<SessionSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<SessionSummary>, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        let sessions = app.state::<Shared>().refresh_sessions(&root)?;
        let _ = app.emit("sessions-changed", ());
        Ok(sessions)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

(`Emitter` is already imported — `emit_to` is used in the tail loop; if `app.emit`
is unresolved, add `use tauri::Emitter;`.)

- [ ] **Step 3: Wire `lib.rs` — manage, poller, register**

Add `mod shared;` at the top (next to `mod commands;`). Add imports:

```rust
use std::time::Duration;
use tauri::{Emitter, Manager, WindowEvent};
```

(Adjust the existing `use tauri::{Manager, WindowEvent};` line to include `Emitter`.)

Add `.manage(shared::Shared::new())` next to the other `.manage(...)` calls.

Register the command — in `generate_handler!`, after `commands::session_activity,`:

```rust
            commands::session_activity,
            commands::refresh_sessions,
```

Add a `.setup(...)` (e.g. right after the `.manage(...)` calls) that spawns the poller:

```rust
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(3));
                if handle.webview_windows().is_empty() {
                    continue; // idle when there are no windows
                }
                let root = match claude_code_manager_core::store::paths::default_claude_root() {
                    Some(r) => r,
                    None => continue,
                };
                let outcome = handle.state::<shared::Shared>().poll(&root);
                let _ = handle.emit("activity-updated", &outcome.activity);
                if outcome.sessions_changed {
                    let _ = handle.emit("sessions-changed", ());
                }
            });
            Ok(())
        })
```

- [ ] **Step 4: Build + test the crate**

Run: `cargo build -p claude-code-manager-webui && cargo test`
Expected: builds (fix any unused imports); all tests green. Note `list_sessions`
now takes `app` — Tauri injects it, the frontend invoke is unchanged.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/src/shared.rs crates/webui/src/commands.rs crates/webui/src/lib.rs
git commit -m "feat(webui): single shared poller + cache; list/activity served from it"
```

---

### Task 3: Frontend — event-driven hooks + cross-window lastViewed sync

**Files:**
- Modify: `crates/webui/frontend/src/hooks/useSessions.ts`
- Modify: `crates/webui/frontend/src/hooks/useActivity.ts`
- Modify: `crates/webui/frontend/src/hooks/usePersistentState.ts`
- Modify: `crates/webui/frontend/src/App.tsx`

**Interfaces:**
- Consumes: `list_sessions` / `refresh_sessions` / `session_activity` commands; `activity-updated` / `sessions-changed` events.

- [ ] **Step 1: `useSessions` — pull on `sessions-changed`, force on refresh**

Replace the `useSessions` function in `useSessions.ts` with:

```ts
export function useSessions() {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const pull = useCallback(async () => {
    try {
      setSessions(await invoke<SessionSummary[]>("list_sessions"));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setSessions(await invoke<SessionSummary[]>("refresh_sessions"));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    pull();
    // The backend poller broadcasts "sessions-changed" to ALL windows on any
    // discovery change; re-pull the (now cached) list.
    const un = listen("sessions-changed", () => pull());
    return () => {
      un.then((f) => f());
    };
  }, [pull]);

  return { sessions, loading, error, refresh };
}
```

Add `listen` to the event import at the top:

```ts
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
```

- [ ] **Step 2: `useActivity` — listen, drop the poll + refresh arg**

Replace `useActivity` in `useActivity.ts` with:

```ts
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Activity = { id: string; last_event_at: number; awaiting_input: boolean };
export type LiveActivity = { at: number; awaiting: boolean };

const toMap = (rows: Activity[]): Map<string, LiveActivity> =>
  new Map(rows.map((r) => [r.id, { at: r.last_event_at, awaiting: r.awaiting_input }]));

/**
 * Live per-session activity, pushed by the single backend poller (one scan
 * app-wide). Pulls once on mount for instant dots, then updates on each
 * "activity-updated" broadcast. No per-window polling.
 */
export function useActivity(): Map<string, LiveActivity> {
  const [map, setMap] = useState<Map<string, LiveActivity>>(new Map());
  useEffect(() => {
    invoke<Activity[]>("session_activity").then((rows) => setMap(toMap(rows))).catch(() => {});
    const un = listen<Activity[]>("activity-updated", (e) => setMap(toMap(e.payload)));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return map;
}
```

- [ ] **Step 3: `App.tsx` — drop the `useActivity` argument**

Change `const activity = useActivity(refresh);` to `const activity = useActivity();`.
(Leave `refresh` from `useSessions` as-is — it's still used by the sidebar.)

- [ ] **Step 4: `usePersistentState` — cross-window sync**

Replace `usePersistentState.ts` with:

```ts
import { useState, useEffect, useRef } from "react";

/** A useState whose value persists to localStorage and syncs across windows. */
export function usePersistentState<T>(
  key: string,
  initial: T,
): [T, React.Dispatch<React.SetStateAction<T>>] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(key);
      return raw === null ? initial : (JSON.parse(raw) as T);
    } catch {
      return initial;
    }
  });

  // What this window last wrote — used to skip echoes and break sync loops.
  const lastWritten = useRef<string>(JSON.stringify(value));

  useEffect(() => {
    const s = JSON.stringify(value);
    lastWritten.current = s;
    try {
      localStorage.setItem(key, s);
    } catch {
      // Ignore write failures.
    }
  }, [key, value]);

  // Another window changed this key → adopt it (unless it's our own echo).
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key !== key || e.newValue == null) return;
      if (e.newValue === lastWritten.current) return;
      try {
        setValue(JSON.parse(e.newValue) as T);
      } catch {
        // Ignore malformed values.
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [key]);

  return [value, setValue];
}
```

- [ ] **Step 5: Verify**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; 26 tests pass; build succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/hooks/useSessions.ts crates/webui/frontend/src/hooks/useActivity.ts crates/webui/frontend/src/hooks/usePersistentState.ts crates/webui/frontend/src/App.tsx
git commit -m "feat(webui): event-driven sessions/activity + cross-window lastViewed sync"
```

---

## Self-Review

**Spec coverage:**
- `dir_signature`/`signature_of` → Task 1. ✓
- `Shared` cache + single-flight + `poll` → Task 2 step 1. ✓
- Cache-backed `list_sessions`/`session_activity` + `refresh_sessions` → Task 2 step 2. ✓
- `manage(Shared)` + poller `.setup()` + register → Task 2 step 3. ✓
- `useSessions` event-driven + `useActivity` listen + drop arg → Task 3 steps 1-3. ✓
- `usePersistentState` cross-window sync (loop-guarded) → Task 3 step 4. ✓
- `signature_of` test → Task 1. ✓

**Placeholder scan:** none.

**Type consistency:** `Shared::{sessions,refresh_sessions,activity,poll}` ↔ command calls; `SessionActivity { id, last_event_at, awaiting_input }` shared by `poll` and the command; events `activity-updated` (payload `Vec<SessionActivity>`) / `sessions-changed` (ping) match the frontend listeners; `useActivity(): Map<string, LiveActivity>` (no arg) matches the `App.tsx` call.
