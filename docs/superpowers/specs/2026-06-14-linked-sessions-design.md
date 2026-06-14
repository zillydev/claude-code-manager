# Highlight linked (forked) sessions — design

Date: 2026-06-14
Status: Approved

## Goal

Surface fork lineage in the sidebar: a derived session shows what it was forked
from, and hovering a session highlights its parent + children.

## Provenance: reuse Claude Code's `forkedFrom`

CC already records, on each copied message line, the source it was forked from:

```json
"forkedFrom": { "sessionId": "<source session>", "messageUuid": "<source message>" }
```

`sessionId` is the immediate source session (verified against a real `/branch`).
So CC `/branch` / `--fork-session` sessions are linkable with no heuristics. We
reuse this exact field — read it everywhere, and write it for claude-manager's
own forks/edits so they link the same way.

No content-matching, no new format, no `CommitOpts`/caller changes for the write
(the source id is derivable from the copied lines themselves).

## 1. Read (lights up existing CC forks)

- `event.rs`: add `ForkedFrom { #[serde(rename="sessionId")] session_id: String,
  #[serde(default, rename="messageUuid")] message_uuid: Option<String> }`, and a
  `#[serde(default, rename="forkedFrom")] forked_from: Option<ForkedFrom>` field on
  `UserEvent` and `AssistantEvent` (backward-compatible: defaults to `None`). Add
  `Event::forked_from_session(&self) -> Option<&str>` returning the source session id
  for User/Assistant variants. (Adding fields only affects parsing; `commit` writes
  via raw `serde_json::Value`, not these structs, and `Event` is never serialized to
  the frontend.)
- `summary.rs`: add `forked_from: Option<String>` to `SessionSummary`; in `summarize`,
  set it to the first non-null `ev.forked_from_session()` (the value is consistent
  across a file's lines).

## 2. Write (links claude-manager's own forks/edits)

In `edit.rs::commit`, entirely self-contained — no `CommitOpts` change:
- Before regenerating ids, capture `source_session = values.iter().find_map(|v|
  v.get("sessionId")...)` — the original session id of the copied lines (the source).
- In the uuid-regeneration pass, for each line: read its **original** `uuid` first,
  then after stamping the new identity, set
  `forkedFrom = {"sessionId": source_session, "messageUuid": original_uuid}` (overwriting
  any inherited `forkedFrom` — we link to the *immediate* parent). Only when
  `source_session` is `Some`.

This covers fork, edit, delete-message, and surgery (all go through `commit`). Forking
a fork links to the immediate parent; lineage is a chain.

## 3. Display (badge + hover-highlight)

`types.ts`: add `forked_from: string | null` to `SessionSummary`.

`Sidebar.tsx` builds two derived maps from the session list:
- `byId: Map<id, SessionSummary>` and `children: Map<sourceId, SessionSummary[]>`.

Per row:
- **Badge** when `s.forked_from` is set: a small `⎇ <source title>` chip
  (`byId.get(s.forked_from)?.title`, else the short source id). Clicking it selects the
  source via `onSelect(byId.get(s.forked_from))`. If the source isn't in the list
  (deleted/pruned) the chip shows the short id and is non-clickable.
- **Fork count** when `children.has(s.id)`: a `N forks` indicator.
- **Hover-highlight**: a `hoveredId` state; the related set = `{s.forked_from}` ∪
  `children.get(hovered).ids`. Rows in the related set get an accent (left-border /
  subtle bg). Cross-project links resolve fine — the list is global across projects.

## Scope (YAGNI)

- Immediate parent/children only (one hop). Full ancestor chains are derivable by
  following `forked_from` but are not built or rendered.
- No tree grouping/indentation (chosen badge + hover); the existing sort/filter is
  untouched.
- Display badge/count/highlight only on user/assistant-derived sessions; metadata-only
  edge cases degrade to the short-id chip.

## Testing

- Core (TDD):
  - `event.rs`: a line with `forkedFrom` parses; `Event::forked_from_session` returns the
    source id.
  - `summary.rs`: `summarize` populates `forked_from` from a `forkedFrom`-bearing event,
    and leaves it `None` otherwise.
  - `edit.rs`: `commit` of a source session's lines emits lines whose `forkedFrom.sessionId`
    equals the source session id (and a new, different `sessionId`).
- Frontend: manual — badge shows the source title and click-jumps; a source shows its
  fork count; hovering highlights parent + children; CC `/branch` sessions already link.

## Risks / edge cases

- Source deleted → chip shows short id, non-clickable (handled).
- Source in another project → resolved (global list).
- Re-forking a fork → `commit` overwrites inherited `forkedFrom` with the immediate
  parent, so no stale links.
- `forkedFrom` is per-line in CC; we read the first and (on write) stamp every line,
  matching CC.
