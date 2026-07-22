# Session Status Indicators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a live "running" dot and an "unread" dot/bold-title on each sidebar session row.

**Architecture:** The core computes each session's `last_event_at` (last real turn's timestamp). A cheap backend command tail-reads only recently-touched files so a lightweight frontend poll can drive a live "running" dot; unread compares `last_event_at` to a per-session last-viewed time kept in localStorage. Frontend-only UI; the two backend additions are additive.

**Tech Stack:** Rust (`claude-code-manager-core`, Tauri commands) + React 19 / TypeScript / Tailwind v4. Tests: `cargo test`, Vitest.

## Global Constraints

- **Two states only:** `running` and `unread`. No "awaiting input" / amber state.
- **`running`** = `now − last_event_at < 10s`. **`unread`** = `last_event_at > lastViewed[id]`.
- **Activity = last real turn**, never file mtime: a `user` event that is not `isMeta`, or an `assistant` event whose model is not `<synthetic>`. Skips summary/system/title/meta lines, so a rename's `custom-title` line is never activity.
- **Poll** every 3000ms, only while `document.visibilityState === "visible"`. Backend scan only tail-reads files modified within the last **60s**. On detected advance, a **1500ms** debounced full `refresh()` keeps tokens/title current.
- **No global directory watcher; no cross-window sharing** (deferred to the concurrency TODO).
- **No TUI changes** — `last_event_at` is additive to `SessionSummary`.

---

### Task 1: Core — `last_event_at` on `SessionSummary`

**Files:**
- Modify: `crates/core/src/session/event.rs` (add `Event::is_activity_turn`)
- Modify: `crates/core/src/session/summary.rs` (field + compute + tests)

**Interfaces:**
- Produces: `Event::is_activity_turn(&self) -> bool`; `SessionSummary.last_event_at: Option<SystemTime>`.

- [ ] **Step 1: Add the `is_activity_turn` predicate**

In `crates/core/src/session/event.rs`, inside `impl Event` (after `is_sidechain`, around line 323), add:

```rust
    /// True if this event is a real conversation turn — the signal for session
    /// "activity" (sidebar running/unread). A non-meta user turn, or a
    /// non-synthetic assistant turn. Excludes summaries, system lines, titles,
    /// meta turns. Sidechain (sub-agent) turns DO count — that's claude working.
    pub fn is_activity_turn(&self) -> bool {
        match self {
            Event::User(e) => !e.is_meta.unwrap_or(false),
            Event::Assistant(e) => e.message.model.as_deref() != Some("<synthetic>"),
            _ => false,
        }
    }
```

- [ ] **Step 2: Add the failing tests in `summary.rs`**

In `crates/core/src/session/summary.rs`, add to the `tests` module (after `meta_user_message_skipped`). First extend the chrono import at the top of the test module (`use chrono::Utc;` → add `TimeZone`):

```rust
    use chrono::{TimeZone, Utc};
```

Then add:

```rust
    #[test]
    fn last_event_at_is_last_real_turn_ignoring_title() {
        let t1 = Utc.timestamp_opt(1_000, 0).unwrap();
        let t2 = Utc.timestamp_opt(2_000, 0).unwrap();
        let mut u = match user("hi", false) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        u.timestamp = Some(t1);
        let mut a = match assistant_with_usage(1, 1, 1) {
            Event::Assistant(a) => a,
            _ => unreachable!(),
        };
        a.timestamp = Some(t2);
        // A trailing /rename custom-title line must NOT count as activity.
        let events = vec![
            Event::User(u),
            Event::Assistant(a),
            Event::CustomTitle(CustomTitleEvent { custom_title: "renamed".into() }),
        ];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.last_event_at, Some(SystemTime::from(t2)));
    }

    #[test]
    fn last_event_at_ignores_meta_user() {
        let t1 = Utc.timestamp_opt(1_000, 0).unwrap();
        let t2 = Utc.timestamp_opt(2_000, 0).unwrap();
        let mut real = match user("real", false) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        real.timestamp = Some(t1);
        let mut meta = match user("meta", true) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        meta.timestamp = Some(t2);
        let s = summarize(
            &[Event::User(real), Event::User(meta)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.last_event_at, Some(SystemTime::from(t1)));
    }

    #[test]
    fn last_event_at_none_when_no_turns() {
        let s = summarize(
            &[Event::CustomTitle(CustomTitleEvent { custom_title: "x".into() })],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.last_event_at, None);
    }
```

