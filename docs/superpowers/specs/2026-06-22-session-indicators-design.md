# Session Status Indicators Design

**Date:** 2026-06-22
**Status:** Approved
**TODOS.md item:** UI overhaul → "show indicators for which sessions are currently running … which are unread"

## Goal

Show two at-a-glance indicators on each sidebar session row:

- **running** — claude is actively writing to that session right now.
- **unread** — the session has content newer than the last time the user opened it.

(The originally-scoped "asking for input / stopped" state was cut — two states only.)

## States & visual

A single **status dot** sits at the left of each row; the title **bolds** when unread.
Priority cascade for the dot:

1. **running** → green dot with a subtle CSS pulse.
2. **unread** (and not running) → accent-colored dot.
3. neither → no dot (the row keeps its current left padding so layout doesn't jump).

The **title is bold whenever the session is unread**, independent of the dot — so a
running session the user hasn't seen shows a green dot *and* a bold title.

Definitions:

- **running** = `now − last_event_at < RUNNING_WINDOW` (10s).
- **unread** = `last_event_at > lastViewed[sessionId]` (0 if never viewed).

`last_event_at` is the timestamp of the session's last *real* conversation event
(see below) — never file mtime, so a rename never looks like activity.

## Detection

### Backend: one new `SessionSummary` field

`crates/core/src/session/summary.rs` — add `last_event_at: Option<SystemTime>`,
computed in the existing `summarize()` event walk. It is the `timestamp` of the
last event that is a real conversation turn:

- a `user` event that is **not** `is_meta` and not a sidechain, or
- an `assistant` event that is not a sidechain.

Explicitly skipped (so they never count as activity): `summary` events, `system`
events, meta user events, and the rename `custom-title` line. Serialized to the
frontend like the existing `mtime` (`{ secs_since_epoch }`), exposed on the TS
`SessionSummary` type as `last_event_at`.

No other backend fields. (`awaiting_input` is **not** added — the cut state needed it.)

### Backend: one cheap poll command

`crates/webui/src/commands.rs` — add `session_activity() -> Vec<SessionActivity>`
where `SessionActivity { id: String, last_event_at: Option<i64> }` (epoch seconds).

- Enumerates the same `~/.claude/projects/**/*.jsonl` set discovery uses; the
  session `id` is the filename stem (no parse needed for the id).
- For each file, reads only the **tail** (last ~16 KB) and scans backward for the
  last real-event line's `timestamp` — **not** a full reparse.
- Optimization (plan detail): `stat` first and only tail-read files whose mtime
  advanced since the previous call, so steady state is mostly cheap stats.

This is the live signal source; it does **not** touch the existing per-open-session
live-tail watcher.

### Frontend: poll + derive

`crates/webui/frontend/src/hooks/useActivity.ts` (new):

- Polls `session_activity` every **~3s**, but only while the window is focused /
  visible (`document.visibilityState`), so a backgrounded window is idle.
- Holds `Map<id, last_event_at>`; exposes it to the sidebar.
- When the poll shows any session's `last_event_at` advanced versus the held map,
  it triggers a **debounced (~1.5s) full `refresh()`** so the parsed fields
  (tokens, title, `last_event_at`) stay current. Running/unread themselves update
  immediately from the poll map without waiting for the refresh.

`deriveStatus` — a **pure** helper (own file, e.g. `lib/sessionStatus.ts`) used by
the sidebar and unit-tested:

```ts
type SessionStatus = { running: boolean; unread: boolean };
function deriveStatus(
  lastEventAt: number | null, // epoch secs, freshest of poll-map vs summary
  lastViewed: number,         // epoch secs, 0 if never
  now: number,                // epoch secs
): SessionStatus
// running = lastEventAt != null && now - lastEventAt < 10
// unread  = lastEventAt != null && lastEventAt > lastViewed
```

### Frontend: unread persistence

Reuse the existing `usePersistentState` hook (localStorage). A
`Record<sessionId, number>` (epoch secs) of last-viewed times under one key
(e.g. `cm:lastViewed`). Set `lastViewed[id] = session.last_event_at` (or `now` if
absent) **when a session is opened** — i.e. when it becomes the open `target` in
`App.tsx` (the same trigger that loads it into the Manager). Opening clears its
unread state.

## Components / files

- **Modify** `crates/core/src/session/summary.rs` — add `last_event_at` + compute it.
- **Modify** `crates/webui/src/commands.rs` — add `SessionActivity` + `session_activity`.
- **Modify** `crates/webui/src/lib.rs` — register `session_activity`.
- **Create** `crates/webui/frontend/src/hooks/useActivity.ts` — the poll hook.
- **Create** `crates/webui/frontend/src/lib/sessionStatus.ts` — `deriveStatus` (pure).
- **Modify** `crates/webui/frontend/src/types.ts` — `last_event_at` on `SessionSummary`.
- **Modify** `crates/webui/frontend/src/App.tsx` — own `lastViewed` state, set on open,
  mount `useActivity`, pass activity map + lastViewed to the sidebar.
- **Modify** `crates/webui/frontend/src/components/Sidebar.tsx` — status dot + bold title.

## Edge cases

- **Rename** appends a `custom-title` line → bumps file mtime but **not**
  `last_event_at` (it's not a real turn) → no false running/unread. This is the
  reason activity is based on the last real event, not mtime.
- **Empty / brand-new session** (no events yet) → `last_event_at = None` → no dot,
  not unread. (Matches the new-session flow: it stays blank until the first turn.)
- **Clock skew / future timestamps** → `now − last_event_at` could be negative;
  treat negative as "running" is fine (very recent). Guard isn't required.
- **Backgrounded window** → poll pauses; on refocus it resumes and catches up.

## Testing

- **Core (Rust):** unit test in `summary.rs` tests that `summarize()` sets
  `last_event_at` to the last real turn's timestamp and ignores a trailing
  `custom-title` / meta line. Follows the existing summary-test style.
- **Backend tail extraction:** if the last-event-from-tail logic is factored into a
  pure function, a unit test covering "tail with trailing meta line" guards it.
- **Frontend:** vitest for `deriveStatus` (running boundary at 10s, unread compare,
  null `last_event_at`) — matches the project's pure-function test convention
  (`lib/*.test.ts`).
- `cargo test`, `tsc --noEmit`, `vitest run`, and `vite build` all green.

## Non-goals

- **No global directory watcher.** Liveness is the lightweight poll; a shared
  cross-window watcher belongs to the separate concurrency TODO.
- **No cross-window sharing** of the poll — each window polls independently for now
  (cheap stats); deduplicating belongs to the concurrency TODO.
- **No "asking for input" state** — cut to two states.
- **No TUI changes** — `last_event_at` is additive to the core struct; the ratatui
  TUI ignores it.
