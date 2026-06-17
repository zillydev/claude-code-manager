# Session copy/fork lineage (shared-uuid overlap) — design

**Date:** 2026-06-17
**Status:** Draft (rewritten — supersedes the earlier `logicalParentUuid`
"clean compaction split" model, which was wrong; see Background)

## Background — why the first model was wrong

The first version of this spec assumed a session that begins with a
`compact_boundary` is a clean *successor* whose pre-compaction history lives in a
single *predecessor*, linkable via `compact_boundary.logicalParentUuid`.

Investigating the reported pair disproved that:

- `81375432` (A, foreground — no `sessionKind`) and `91052458`
  (B, `sessionKind: "bg"`, background) **share 383 byte-identical messages**
  (same `uuid`s), including the identical manual `/compact` boundary
  (`0495b1bc`, ts `17:56:16.750`). B = those 383 shared messages + 292 unique
  newer ones; A = the 383 + 1354 unique (its full pre-compaction history and its
  own later continuation).
- B's own text explains it: a **process restart** *"spun up a fresh session"*,
  which **fork-copied the live conversation into a new file** (reusing uuids).
- `logicalParentUuid` is **not** a reliable cross-session key — that uuid
  (`8b65a417`) was copied into *both* files. `forkedFrom` was **not** written on
  B. Per the Claude Code guide, the JSONL has **no documented back-pointer** for
  resume/background/restart copies.

So the reliable lineage signal is the **shared message-uuid overlap** itself:
when Claude Code forks/resumes/background-copies a session, the new file reuses
the **same message uuids** for the copied portion. This generalizes beyond
compaction to any copy (`/branch`, `--fork-session`, background/restart forks).

## Problem

Claude Code can persist one logical conversation into multiple session files
that share a block of identical-uuid messages. Today claude-manager shows these
as unrelated sessions, so the overlap reads as "duplicated" / "missing latest" /
"missing earlier" history depending on which file you open. We want to detect
the lineage and surface it, including how much overlaps.

## Detection signal & direction

**Link:** two sessions in the same project directory are linked if they share
message uuids. (Lineage always keeps the same cwd → same encoded project dir, so
detection is scoped per directory — bounding cost and matching the data.)

**Direction (ancestor vs descendant):** a copy's earliest content is the fork
point, so:

- Let `head(S)` = the root uuid of S's **first** conversation tree (the root of
  the first uuid-bearing event in file order).
- `S` is a **descendant (copy)** if `head(S)` is also present in another session
  `A` in the same dir. Then `A` is `S`'s **ancestor**, and
  `S.copied_from = A.id`.

In the reported case `head(B) = 0495b1bc`, which is present in A (as A's third
tree's root) but `head(A) = 2b7cf774` is **not** present in B — so B is the
descendant, A the ancestor. Correct.

Tie-breakers: if several sessions contain `head(S)`, pick the one with the
largest uuid-overlap with S. If two sessions each contain the other's head
(mutual), treat the one with more total messages as the ancestor.

## Design

### 1. Backend — detect per project directory in `discover`

`store::discover` already reads + parses every session file per directory. Today
it summarizes each file and discards the events; this feature retains the
directory's parsed events long enough to compute lineage, then drops them
(memory bounded to one directory at a time).

Per directory:

1. For each session, record `id`, the set of its message uuids, and `head(id)`
   (root uuid of its first tree, via the existing `tree`/parent-walk helpers).
2. Build `owners: HashMap<Uuid, Vec<session_id>>` over the directory's uuids.
3. For each session `S` with `forked_from == None` (an explicit
   Claude-Code-recorded fork already links it — don't double-link): look up
   `owners[head(S)]`; among other sessions there, choose the one `A` with the
   largest `|uuids(S) ∩ uuids(A)|`. If found, set
   `S.copied_from = Some(A.id)` and `S.copied_overlap = Some(that count)`.

This is O(directory events) to index plus one intersection per linked session.

### 2. Backend — `SessionSummary` fields

Add (parallel to the existing `forked_from`):

```rust
/// Ancestor session this one's history was copied from, inferred from shared
/// message uuids — fork / resume / background-restart copies that Claude Code
/// does not record with an explicit `forkedFrom`). None when not a copy or when
/// an explicit `forked_from` already links it.
pub copied_from: Option<String>,
/// Number of messages shared with `copied_from` (for display: "N shared").
pub copied_overlap: Option<usize>,
```

`summarize` initializes both to `None`; `discover` fills them per the algorithm
above. (`copied_from` is intentionally distinct from `forked_from`: the latter
is Claude Code's explicit field, the former is inferred. The frontend shows a
single lineage edge, preferring `forked_from` when present.)

### 3. Frontend — sidebar linkage + transcript banner

`types.ts`: add `copied_from: string | null` and `copied_overlap: number | null`
to `SessionSummary`.

**Sidebar** (`Sidebar.tsx`) — extend the existing forkedFrom linkage maps
(`byId`, `childrenBySource`, `relatedIds`, `hoveredId`) with a parallel
`copiedChildrenBySource` (group by `copied_from`), and include `copied_from`
edges in `relatedIds` so hover highlights both ends:

- descendant row badge: `⮌ copied from <ancestor title> · <N> shared`
  (click → select ancestor);
- ancestor row badge: `⧉ <k>` (k = number of sessions copied from it; click /
  hover → descendants);
- both directions participate in the hover-highlight ring.

**Transcript banner** — derived in the component that has the open session id
and the `sessions` list (App / Transcript header), from the summaries:

- if the open session has `copied_from`: a slim banner
  **"⮌ Copied from `<ancestor title>` — `<N>` earlier messages live there"**
  → opens the ancestor;
- if other sessions list it as their `copied_from`: **"Also continued as
  `<descendant title>` →"** (one per descendant) → opens that descendant.

Clicking calls the existing select/open handler with the target's `file_path` +
`project_label`. No change to `RenderedSession`.

Showing `<N> shared` directly surfaces the overlap that prompted this work, so
the user knows the relationship is a copy (not divergent history) and which file
holds the unique parts.

## Non-goals (possible follow-ups)

- **De-duplicating / collapsing the shared prefix** when viewing a descendant
  (e.g., folding the N copied messages with a "shared with `<ancestor>`"
  marker), or **stitching** ancestor-unique + descendant into one deduped
  transcript. Valuable but heavier; deferred until the link itself is in place.
- Cross-directory lineage (lineage keeps the same cwd, so out of scope).
- Multi-hop auto-flattening (each hop is its own `copied_from`; chains are
  walkable via the banners).

## Testing

- **Core unit tests** (synthetic `projects/<dir>/` with two `.jsonl` files):
  - B's first-tree root uuid is also a message in A, and B shares K uuids with A
    ⇒ `B.copied_from == Some(A.id)`, `B.copied_overlap == Some(K)`,
    `A.copied_from == None`.
  - Control: two sessions with **no** shared uuids ⇒ both `copied_from == None`.
  - Explicit-fork precedence: B already has `forked_from == Some(A.id)` ⇒
    `copied_from` stays `None` (no double-link).
  - Direction: ancestor (whose first-tree root is unique) is never marked as the
    descendant of its own copy.
- **Frontend:** `tsc` clean; existing 14 tests stay green; add a small test for
  the descendant-derivation helper if extracted.
- **Manual:** open `91052458` → banner "Copied from 81375432 — 383 earlier
  messages live there", click → lands on `81375432`; open `81375432` → "Also
  continued as 91052458 (background) →"; sidebar shows the badge + shared count +
  hover-highlight on both rows.