- [ ] **Step 3: Run the tests, verify they fail**

Run: `cargo test -p claude-code-manager-core last_event_at`
Expected: FAIL — `no field last_event_at on type SessionSummary` (compile error).

- [ ] **Step 4: Add the field and compute it**

In `crates/core/src/session/summary.rs`, add the field to the struct (after `mtime`, line 18):

```rust
    pub mtime: SystemTime,
    /// Timestamp of the last real conversation turn (see `Event::is_activity_turn`).
    /// Drives the sidebar running/unread indicators; `None` for a session with no
    /// turns yet. Never reflects mtime, so a rename never looks like activity.
    pub last_event_at: Option<SystemTime>,
```

Add the accumulator next to the other `let mut` bindings (after line 59):

```rust
    let mut last_event_at: Option<SystemTime> = None;
```

Add the capture as the FIRST statement inside `for ev in events {` (before the `forked_from_session` block, line 61):

```rust
        if ev.is_activity_turn() {
            if let Some(ts) = ev.timestamp() {
                last_event_at = Some(SystemTime::from(ts));
            }
        }
```

Add the field to the returned struct literal (after `mtime,`, line 157):

```rust
        mtime,
        last_event_at,
```

- [ ] **Step 5: Run tests, verify they pass**

Run: `cargo test -p claude-code-manager-core`
Expected: PASS (all core tests, including the 3 new ones).

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/session/event.rs crates/core/src/session/summary.rs
git commit -m "feat(core): SessionSummary.last_event_at (last real turn timestamp)"
```

---

### Task 2: Core + command — activity scan / poll endpoint

**Files:**
- Modify: `crates/core/src/session/parse.rs` (`last_activity` + test)
- Modify: `crates/core/src/store/mod.rs` (`scan_activity`)
- Modify: `crates/webui/src/commands.rs` (`SessionActivity` + `session_activity`)
- Modify: `crates/webui/src/lib.rs` (register command)

**Interfaces:**
- Consumes: `Event::is_activity_turn` (Task 1); `parse_tail`, `paths::{projects_dir, default_claude_root}`.
- Produces: `parse::last_activity(&Path) -> Option<DateTime<Utc>>`; `store::scan_activity(&Path, u64) -> Vec<(Uuid, SystemTime)>`; command `session_activity() -> Vec<SessionActivity { id: String, last_event_at: i64 }>`.

- [ ] **Step 1: Add the failing test for `last_activity`**

In `crates/core/src/session/parse.rs` `tests` module (it already has a `TempPath` helper), add. First ensure these imports exist at the top of the test module — add any missing:

```rust
    use crate::session::event::{
        AssistantEvent, AssistantMessage, CustomTitleEvent, Event, Message, MessageContent,
        UserEvent,
    };
    use chrono::{TimeZone, Utc};
```

Then the test (uses the existing `TempPath` to make a temp file path; writes events as JSONL via `serde_json` since `Event` derives `Serialize`):

```rust
    #[test]
    fn last_activity_skips_trailing_custom_title() {
        let t_user = Utc.timestamp_opt(5_000, 0).unwrap();
        let user_ev = Event::User(UserEvent {
            uuid: Some(uuid::Uuid::new_v4()),
            parent_uuid: None,
            timestamp: Some(t_user),
            is_sidechain: false,
            is_meta: Some(false),
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Text("hello".into()),
            },
            tool_use_result: None,
            cwd: None,
            git_branch: None,
            forked_from: None,
        });
        let title_ev = Event::CustomTitle(CustomTitleEvent { custom_title: "renamed".into() });
        let lines = format!(
            "{}\n{}\n",
            serde_json::to_string(&user_ev).unwrap(),
            serde_json::to_string(&title_ev).unwrap()
        );
        let tmp = TempPath::new("last_activity_title");
        std::fs::write(&tmp.0, lines).unwrap();

        assert_eq!(last_activity(&tmp.0), Some(t_user));
    }

    #[test]
    fn last_activity_none_for_missing_file() {
        assert_eq!(last_activity(std::path::Path::new("/no/such/file.jsonl")), None);
    }
