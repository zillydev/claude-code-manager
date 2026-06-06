# Show pre-compaction history — design

Date: 2026-06-06
Status: Approved (design phase complete)

## Problem

When a Claude Code session auto-compacts (runs out of context), CC does **not**
append the summary onto the existing conversation. It writes a brand-new root line
(`type:"system"`, `parentUuid:null`) into the **same** `.jsonl`, then an
`isCompactSummary:true` user message ("This session is being continued from a
previous conversation…"), then continues. The original pre-compaction conversation
stays in the same file as an **earlier, now-disconnected tree** (its own
null-parent root). The two trees share no nodes.

claude-manager renders only the *active path*: `resolve_leaf` takes the last
uuid-bearing line and `current_path` walks `parentUuid` back from it. That walk only
traverses the tree containing the global leaf — the post-compaction tree — so the
pre-compaction history is invisible in the UI even though it is fully present on disk.

Reference case — session `6180c55c-e7aa-416b-a7ec-8d0ec6a5f20b`:
- Pre-compaction tree: root `attachment 736bf121` @line 4 — **1268 messages**.
- Active tree: root `system 5279d258` @line 2203 (parent null) → compact summary
  `76b00368` @line 2204 → **3 messages**, contains the global leaf.
- 0 dangling-parent nodes; the algorithm below yields exactly these 2 segments.

## Goal

Render the **full** session — every conversation tree in the file, in chronological
order, with a clear compaction divider between them — so pre-compaction history is
viewable. The active (post-compaction) tree keeps every current behavior exactly;
earlier trees are **view-only**.

## Non-goals

- No editing/forking/deleting of pre-compaction messages (view-only).
- No change to stats, title, token counts, or the active segment's edit/fork indices.
- No virtualization or collapse-by-default (render inline-expanded for v1; collapse is
  a reversible later option if perf demands it).
- No backend mutation changes (surgery/commit still operate on the active path only).

## Architecture

### Core: `tree::segments` (new)

```rust
/// Every conversation tree in the file, oldest first, each as its own active path.
/// The LAST element is the active segment (the one containing `resolve_leaf`);
/// it is byte-identical to `current_path(events, leaf)`.
pub fn segments(events: &[Event]) -> Vec<Vec<&Event>>;
```

Algorithm:
1. Build `by_uuid: HashMap<Uuid, &Event>`.
2. `root_of(u)` — walk `parent_uuid` while the parent **exists in `by_uuid`**; the root
   is the last present node (its parent is `None` or absent from the file). Memoize
   per-uuid (avoid O(n²) on ~1500 nodes); cycle-guard.
   - A dangling-parent node (parent set but missing) becomes its own root. Real CC files
     don't have these (verified: 0 in the reference file), but the rule is well-defined.
3. Group uuid-bearing events by `root_of`. Each group is one tree (one compaction
   boundary; forks share a root and stay in one tree).
4. For each tree, the segment is `current_path(events, leaf_of_tree)`, where
   `leaf_of_tree` = the tree's last-in-file-order member. Within a tree this picks the
   active branch and ignores abandoned forks — same semantics as today.
5. **Identify the active segment as the tree whose root == `root_of(resolve_leaf(events))`.**
   Order the remaining segments by their root's file position (ascending) and place the
   active segment **last**. (Do not rely on the newest root sorting last — anchor on the
   global leaf so the edit-index invariant never depends on a sort.)

Single-tree files (no compaction) → one segment, identical to today's `current_path`.

### Core: `render_session` refactor

Extract the existing per-path turn-rendering body into a helper:

```rust
fn render_path_turns(path: &[&Event], editable: bool) -> Vec<RenderedTurn>;
```

It is the current rendering loop verbatim (suppressed-set, `msg_index`, assistant
merging, command/meta/synthetic skips), with one addition: each emitted `User`/
`Assistant` turn carries `editable`.

`render_session` then:
- `let segs = tree::segments(&session.events);`
- For each segment, `let is_active = /* segment is the active one */;` render via
  `render_path_turns(seg, is_active)`. Collect `tool_results` per segment
  (`collect_tool_results(seg)`).
- Insert a `RenderedTurn::Compaction { messages_before }` divider **before every
  non-first** rendered segment (`messages_before` = count of user/assistant turns in the
  segment just above the divider).
- Concatenate into the single flat `turns` vector.
- `stats` = `compute_stats(active_segment)` (unchanged — active path only).
- `title`, `event_count` unchanged.

The active segment is rendered **last** and is the only `editable` one, so its
`edit_index`/`fork_index` are computed over the active path exactly as today.

### Data model changes (`render.rs` + `types.ts`)

- New `RenderedTurn::Compaction { messages_before: usize }` → TS `{ type: "compaction";
  messages_before: number }`.
- Add `editable: bool` to `RenderedTurn::User` and `RenderedTurn::Assistant` → TS
  `editable: boolean` on those variants.

Single flat `turns` array (pre-compaction turns + dividers + active turns) is deliberate:
- **find covers the whole history for free** — data-seg-ids land on pre-compaction
  turns, `exactMatches` iterates the full array, and reveal-on-navigate keys by
  `turnIndex:blockIndex` which still resolves. `exactMatches`/`domMarkdownMatches` ignore
  the `compaction` turn type (no `data-seg-id`, not user/assistant), so seg-id ordinals
  stay consistent between data and DOM.
- The active turns' `edit_index`/`fork_index` are message indices into the active path,
  **independent of array position**, so prepending pre-compaction turns does not change
  them.

### webui (`Transcript.tsx`)

- Render `RenderedTurn` `compaction` as a centered divider, e.g.
  `⎯⎯ context compacted · {messages_before} messages above ⎯⎯`.
- **Gate every mutating control on `turn.editable`** (this is a guard against a
  data-corrupting action — a pre-compaction mutation would fire against the active path,
  hitting the wrong message or erroring). When `!editable`, render **no** `TurnActions`
  at all — hide edit, fork, delete, **and copy-id** (a pre-compaction uuid isn't on the
  active path, so copy-id is misleading).
- Confirm no mutating control uses the flat array index `i` — actions use
  `turn.edit_index` / `turn.fork_index` / `turn.uuid` (verify while wiring).
- Pre-compaction turns render through the same `Turn`/`BlockRenderer` components (so they
  get `data-seg-id` and are searchable). Inline-expanded; auto-scroll-to-bottom still
  lands on the active tip.

## Testing

**Automated (Rust, TDD):**
- `tree::segments`:
  - single linear tree → one segment == `current_path`.
  - two trees (compaction): a null-parent root A…→tipA, a second null-parent root B…→tipB
    appended after → two segments `[A.., B..]`, active = the one with the global leaf,
    correct membership and order.
  - fork within a tree (a→b, a→c) → still one segment (active branch only), not two.
  - dangling-parent node → becomes its own segment (documents the rule).
  - memoization/cycle guard doesn't hang on a self-referential parent.
- `render_session` **regression**: a single-tree (non-compacted) events list renders the
  **same turns** (count, types, and every `edit_index`/`fork_index`) as before this
  change. This pins the one failure mode that corrupts user data (editing the wrong
  message). 
- `render_session` two-tree: produces a `Compaction` divider between segments, pre-
  compaction turns `editable=false`, active turns `editable=true`.

**Integration / manual (the real verification):**
- Against the actual file `6180c55c…`: `segments` yields **exactly 2 segments** with
  **1268** and **3** messages, active = the 3-msg tree. (Synthetic unit tests pass by
  construction; this real-file check is what proves no dangling-parent fragmentation.)
- GUI on this 1271-turn session: pre-compaction history is visible above the divider,
  view-only (no action buttons), active conversation unchanged and still editable.
  **Specifically check find open-time and responsiveness** (react-markdown ×1268 eager +
  find's `[data-seg-id]` walk is the realistic stress case). If janky, collapse-by-default
  is the escape hatch — do not pre-build it.

## Follow-up

- After this lands, **update or delete** the `compaction-reroots-session-tree` memory —
  it asserts the claude-manager limitation "as of 2026-06-06", which this fix makes false.

## Risks / edge cases

- **Edit-index correctness** is the data-corruption risk; pinned by the single-tree
  regression test + anchoring the active segment on the global leaf + the `editable` gate.
- **Dangling parents** would fragment the display; well-defined (own root) and absent in
  real files; covered by a test that documents the behavior.
- **Performance** of eager-rendering ~1300 turns is consistent with today's
  non-virtualized rendering of large active paths; collapse is the reversible escape hatch.
