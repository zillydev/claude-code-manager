# claude-code-manager — Message bookmarks

**Date:** 2026-08-05
**Status:** Approved (user delegated detail decisions)
**Goal:** Flag individual messages and find them again — within a session and across all sessions.

## Decisions (from brainstorming)

| Decision | Choice | Rationale |
|---|---|---|
| Scope | Global collection from day one | Per-session jump list AND an all-sessions browser |
| Data | Bare flag + auto snippet | `bookmarks?: string[]` (message uuids) on `SessionEntry`; zero-friction add; schema can grow labels later without migration |
| Browser home | Overlay window | Matches LineageTree / TagManager pattern; reuses `openSession(s, scrollToMessage)` plumbing |
| Inheritance | None | Tags-only inheritance rule stands. (Verbatim fork preserves uuids, so fork-inheritance would work — deferred deliberately.) |

## Data model

- `SessionEntry.bookmarks?: string[]` — the **turn's head uuid** (`turn.uuid`), insertion order; views derive their own display order.
- `withEntry` prune/normalize includes bookmarks: empty list drops the field; an entry with only bookmarks survives.
- Pure `toggleBookmarkIn(meta, id, uuid)`; `toggleBookmark` on the `useSessionMeta` context. Broadcast rides the existing meta channel.

## UI

- **Toggle:** `TurnActions` ⋯ menu gains Bookmark / Remove bookmark on user + assistant turns. No `canMutate` needed (metadata-only, works on pre-compaction turns).
- **Marker:** accent ⚑ beside the role label on bookmarked turns (both chat sides, compact included).
- **In-session list:** header `⚑ n` button (hidden at zero) → dropdown in transcript order, snippets from rendered turns; click → scroll + flash via `[data-msg-uuids~=…]`.
- **Global browser:** sidebar-header ⚑ → overlay listing sessions that have bookmarks (discovered sessions only; trashed ones simply don't render), newest first, grouped `sessionTitle()` + project + count; rows = snippet (+ time). Click → `openSession(s, uuid)` — the lineage-tree path, flash included. Empty state points at the ⋯ menu.

## Backend

- One thin command `message_previews(file_path, uuids) → [{uuid, snippet, timestamp}]` wrapping the `store::message_snippets` walk, extended with each event's timestamp — for sessions that aren't open.

## Edge cases & testing

- Unresolvable uuid → "message not found" row, never silently dropped.
- Pure tests: toggle add/remove/prune/no-mutate; prune keeps a bookmarks-only entry.
- Rust test: previews return snippet + timestamp.

## Out of scope

Labels, bookmark search, export, inheritance — all addable without migration.