```

(If `TempPath::new` has a different constructor in this module, mirror the existing test's usage — check the other tests in the same file.)

- [ ] **Step 2: Run it, verify it fails**

Run: `cargo test -p claude-code-manager-core last_activity`
Expected: FAIL — `cannot find function last_activity`.

- [ ] **Step 3: Implement `last_activity`**

In `crates/core/src/session/parse.rs`, ensure `use chrono::{DateTime, Utc};` is present at the top (add if missing), then add after `parse_tail`:

```rust
/// Timestamp of the last real conversation turn, read from only the file's tail
/// (no full parse) — the cheap activity probe behind the sidebar "running" dot.
/// Skips meta turns and the rename `custom-title` line via `Event::is_activity_turn`.
pub fn last_activity(path: &Path) -> Option<DateTime<Utc>> {
    const TAIL_BYTES: u64 = 65_536;
    let len = std::fs::metadata(path).ok()?.len();
    let outcome = parse_tail(path, len.saturating_sub(TAIL_BYTES)).ok()?;
    outcome
        .events
        .iter()
        .rev()
        .find_map(|ev| if ev.is_activity_turn() { ev.timestamp() } else { None })
}
```

- [ ] **Step 4: Run it, verify it passes**

Run: `cargo test -p claude-code-manager-core last_activity`
Expected: PASS.

- [ ] **Step 5: Add `store::scan_activity`**

In `crates/core/src/store/mod.rs`: extend the parse import to include `last_activity` (the line `use crate::session::parse::{parse_file, ParseOutcome};` → add `last_activity`), and ensure `use std::time::SystemTime;` is present (add if missing). Then add a public function (mirror the directory-walk in `search_sessions`):

```rust
/// Cheap activity probe for the sidebar "running" poll. Returns `(session id,
/// last real-event time)` for every session whose file was modified within
/// `recent_secs` — the only ones that could be "running". Tail-reads each such
/// file; everything older is skipped after a single stat.
pub fn scan_activity(root: &Path, recent_secs: u64) -> Vec<(Uuid, SystemTime)> {
    let projects = paths::projects_dir(root);
    let now = SystemTime::now();
    let mut out = Vec::new();
    let read_dir = match fs::read_dir(&projects) {
        Ok(rd) => rd,
        Err(_) => return out,
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let inner = match fs::read_dir(&path) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for child in inner.flatten() {
            let child_path = child.path();
            if child_path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = match child_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let id = match Uuid::parse_str(stem) {
                Ok(u) => u,
                Err(_) => continue,
            };
            let mtime = match child.metadata().and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            // Only recently-touched files can be running; skip the rest (stat only).
            if now.duration_since(mtime).map(|d| d.as_secs()).unwrap_or(u64::MAX) > recent_secs {
                continue;
            }
            if let Some(ts) = last_activity(&child_path) {
                out.push((id, SystemTime::from(ts)));
            }
        }
    }
    out
}
```

- [ ] **Step 6: Add the `session_activity` command**

In `crates/webui/src/commands.rs`, add near `new_session`/`path_exists`:

```rust
#[derive(Serialize)]
pub struct SessionActivity {
    pub id: String,
    /// Last real-event time, epoch seconds.
    pub last_event_at: i64,
}

