# Inline "forked from here" markers — design

**Date:** 2026-06-17
**Status:** Approved

## Problem

The transcript already shows an inline `Compaction` divider where a session was
compacted. There is no equivalent marker showing where *other* sessions branched
off this one. The user wants, at the message a child session forked from, an
inline line/button — "⑂ N forked from here" — that links to those children.
This is the inverse of the sidebar's "⮌ from `<source>`" badge (shown on the
child): here the marker is shown on the **parent**, at the branch point.

Scope (decided): cover **both** explicit forks (Claude Code `/branch` and
claude-manager "fork from here", which record `forkedFrom.messageUuid`) **and**
the copied lineage (resume / background-restart copies detected via shared-uuid
overlap, whose branch point is the copy's shared root message).

## Why frontend-driven

The marker depends on *other* sessions — the children that branched from this
one — which `render_session` cannot see (it renders a single session). So, unlike
the `Compaction` divider (intrinsic to one session's tree, inserted by the
backend), this marker is injected in the **frontend** from the sidebar's session
list. The backend's only job is to expose, per session, *where in its parent* it
branched (a message uuid). An alternative — a new backend `RenderedTurn` variant
— was rejected because the cross-session child list isn't available to
`render_session`.

## Design

### 1. Backend — branch-point message uuids on `SessionSummary`

Two additive `Option<String>` fields (parallel to the existing `forked_from` /
`copied_from` session-id fields):

- `forked_from_message: Option<String>` — the parent message uuid this session
  forked at, read from `forkedFrom.messageUuid`. Set in `summarize` from the same
  `forkedFrom` provenance that yields `forked_from` (a new `Event::forked_from()`
  accessor returning `Option<&ForkedFrom>`, or a `forked_from_message()` helper,
  exposes the `message_uuid`).
- `copied_from_message: Option<String>` — for a detected copy, the parent message
  uuid where it began = the copy's **head** uuid (its first-tree root, which is a
  message present in the ancestor — that is how the copy was detected). Set in
  `store::discover` at the same point it sets `copied_from`, using the
  `SessionUuids.head` already computed for that session.

`summarize` initializes `copied_from_message` to `None`; `discover` fills it.

### 2. Frontend — derive branch points in `App`

`types.ts`: add `forked_from_message: string | null` and
`copied_from_message: string | null` to `SessionSummary`.

`App`, for the open session `A` (`transcript.id`), builds from the `sessions`
list:

```
branchPoints: Map<messageUuid, Array<{ session: SessionSummary; kind: "forked" | "copied" }>>
```

- for each `s` where `s.forked_from === A.id` and `s.forked_from_message`:
  append `{ s, "forked" }` under `s.forked_from_message`;
- for each `s` where `s.copied_from === A.id` and `s.copied_from_message`:
  append `{ s, "copied" }` under `s.copied_from_message`.

`App` passes `branchPoints` and an `onOpenSession(s)` handler (the existing
`open(s.file_path, s.project_label)`) to `Transcript`.

### 3. Transcript — render the inline marker

`Transcript` gains two optional props: `branchPoints` and `onOpenSession`. In the
`visibleTurns.map` loop, after each `<Turn>`, if the turn's `fork_uuid` or `uuid`
is a key in `branchPoints`, render a sibling `<ForkMarker>` element (keyed
separately so the memoized `Turn` is untouched — no perf regression).

`ForkMarker` mirrors the compaction divider's centered-line style:

- a centered rule with a label "⑂ `<N>` forked from here";
- one clickable chip per child: the child's title, click → `onOpenSession(child)`;
- chips tagged subtly by `kind` ("copied" children read e.g. "`<title>` (copy)").

Matching is best-effort against rendered turn boundaries (`fork_uuid` then
`uuid`). A branch point that matches no rendered turn (e.g. a copy whose head is a
system `compact_boundary`, not a user/assistant turn) is **omitted inline** — the
relationship is still visible via the lineage banner and the sidebar badge. This
keeps the matching simple (the frontend only sees turn-level uuids, not every
merged event uuid).

### Data flow

```
SessionSummary.forked_from + forked_from_message   (explicit fork: parent id + branch msg)
SessionSummary.copied_from  + copied_from_message   (copy: ancestor id + head/branch msg)
        │  (sidebar list, in App)
        ▼
App: branchPoints = Map<branchMsgUuid, {child, kind}[]>  for the open session
        │  prop
        ▼
Transcript loop: turn.fork_uuid|uuid ∈ branchPoints  →  <ForkMarker> after the turn
        │  chip click
        ▼
onOpenSession(child) → open(child.file_path, child.project_label)
```

## Non-goals

- A backend `RenderedTurn` variant for fork points (cross-session data isn't
  available to `render_session`).
- Resolving branch points that fall on non-turn events (mid-assistant merged
  events, or a copy's system-boundary head) to a nearby turn — omitted inline,
  still surfaced elsewhere.
- Any change to how forks/copies are *created* or to the sidebar badges / lineage
  banner (already shipped).

## Testing

- **Core unit tests:**
  - `summarize` sets `forked_from_message` from `forkedFrom.messageUuid` (and
    `None` when absent); `forked_from` unchanged.
  - `discover` sets `copied_from_message` to the descendant's head uuid for a
    detected copy (extend the existing `discover_sets_copied_from_via_shared_uuid`
    fixture to also assert `copied_from_message == Some(<B's head>)`).
- **Frontend:** `tsc` clean; existing 14 tests stay green; add a small unit test
  for the `branchPoints` derivation helper if it is extracted to a pure function.
- **Manual:** open a parent session that has children forked from a specific
  message (e.g. one of the user's real "fork from here" sessions) → an inline
  "⑂ N forked from here" line appears just after that turn, and each chip opens
  the corresponding child. Open `81375432` → if the copy's head matches a turn, a
  "copied from here" marker appears at it (otherwise it remains in the banner).
