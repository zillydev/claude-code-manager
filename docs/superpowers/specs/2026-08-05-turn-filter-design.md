# claude-code-manager — In-session turn filter

**Date:** 2026-08-05
**Status:** Approved (user delegated detail decisions)
**Goal:** "Better searching and filtering in a session" — the find bar already *searches*; what's missing is *narrowing the view*.

## Design

A **Filter** dropdown in the transcript header (next to Compact), single-select:

- **All turns** (default)
- **User messages** — user + command turns
- **Assistant replies** — assistant turns
- **Tool calls** — assistant turns containing at least one `tool_call` block
- **⚑ Bookmarked** — bookmarked turns only (uses the bookmarks feature)

Pure `filterTurns(turns, mode, bookmarks)` in `lib/turnFilter.ts`. Compaction dividers always survive (same rule as compact view). Trigger shows the active mode with accent styling when narrowed.

## Composition rules

- Pipeline: `filterTurns` → `filterCompact` (when compact on and find closed). Filter applies **before** compact, so compact's first/last-of-run logic operates on the narrowed set.
- **Find searches what you see:** `useFind` now receives the filtered list instead of `session.turns` — matches, counts, and seg-ids stay aligned with the rendered turns, so filter + find compose instead of conflicting.
- Filter state is a per-view gesture: component state, reset to All on session switch, not persisted.

## Testing

`filterTurns` tests: each mode, dividers kept, bookmarked-set intersection, all-mode identity (returns the same array so memoized turns don't re-render).

## Out of scope

Multi-select modes, persisted filter, per-tool-name filtering.
