# Cross-session compaction lineage — design

**Date:** 2026-06-17
**Status:** Approved (scope: banner + sidebar linkage, mirroring forkedFrom)

## Problem

When a session is `/compact`-ed (or otherwise compacted such that Claude Code
continues in a **new** session file), the conversation is split across two files:

- the **predecessor** holds the history up to the compaction point;
- the **successor** is a fresh file whose first message is the
  `isCompactSummary` "This session is being continued from a previous
  conversation…" text, followed by the continuation.

Concretely (the reported case): predecessor
`81375432-9de4-41b1-9cee-c94721b5a9f2` ran to the manual `/compact` point
(~"Task 4 spec review", 19:56); successor
`91052458-9e2f-4cb9-80dc-afe3b9ce904d` carries the continuation (Tasks 4-cq/5/6…
to 21:39). Opening either file shows only its half, so each looks like it is
"missing" history. Today claude-manager renders each file correctly and
completely — there is no link between the two halves.

This is distinct from in-file auto-compaction (where the pre-compaction tree is
orphaned in the *same* file and is already rendered with a divider via
`tree::segments`). This feature handles the **cross-file** case.

## The reliable signal

Claude Code writes a `system` event with `subtype: "compact_boundary"` at the
root of the continuation, carrying:

- `compactMetadata: { trigger: "manual" | "auto", … }`
- `logicalParentUuid`: the uuid of the message in the *source* session where
  compaction occurred.

For the successor (`91052458`), the root `compact_boundary`'s
`logicalParentUuid` (`8b65a417…`) is **not** present in its own file but **is**
a message in the predecessor (`81375432`). That gives a precise, data-driven
cross-file link — no content-matching heuristics (which we already proved
unreliable).

In-file compactions (the predecessor's own two `compact_boundary` records) have
`logicalParentUuid`s that *are* present in their own file, so they are correctly
ignored by the cross-file rule.

## Design

### 1. Parse: retain `logicalParentUuid` on system events

`SystemEvent` (`crates/core/src/session/event.rs`) gains:

```rust
#[serde(default, rename = "logicalParentUuid")]
pub logical_parent_uuid: Option<Uuid>,
```

Add a helper on the event/tree layer:

```rust
/// The cross-file compaction parent: the `logicalParentUuid` of a
/// `compact_boundary` system event whose logicalParentUuid is NOT one of this
/// session's own event uuids (i.e. it points into a different session file).
/// None for in-file compactions and non-compacted sessions.
pub fn compaction_logical_parent(events: &[Event]) -> Option<Uuid>
```

It scans for `Event::System` with `subtype == "compact_boundary"` and a
`logical_parent_uuid` not contained in the set of this session's uuids; returns
the first such uuid (there is normally exactly one, at the file root).

### 2. Resolve to a session id during discovery (per project dir)

`SessionSummary` gains `pub compacted_from: Option<String>` — the **predecessor
session id** (parallel to the existing `forked_from`). `summarize` initializes
it to `None`.

`store::discover` resolves it **within each project directory** (a compaction
continuation keeps the same cwd, so the predecessor is always in the same
encoded dir — this also bounds memory and keeps resolution O(dir)):

- While iterating a project dir, retain each session's `(id, events)`.
- Build a `HashMap<Uuid, String /*session id*/>` from that dir's event uuids.
- For each session, compute `compaction_logical_parent(events)`; if it resolves
  to another session id in the dir map, set that summary's `compacted_from`.

No global index, no second file read. The predecessor → successor (reverse)
direction is derived in the frontend (a session B is a successor of A iff
`B.compacted_from === A.id`), exactly like fork children.

### 3. Frontend: sidebar linkage + transcript banner

`types.ts`: `SessionSummary.compacted_from: string | null`.

**Sidebar** (`Sidebar.tsx`) — extend the existing forkedFrom linkage maps
(`byId`, `childrenBySource`, `relatedIds`, `hoveredId`) with a parallel
`compactedChildrenBySource` (group by `compacted_from`). Render, mirroring the
fork badges:

- on a successor row: `⮌ compacted from <predecessor title>` (click → select
  predecessor);
- on a predecessor row: `⟳ N` continuation badge (click/hover → successors);
- include both relations in the hover-highlight set.

**Transcript banner** — the open session's predecessor/successor are derived in
the component that has both the open session id and the summaries list (App /
Transcript header), from the `sessions` list:

- predecessor = `byId.get(open.compacted_from)`;
- successors = `sessions.filter(s => s.compacted_from === open.id)`.

Show a slim banner above the transcript:

- if predecessor: **"⮌ Continued (compacted) from `<title>` — view earlier
  history"** → opens the predecessor;
- if successor(s): **"Continues in `<title>` →"** → opens the successor.

Clicking a banner calls the existing select/open handler with the target's
`file_path` + `project_label`. No change to `RenderedSession`.

## Non-goals

- Rendering the predecessor's turns *inline* in the successor (the "one
  continuous transcript" option). Out of scope for this pass; the banner
  jump-through covers the need. Revisit if desired.
- Linking across more than one hop automatically (A→B→C). Each hop is its own
  `compacted_from`; the chain is naturally walkable by following banners.
- Any change to in-file (`tree::segments`) compaction rendering, which already
  works.

## Testing

- **Core unit tests:**
  - `compaction_logical_parent`: returns the cross-file uuid for a
    `compact_boundary` whose `logicalParentUuid` is absent from the file;
    returns `None` when the `logicalParentUuid` is present in-file, and when
    there is no `compact_boundary`.
  - `discover` resolution: a temp `projects/<dir>/` with two session files where
    B's `compact_boundary.logicalParentUuid` is a message uuid in A ⇒
    `B.compacted_from == Some(A.id)` and `A.compacted_from == None`; a control
    where the uuid is only in-file ⇒ both `None`.
- **Frontend:** `tsc` clean; existing 14 tests stay green (linkage is pure
  derivation; add a small test for the successor-derivation helper if extracted).
- **Manual:** open `91052458` → banner "Continued (compacted) from 81375432",
  click → lands on `81375432`; open `81375432` → "Continues in 91052458",
  click → lands on `91052458`; sidebar shows the badge + hover-highlight on both
  rows.
