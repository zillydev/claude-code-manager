# Shared State / Single Poller Design

**Date:** 2026-06-22
**Status:** Approved
**TODOS.md item:** Concurrency → "avoid recomputing things. keep the state shared so every window feels as snappy as a single window"

## Goal

Do the expensive cross-window work — full discovery (`list_sessions`) and the
activity scan (`session_activity`) — **once**, app-wide, instead of once per
window. Replace each window's independent polling with a **single backend poller**
that computes shared state and **pushes** updates to all windows. Plus a small
fix: cross-window `lastViewed` sync so an unread badge cleared in one window
clears in the others.

## Architecture

One app-global cache + one background poller thread. Windows become event-driven
listeners instead of independent pollers.

### `crates/webui/src/shared.rs` (new) — the cache

```rust
pub struct Shared {
    data: Mutex<SharedData>, // cached results
    compute: Mutex<()>,      // serializes discovery (single-flight)
}
#[derive(Default)]
struct SharedData {
    sessions: Option<Vec<SessionSummary>>, // None = never computed
    activity: Vec<SessionActivity>,
    file_sig: u64,                         // signature of all (path, mtime)
}
```

Methods:
- `sessions(&self, root) -> Result<Vec<SessionSummary>, String>` — return the
  cache; on a cold cache compute once under `compute` (double-checked, so two
  windows racing at startup → one `discover()`, the other waits and gets the
  cached clone). Pairs the discover with `dir_signature` so `file_sig` is set.
- `refresh_sessions(&self, root)` — force a `discover()` (ignore cache) under
  `compute`, update `sessions` + `file_sig`. Backs the manual ↻.
- `activity(&self) -> Vec<SessionActivity>` — clone the cached activity.
- `poll(&self, root) -> PollOutcome { activity, sessions_changed }` — the poller
  tick (below).

A private `discover_into(root)` does `discover()` + `dir_signature()` + store, so
`sessions` (cold), `refresh_sessions`, and `poll` share one code path (DRY) and
`file_sig` never drifts (no redundant re-discovery).

### The poller (spawned in `lib.rs` `.setup()`)

A `std::thread` loop, every **3s**:
1. Skip if `handle.webview_windows().is_empty()` (idle when no windows).
2. `let outcome = shared.poll(&root)`:
   - `scan_activity(root, 60)` → map to `Vec<SessionActivity>` → store → return it.
   - `dir_signature(root)`; if it differs from `file_sig` → `discover_into` (under
     `compute`) and set `sessions_changed = true`.
3. `handle.emit("activity-updated", &outcome.activity)` (small payload, pushed to
   all windows).
4. If `outcome.sessions_changed` → `handle.emit("sessions-changed", ())` — a
   **ping** (no payload): windows re-pull the now-cached `list_sessions`, so a
   500-session list is never serialized to every window on every change; the
   heavy `discover()` still runs exactly once (in the poller).

### Commands (cache-backed)

- `list_sessions(app)` → `app.state::<Shared>().sessions(&root)` (spawn_blocking).
- `session_activity(app)` → `app.state::<Shared>().activity()` (sync clone; cheap).
- **New** `refresh_sessions(app)` → `shared.refresh_sessions(&root)` then
  `app.emit("sessions-changed", ())` so other windows re-pull; returns the list to
  the caller (spawn_blocking).
- `Shared` is `manage`d in `lib.rs` (`.manage(shared::Shared::new())`);
  `refresh_sessions` registered in `generate_handler!`.

### Core (`crates/core/src/store/mod.rs`)

- **New** `dir_signature(root) -> u64` — stat-walk `projects/*/*.jsonl`, collect
  `(path, mtime_secs)`, and hash via a pure helper
  `signature_of(entries: Vec<(String, u64)>) -> u64` (sorts → hashes, so it's
  order-independent and change-sensitive). `scan_activity` is unchanged (the
  poller still calls it).

## Frontend

### `useSessions` (event-driven)

- `pull()` — `invoke("list_sessions")` → `setSessions` (cached, instant).
- `refresh()` — `invoke("refresh_sessions")` → `setSessions(returned)`.
- On mount: `pull()` once, then `listen("sessions-changed", pull)` (global
  `listen` from `@tauri-apps/api/event` — the poller's `emit` reaches every
  window). Cleanup on unmount.
- Live-tail `useTranscript` is untouched (per-window watcher streams the open
  transcript sub-second; the 3s poller only owns the sidebar).

### `useActivity` (listen, no poll)

- Drop the `setInterval` **and** the `refresh` param + debounced-refresh-on-change
  (the backend now triggers rediscovery via the signature → `sessions-changed`).
- On mount: `invoke("session_activity")` once for instant initial dots, then
  `listen("activity-updated", e => setMap(toMap(e.payload)))`. Returns the same
  `Map<string, LiveActivity>`.
- `App.tsx`: `const activity = useActivity();` (drop the argument). The
  keep-open-read effect is unchanged.

### `usePersistentState` — cross-window sync (the `lastViewed` fix)

Add a `storage`-event listener: when another window writes the same key,
`setValue(JSON.parse(newValue))`. Guard against a write-loop with a `serialized`
ref — skip if the incoming value equals what this window last wrote (so a window
viewing a session clears its unread dot everywhere, with no event storm). The
existing write-effect is unchanged.

## Testing

- **Core:** `signature_of` — same entries in any order hash equal; a changed mtime
  or an added/removed entry hashes different. Pure, lives with `store` tests.
- **Frontend:** keep the existing `deriveStatus` tests. The poller/push, cache
  single-flight, and storage-sync are integration/concurrency behavior — verified
  manually (open 2-3 windows: one scan per interval; a change in one appears in
  all; viewing in one clears its unread dot in another).
- `cargo test`, `tsc --noEmit`, `vitest run`, `vite build` all green.

## Edge cases

- **Cold start:** first `list_sessions` single-flights `discover()`; the poller's
  first tick (≤3s) then owns refreshes. Initial running dots appear after the
  first activity push (~3s) — acceptable.
- **No windows:** poller idles (gated on `webview_windows()`).
- **Manual refresh:** `refresh_sessions` updates the caller immediately and pings
  the rest; `file_sig` is set so the poller doesn't redundantly re-discover.
- **Storage-event loop:** prevented by the `serialized`-ref equality guard.

## Non-goals

- **Watcher dedup** when two windows view the *same* session (audit gap #3) —
  deferred; low value now that the sidebar work is shared.
- No change to the per-window live-tail of the open transcript.
- No leader-election / BroadcastChannel (rejected mechanism).