/// Lightweight poll for the sidebar "running" dot: last real-event time for
/// sessions touched in the last 60s. Cheap (mostly stats) — safe to call ~every
/// few seconds. Runs on the blocking pool so it never stalls the UI thread.
#[tauri::command]
pub async fn session_activity() -> Result<Vec<SessionActivity>, String> {
    tauri::async_runtime::spawn_blocking(|| -> Result<Vec<SessionActivity>, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        let rows = store::scan_activity(&root, 60)
            .into_iter()
            .map(|(id, t)| SessionActivity {
                id: id.to_string(),
                last_event_at: t
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            })
            .collect();
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

(`Serialize` is already imported in `commands.rs` — it's used by `NewSessionResult`.)

- [ ] **Step 7: Register the command**

In `crates/webui/src/lib.rs`, add to the `tauri::generate_handler![...]` list (after `commands::path_exists,`):

```rust
            commands::path_exists,
            commands::session_activity,
```

- [ ] **Step 8: Build + test**

Run: `cargo test -p claude-code-manager-core && cargo build -p claude-code-manager-webui`
Expected: tests pass; webui builds.

- [ ] **Step 9: Commit**

```bash
git add crates/core/src/session/parse.rs crates/core/src/store/mod.rs crates/webui/src/commands.rs crates/webui/src/lib.rs
git commit -m "feat(webui): session_activity poll command + core scan_activity/last_activity"
```

---

### Task 3: Frontend — type + `deriveStatus` (pure)

**Files:**
- Modify: `crates/webui/frontend/src/types.ts`
- Create: `crates/webui/frontend/src/lib/sessionStatus.ts`
- Create: `crates/webui/frontend/src/lib/sessionStatus.test.ts`

**Interfaces:**
- Produces: `SessionSummary.last_event_at?: { secs_since_epoch: number; nanos_since_epoch: number } | null`; `deriveStatus(lastEventAt, lastViewed, now) -> { running, unread }`.

- [ ] **Step 1: Add the type field**

In `crates/webui/frontend/src/types.ts`, in `interface SessionSummary` (after the `mtime` line), add:

```ts
  last_event_at: { secs_since_epoch: number; nanos_since_epoch: number } | null;
```

- [ ] **Step 2: Write the failing test**

Create `crates/webui/frontend/src/lib/sessionStatus.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { deriveStatus } from "./sessionStatus";

describe("deriveStatus", () => {
  it("running when last event is within the 10s window", () => {
    expect(deriveStatus(1000, 0, 1005).running).toBe(true);
    expect(deriveStatus(1000, 0, 1009).running).toBe(true);
  });
  it("not running at/after the 10s boundary", () => {
    expect(deriveStatus(1000, 0, 1010).running).toBe(false);
    expect(deriveStatus(1000, 0, 1100).running).toBe(false);
  });
  it("unread when last event is newer than last viewed", () => {
    expect(deriveStatus(1000, 999, 9999).unread).toBe(true);
    expect(deriveStatus(1000, 1000, 9999).unread).toBe(false);
    expect(deriveStatus(1000, 1001, 9999).unread).toBe(false);
  });
  it("neither when there are no events", () => {
    expect(deriveStatus(null, 0, 9999)).toEqual({ running: false, unread: false });
  });
});
```

- [ ] **Step 3: Run it, verify it fails**

Run: `cd crates/webui/frontend && npx vitest run sessionStatus`
Expected: FAIL — cannot resolve `./sessionStatus`.

- [ ] **Step 4: Implement `deriveStatus`**

Create `crates/webui/frontend/src/lib/sessionStatus.ts`:

```ts
export type SessionStatus = { running: boolean; unread: boolean };

/** A session is "running" if its last real event landed within this window. */
export const RUNNING_WINDOW_SECS = 10;

/**
 * Derive a session's sidebar status from epoch-second inputs.
 * @param lastEventAt last real-event time (epoch s), or null if no turns yet
 * @param lastViewed  epoch s the user last opened this session (0 if never)
 * @param now         current epoch s
 */
export function deriveStatus(
  lastEventAt: number | null,
  lastViewed: number,
  now: number,
): SessionStatus {
  if (lastEventAt == null) return { running: false, unread: false };
  return {
    running: now - lastEventAt < RUNNING_WINDOW_SECS,
    unread: lastEventAt > lastViewed,
  };
}
```

- [ ] **Step 5: Run it, verify it passes + tsc**

Run: `cd crates/webui/frontend && npx vitest run sessionStatus && npx tsc --noEmit`
Expected: tests pass; tsc clean.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/types.ts crates/webui/frontend/src/lib/sessionStatus.ts crates/webui/frontend/src/lib/sessionStatus.test.ts
git commit -m "feat(webui): last_event_at type + deriveStatus helper (running/unread)"
```

---

### Task 4: Frontend — `useActivity` poll hook

**Files:**
- Create: `crates/webui/frontend/src/hooks/useActivity.ts`

**Interfaces:**
- Consumes: `session_activity` command (Task 2).
- Produces: `useActivity(refresh: () => void): Map<string, number>` (sessionId → last_event_at epoch s, recent sessions only).

- [ ] **Step 1: Implement the hook**

Create `crates/webui/frontend/src/hooks/useActivity.ts`:

```ts
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const POLL_MS = 3000;
const REFRESH_DEBOUNCE_MS = 1500;

type Activity = { id: string; last_event_at: number };

/**
 * Polls the backend for recently-active sessions' last-event time (epoch s) so
 * the sidebar can show a live "running" dot. Only polls while the window is
 * visible. When any session's activity advances, fires a debounced full
 * `refresh()` so the parsed list (tokens/title/last_event_at) catches up.
 * Returns a Map of sessionId → last_event_at for recently-touched sessions only.
 */
export function useActivity(refresh: () => void): Map<string, number> {
  const [map, setMap] = useState<Map<string, number>>(new Map());
  const mapRef = useRef(map);
  mapRef.current = map;
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  useEffect(() => {
    let cancelled = false;
    let debounce: ReturnType<typeof setTimeout> | null = null;

    const tick = async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const rows = await invoke<Activity[]>("session_activity");
        if (cancelled) return;
        const next = new Map(rows.map((r) => [r.id, r.last_event_at]));
        let advanced = false;
        for (const [id, t] of next) {
          if (t > (mapRef.current.get(id) ?? 0)) {
            advanced = true;
            break;
          }
        }
        setMap(next); // new identity every tick → consumers re-render, running decays
        if (advanced) {
          if (debounce) clearTimeout(debounce);
          debounce = setTimeout(() => refreshRef.current(), REFRESH_DEBOUNCE_MS);
        }
      } catch {
        /* ignore poll errors (e.g. mid-shutdown) */
      }
    };

    const interval = setInterval(tick, POLL_MS);
    tick();
    return () => {
      cancelled = true;
      clearInterval(interval);
      if (debounce) clearTimeout(debounce);
    };
  }, []);

  return map;
}
```

- [ ] **Step 2: Typecheck**

Run: `cd crates/webui/frontend && npx tsc --noEmit`
Expected: no errors (the hook is unused until Task 5 — its export alone won't error).

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/hooks/useActivity.ts
git commit -m "feat(webui): useActivity poll hook for live session status"
```

---

### Task 5: Frontend — wire indicators into App + Sidebar

**Files:**
- Modify: `crates/webui/frontend/src/App.tsx`
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `useActivity` (Task 4), `deriveStatus` (Task 3), `usePersistentState` (existing), `SessionSummary.last_event_at` (Task 3).

- [ ] **Step 1: App — imports + activity + lastViewed state**

In `crates/webui/frontend/src/App.tsx`, add imports near the top:

```tsx
import { usePersistentState } from "./hooks/usePersistentState";
import { useActivity } from "./hooks/useActivity";
```

Inside `App()`, after the `target` state declaration, add:

```tsx
  const activity = useActivity(refresh);
  const [lastViewed, setLastViewed] = usePersistentState<Record<string, number>>(
    "cm:lastViewed",
    {},
  );
```

- [ ] **Step 2: App — mark a session viewed when it opens**

In the load effect (the one keyed on `[target, open, close, refresh]`), in the `if (exists) {` branch, after `open(target.filePath, target.projectLabel);`, add:

```tsx
        const nowSecs = Math.floor(Date.now() / 1000);
        setLastViewed((m) => ({ ...m, [target.sessionId]: nowSecs }));
```

Add `setLastViewed` to that effect's dependency array (append it):

```tsx
  }, [target, open, close, refresh, setLastViewed]);
```

- [ ] **Step 3: App — keep the open session "read" as it streams**

Add a new effect after the load effect:

```tsx
  // While a session is open, keep its last-viewed time abreast of incoming
  // turns so live-tailed content in the session you're watching isn't "unread".
  useEffect(() => {
    if (!target) return;
    const live = activity.get(target.sessionId);
    if (live == null) return;
    setLastViewed((m) => (live > (m[target.sessionId] ?? 0) ? { ...m, [target.sessionId]: live } : m));
  }, [activity, target, setLastViewed]);
```

- [ ] **Step 4: App — pass props to Sidebar**

On the `<Sidebar ... />` element, add:

```tsx
              activity={activity}
              lastViewed={lastViewed}
```

- [ ] **Step 5: Sidebar — accept props + import**

In `crates/webui/frontend/src/components/Sidebar.tsx`, add the import:

```tsx
import { deriveStatus } from "../lib/sessionStatus";
```

Add to the `Props` interface:

```tsx
  activity: Map<string, number>;
  lastViewed: Record<string, number>;
```

Add to the destructured params in `export default function Sidebar({ ... })`:

```tsx
  activity,
  lastViewed,
```

- [ ] **Step 6: Sidebar — compute per-row status**

Inside the `sorted.map((s) => {` body, right after `const editing = editingId === s.id;`, add:

```tsx
            const liveEventAt = activity.get(s.id) ?? s.last_event_at?.secs_since_epoch ?? null;
            const status = deriveStatus(liveEventAt, lastViewed[s.id] ?? 0, now);
            // You're looking at the open session — never flag it unread.
            const unread = status.unread && s.id !== activeId;
```

- [ ] **Step 7: Sidebar — render the status dot**

The row wrapper is `<div key={s.id} ... className={`relative flex group ...`}>`. As the FIRST child inside that `<div>` (before the `{editing ? (` block), add:

```tsx
              {(status.running || unread) && (
                <span
                  className={`absolute left-1 top-2.5 h-2 w-2 rounded-full ${
                    status.running ? "bg-green-500 animate-pulse" : "bg-[var(--color-accent)]"
                  }`}
                  title={
                    status.running
                      ? "Claude is working in this session"
                      : "Unread — new since you last opened it"
                  }
                />
              )}
```

- [ ] **Step 8: Sidebar — bold the title when unread**

In the main clickable area, the title `<div>` currently reads
`className="text-sm text-[var(--color-text)] truncate mt-0.5"`. Change it to bold when unread:

```tsx
                  <div
                    className={`text-sm text-[var(--color-text)] truncate mt-0.5 ${
                      unread ? "font-semibold" : ""
                    }`}
                  >
                    {s.title}
                  </div>
```

- [ ] **Step 9: Full verification**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; all tests pass; build succeeds.

- [ ] **Step 10: Commit**

```bash
git add crates/webui/frontend/src/App.tsx crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): sidebar running/unread indicators"
```

---

## Self-Review

**Spec coverage:**
- `running` / `unread` states → Task 3 (`deriveStatus`), Task 5 (render). ✓
- `last_event_at` skipping meta/title → Task 1 (`is_activity_turn` + tests). ✓
- Cheap poll command, tail-read, recent-only → Task 2. ✓
- 3s focus-gated poll + debounced refresh → Task 4. ✓
- Unread via localStorage last-viewed, set on open, kept fresh while open → Task 5 (steps 2–3). ✓
- Status dot + bold title visual → Task 5 (steps 7–8). ✓
- No TUI / no global watcher / two states → honored (additive field; poll only). ✓

**Placeholder scan:** none — every code step has complete code.

**Type consistency:** `last_event_at` is `Option<SystemTime>` in Rust (Task 1) → serialized `{ secs_since_epoch, nanos_since_epoch }` and typed so in `types.ts` (Task 3); read as `s.last_event_at?.secs_since_epoch` (Task 5). The poll command returns `last_event_at: i64` epoch-seconds (Task 2) → `Activity.last_event_at: number` (Task 4) → both feed `deriveStatus(lastEventAt: number | null, ...)` (Task 3) as epoch seconds. `now` is epoch seconds everywhere (`Math.floor(Date.now()/1000)`). `deriveStatus` / `useActivity` / `usePersistentState` names match their call sites.
