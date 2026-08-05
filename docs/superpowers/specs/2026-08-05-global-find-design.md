# claude-code-manager — Global find (⌘⇧F)

**Date:** 2026-08-05 · **Status:** Approved
**Goal:** One comprehensive, Discord-style search across all sessions, with operators.

## Decisions (brainstormed)

- **Overlay on ⌘⇧F** + sidebar 🔍 button; the sidebar box returns to name/project filtering only — `search_transcripts` / `search_sessions_detailed` and the `N×·snippet` rows are removed (replaced wholesale).
- **Operators (all groups):** `in:` project, `from:` user|assistant, `before:`/`after:` yyyy-mm-dd on message ts (local midnight; after inclusive, before exclusive), `tag:` (AND-repeatable), `is:bookmarked`, `tool:<name>`, `has:link`, `session:` alias/title substring. Unknown `foo:bar` = literal text. Malformed dates = inline error. Free text = content match (conversation text only).
- **Enter to search**; live value autocomplete from memory for `in:`/`tag:`/`from:`/`is:`.

## Architecture

- `lib/searchQuery.ts` — pure parser (quoted values supported) → `SearchQuery`.
- Frontend prefilters (`in:`, `tag:`, `session:`, mtime cut for `after:`) resolve a candidate list of `{path, session_id, only_uuids?}`; `is:bookmarked` sets `only_uuids` from meta so the same backend path serves it (raw-line uuid prefilter keeps it cheap).
- `core::store::search_messages(files, filters) → (Vec<MessageHit{session_id, uuid, role, ts, snippet}>, truncated)`, cap 200. Raw-line prefilter for text; per-message checks for role/dates/tool/has:link; text confirmed in conversation text blocks only. Snippet centred on the match, else the message's first text.
- Empty text with no filters at all → prompt to add a term (no dump-everything walk).
- `GlobalFind.tsx` overlay: input + parsed-chips hint row + value suggestions; results grouped by session (newest hit first, transcript order within); click → `openSession(s, uuid)` + close. Truncation notice at cap.

## Testing

Parser (quoting, unknown keys, repeated tags, bad dates); core filters (role/date/tool/has_link/only_uuids, cap+truncated, text-block-only); command wiring via existing patterns.

## Out of scope

Relevance ranking, persistent indexes, cross-session match navigation, operator pills UI.
