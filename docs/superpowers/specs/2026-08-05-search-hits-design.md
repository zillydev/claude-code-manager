# claude-code-manager — Cross-session search hits

**Date:** 2026-08-05
**Status:** Approved (user delegated detail decisions)
**Goal:** "Better searching and filtering across sessions" — the sidebar search said only *that* a session matched; now it says why, and lands you on the match.

## Design

- `store::search_sessions_detailed(root, query) → Vec<SearchHit { id, count, first_uuid, snippet }>` replaces the id-set behind the `search_transcripts` command.
  - The cheap raw-line prefilter stays (skip JSON work unless the line contains the query at all).
  - A hit must appear in **conversation text** — a user message's string content or a text block, or an assistant text block. Tool inputs/results and machinery (uuids, base64) no longer match: precision is part of "better", so some former raw-grep matches disappear by design.
  - `count` = matching message lines; `first_uuid` + a `snippet` centred on the first match (~80 chars, whitespace collapsed, ellipses when clipped).
- Sidebar: a matching row (while content search is active) grows a dim match line — `N× · …snippet…` — and clicking it opens the session **scrolled to + flashing the first match** (`onSelect` gains an optional message uuid; App forwards it to `openSession`, the lineage-tree plumbing).

## Testing

Rust: text match counted with uuid + centred snippet; query appearing only in a tool result matches nothing; multi-message counts.

## Out of scope

Ranking, per-match navigation across sessions, regex/content operators, indexing.
