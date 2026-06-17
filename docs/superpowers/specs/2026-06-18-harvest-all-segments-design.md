# Harvest across all segments (v2) — design

**Date:** 2026-06-18
**Status:** Approved

## Problem

`session-harvest` (v1) reads only the **active path** of a session. For a session
that has been (re-)compacted, the active path is just the latest segment, so
harvest misses the orphaned **pre-compaction trees** — which is where much of the
reusable work lives. The user's richest sessions are heavily compacted, so this
limitation is significant. v2 lets harvest mine **every conversation tree** in a
session.

This is the v1 spec's stated non-goal ("pre-compaction orphaned trees out of
scope") being lifted.

## Background

A compacted `.jsonl` contains multiple disconnected conversation trees: each
auto/manual compaction writes a fresh null-parent root, orphaning the prior tree
in the same file. `session::tree::segments(events)` already groups `Event`s into
these trees (non-active first in file order, active last). `surgery` operates on
`RawLine`s (which carry `uuid` and `parent`) and currently only uses
`raw::active_path` — the active tree alone.

## Design

### 1. `raw::segments` — raw-line tree grouping

Add `pub fn segments(lines: &[RawLine]) -> Vec<Vec<&RawLine>>` to
`crates/core/src/session/raw.rs`, mirroring `tree::segments` but on `RawLine`
(using `.uuid` / `.parent`): group uuid-bearing lines by tree root (walk `parent`
while present; cycle-guarded), order the non-active roots first in file order and
the active root (the one containing the global leaf) last, and for each root emit
that tree's path via the existing `raw::path_to(lines, tree_leaf)`. Single-tree
sessions return one segment equal to `active_path`.

### 2. `extract` gains `--all-segments` (default OFF)

The flag is the safety boundary: **session-surgery never passes it, so its
active-path behavior is exactly preserved.** Only session-harvest opts in.

`surgery::extract` signature becomes:

```rust
pub fn extract(
    path: &Path,
    from: Option<Uuid>,
    to: Option<Uuid>,
    outline: bool,
    all_segments: bool,
) -> Result<String>
```

Behavior:

- **`all_segments = false`** (unchanged): operate on `active_path` exactly as today
  (whole active path when no range; turn-snapped range otherwise; outline or full).
- **`all_segments = true`, no `from`/`to`:** iterate `raw::segments`; before each
  segment emit a divider line —
  `── segment <k>/<N> · pre-compaction ──` for all but the last, `· active` for
  the last — then render that segment (outline or full text).
- **`all_segments = true`, with `from`/`to`:** find the segment containing both
  uuids and snap/render the range within it (a range never crosses a compaction
  boundary). If `from` and `to` fall in different segments, error
  (`"from and to are in different conversation trees"`).

Rendering reuses the existing `render_range_text` / `render_outline` per segment;
only the segment iteration + dividers are new.

### 3. CLI

`crates/tui/src/main.rs` `Extract` subcommand gains `--all-segments` (bool);
the handler passes it through to `surgery::extract`.

### 4. Skill update (`session-harvest`)

- The outline command becomes
  `surgery extract --session X --outline --all-segments` and the drill-down
  `surgery extract --session X --from F --to T --all-segments`, so harvest sees
  and targets every tree.
- The outline now contains segment dividers; the navigation section notes that
  candidates may come from any segment, and each discovery candidate is tagged
  with its segment (e.g. `seg 1/3 · pre-compaction`).
- Remove the v1 "active path only" limitation note.

session-surgery's SKILL.md is **not** changed (it does not pass the flag).

## Non-goals

- Changing `extract`'s default (active path stays default; harvest opts in).
- Cross-file lineage during harvest (that's the copied_from feature; harvest
  still reads a single session file — now all its trees).
- Ranges that span a compaction boundary (errored, not silently merged).

## Testing

- **`raw::segments` unit tests:** a two-root fixture (a pre-compaction tree + an
  active tree appended) → 2 segments, active last; a single linear session → 1
  segment equal to `active_path`; a dangling-parent root starts a new segment.
- **`extract --all-segments` tests:**
  - outline over a two-tree fixture contains both trees' turns and a
    `── segment 1/2 · pre-compaction ──` divider;
  - `from`/`to` inside the pre-compaction tree resolves and renders that range;
  - `from` in one tree + `to` in another errors.
- **Backward-compat:** the existing `extract_*` tests (no flag) stay green
  unchanged — active-path behavior preserved for session-surgery.
- **Manual:** run `surgery extract --session <a compacted session> --outline
  --all-segments` and confirm the pre-compaction tree's turns now appear with a
  divider; harvest discovery on that session lists candidates from pre-compaction
  spans.
