# Show Pre-compaction History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render every conversation tree in a session file (one extra per auto-compaction), oldest first, with a compaction divider — so the pre-compaction history is viewable. The active tree is unchanged and editable; earlier trees are view-only.

**Architecture:** A new `tree::segments` groups uuid-bearing events by their root (walk `parent_uuid` to the last present node), one segment per tree, with the active segment (the one containing `resolve_leaf`) rendered last. `render_session` renders each segment via an extracted helper, inserts a `Compaction` divider between segments, and marks only the active segment's turns `editable`. The webui renders the divider and hides all mutating controls — and the `editIndex` itself — when a turn isn't editable.

**Tech Stack:** Rust (claude-manager-core), React 19 + TypeScript (webui). Rust tests via `cargo test -p claude-manager-core`. Frontend type-check via `tsc`.

---

## File Structure

- `crates/core/src/session/tree.rs` — add `segments()` + unit tests (new logic).
- `crates/core/src/session/render.rs` — add `RenderedTurn::Compaction`, add `editable` to `User`/`Assistant`, extract `render_path_turns`, rewrite `render_session` to loop segments; add unit tests.
- `crates/webui/frontend/src/types.ts` — add `compaction` variant + `editable` field.
- `crates/webui/frontend/src/components/Transcript.tsx` — gate `editIndex` on editable, gate `TurnActions` on editable, render the `compaction` divider.

---

## Task 1: `tree::segments`

**Files:**
- Modify: `crates/core/src/session/tree.rs`

- [ ] **Step 1: Add the failing tests**

In `crates/core/src/session/tree.rs`, inside the existing `mod tests { ... }` (after the last test, before the closing `}`), add:

```rust
    #[test]
    fn segments_single_tree_equals_current_path() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let events = vec![user(a, None), user(b, Some(a)), user(c, Some(b))];
        let segs = segments(&events);
        assert_eq!(segs.len(), 1);
        let cp = current_path(&events, resolve_leaf(&events).unwrap());
        let seg_ids: Vec<_> = segs[0].iter().map(|e| e.uuid()).collect();
        let cp_ids: Vec<_> = cp.iter().map(|e| e.uuid()).collect();
        assert_eq!(seg_ids, cp_ids);
    }

    #[test]
    fn segments_two_roots_active_last() {
        // tree 1: a -> b ; tree 2 (new null-parent root, appended after): c -> d
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let d = "00000000-0000-0000-0000-0000000000d4";
        let events = vec![
            user(a, None),
            user(b, Some(a)),
            user(c, None),
            user(d, Some(c)),
        ];
        let segs = segments(&events);
        assert_eq!(segs.len(), 2);
        let ids = |seg: &Vec<&Event>| -> Vec<String> {
            seg.iter().filter_map(|e| e.uuid()).map(|u| u.to_string()).collect()
        };
        assert_eq!(ids(&segs[0]), vec![a.to_string(), b.to_string()]);
        assert_eq!(ids(&segs[1]), vec![c.to_string(), d.to_string()]); // active = global leaf d
    }

    #[test]
    fn segments_fork_within_tree_is_one_segment() {
        // a -> b -> cc (abandoned), a -> d (active). One root (a) => one segment a,d.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let cc = "00000000-0000-0000-0000-0000000000c3";
        let d = "00000000-0000-0000-0000-0000000000d4";
        let events = vec![
            user(a, None),
            user(b, Some(a)),
            user(cc, Some(b)),
            user(d, Some(a)),
        ];
        let segs = segments(&events);
        assert_eq!(segs.len(), 1);
        let ids: Vec<String> =
            segs[0].iter().filter_map(|e| e.uuid()).map(|u| u.to_string()).collect();
        assert_eq!(ids, vec![a.to_string(), d.to_string()]);
    }

    #[test]
    fn segments_dangling_parent_starts_new_tree() {
        // x's parent points at a uuid not present in the file -> x is its own root.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let x = "00000000-0000-0000-0000-0000000000e5";
        let missing = "00000000-0000-0000-0000-0000000000ff";
        let events = vec![user(a, None), user(b, Some(a)), user(x, Some(missing))];
        let segs = segments(&events);
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn segments_self_cycle_terminates() {
        let z = "00000000-0000-0000-0000-0000000000aa";
        let events = vec![user(z, Some(z))];
        let segs = segments(&events); // must not hang
        assert_eq!(segs.len(), 1);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p claude-manager-core segments_ 2>&1 | tail -20`
Expected: FAIL — `cannot find function segments in this scope`.

- [ ] **Step 3: Implement `segments`**

In `crates/core/src/session/tree.rs`, add this function after `current_path` (before the `#[cfg(test)]` module):

```rust
/// Every conversation tree in the file, oldest first; each is the active path of
/// one root. The LAST element is the active segment — the tree containing
/// `resolve_leaf` — and equals `current_path(events, leaf)`.
///
/// Events are grouped by their root: walk `parent_uuid` while the parent is
/// present in the file; the root is the last present node (parent `None` or
/// absent). Forks share a root and stay in one segment; only a new null-parent
/// (or dangling-parent) root starts a new segment — which is what an
/// auto-compaction writes (a fresh root + the "continued from a previous
/// conversation" summary in the same file).
pub fn segments(events: &[Event]) -> Vec<Vec<&Event>> {
    let mut by_uuid: HashMap<Uuid, &Event> = HashMap::with_capacity(events.len());
    for ev in events {
        if let Some(u) = ev.uuid() {
            by_uuid.insert(u, ev);
        }
    }
    if by_uuid.is_empty() {
        return Vec::new();
    }

    // Memoized root resolution (avoid O(n^2) on large files); cycle-guarded.
    fn root_of(
        start: Uuid,
        by_uuid: &HashMap<Uuid, &Event>,
        cache: &mut HashMap<Uuid, Uuid>,
    ) -> Uuid {
        if let Some(r) = cache.get(&start) {
            return *r;
        }
        let mut chain: Vec<Uuid> = Vec::new();
        let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        let mut cur = start;
        let root = loop {
            if !seen.insert(cur) {
                break cur; // cycle: treat current node as the root
            }
            chain.push(cur);
            match by_uuid.get(&cur).and_then(|e| e.parent_uuid()) {
                Some(p) if by_uuid.contains_key(&p) => cur = p,
                _ => break cur, // parent None or missing -> cur is the root
            }
        };
        for c in chain {
            cache.insert(c, root);
        }
        root
    }

    // Group uuid-bearing events by root, in file order, recording each root's
    // first-seen position (= the root node's file position, since ancestors are
    // written before descendants).
    let mut cache: HashMap<Uuid, Uuid> = HashMap::with_capacity(by_uuid.len());
    let mut groups: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    let mut root_order: Vec<Uuid> = Vec::new();
    for ev in events {
        let Some(u) = ev.uuid() else { continue };
        let r = root_of(u, &by_uuid, &mut cache);
        if !groups.contains_key(&r) {
            root_order.push(r);
        }
        groups.entry(r).or_default().push(ev);
    }

    // Active tree = the one containing the global leaf; render it last.
    let active_root = resolve_leaf(events).map(|l| root_of(l, &by_uuid, &mut cache));
    let mut ordered: Vec<Uuid> =
        root_order.iter().copied().filter(|r| Some(*r) != active_root).collect();
    if let Some(ar) = active_root {
        ordered.push(ar);
    }

    let mut out: Vec<Vec<&Event>> = Vec::with_capacity(ordered.len());
    for r in ordered {
        // The tree's leaf is its last-in-file-order member.
        if let Some(tree_leaf) = groups[&r].last().and_then(|e| e.uuid()) {
            out.push(current_path(events, tree_leaf));
        }
    }
    out
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p claude-manager-core segments_ 2>&1 | tail -20`
Expected: PASS — all 5 `segments_*` tests green.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/session/tree.rs
git commit -m "feat(core): tree::segments — split a session file into its conversation trees"
```

---

## Task 2: render.rs — Compaction divider, editable, segment loop

**Files:**
- Modify: `crates/core/src/session/render.rs`

- [ ] **Step 1: Add the `Compaction` variant and `editable` field to `RenderedTurn`**

In `crates/core/src/session/render.rs`, change the `User` and `Assistant` variants to add `editable: bool`, and add a `Compaction` variant. Replace the `RenderedTurn` enum (currently lines ~47-85) with:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum RenderedTurn {
    #[serde(rename = "user")]
    User {
        timestamp: Option<DateTime<Utc>>,
        text: String,
        /// The message line's event uuid (empty if absent).
        uuid: String,
        /// 0-based index among user/assistant message lines on the active path.
        /// Used directly as both the edit target and the fork point.
        edit_index: usize,
        fork_index: usize,
        /// True only on the active (post-compaction) segment. Pre-compaction
        /// turns are view-only; their indices are segment-local and must not be
        /// used by the backend.
        editable: bool,
    },
    #[serde(rename = "assistant")]
    Assistant {
        timestamp: Option<DateTime<Utc>>,
        model: Option<String>,
        blocks: Vec<RenderedBlock>,
        /// The FIRST merged assistant event's uuid (the head of the turn).
        uuid: String,
        /// Message index of the FIRST merged assistant event (the one whose
        /// first text block is shown/edited).
        edit_index: usize,
        /// Message index of the LAST merged assistant event, so a fork includes
        /// the whole reply plus its interleaved tool-result lines.
        fork_index: usize,
        editable: bool,
    },
    #[serde(rename = "system")]
    System {
        timestamp: Option<DateTime<Utc>>,
        content: String,
    },
    #[serde(rename = "summary")]
    Summary {
        timestamp: Option<DateTime<Utc>>,
        content: String,
    },
    /// A boundary between two conversation trees in the same file (an
    /// auto-compaction point). `messages_before` counts the user/assistant turns
    /// in the segment immediately above this divider.
    #[serde(rename = "compaction")]
    Compaction { messages_before: usize },
}
```

- [ ] **Step 2: Extract `render_path_turns` and rewrite `render_session`**

Replace the entire `render_session` function (currently lines ~110-327, from the `/// Render a fully-parsed session...` doc comment through its closing `}`) with the following **two** functions. `render_path_turns` is the existing turn-building body moved verbatim, parameterized by `path` and `editable`, with `editable` added to the two `turns.push(...)` calls; `render_session` loops segments.

```rust
/// Render one conversation path (a single tree's active path) into turns.
/// `editable` marks whether these turns belong to the active segment.
fn render_path_turns(path: &[&Event], editable: bool) -> Vec<RenderedTurn> {
    let tool_results = collect_tool_results(path);

    // Track user events that are pure tool-result containers.
    let mut suppressed: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (idx, ev) in path.iter().enumerate() {
        if let Event::User(u) = ev {
            if let MessageContent::Blocks(blocks) = &u.message.content {
                if !blocks.is_empty()
                    && blocks
                        .iter()
                        .all(|b| matches!(b, UserBlock::ToolResult { .. }))
                {
                    suppressed.insert(idx);
                }
            }
        }
    }

    let mut turns: Vec<RenderedTurn> = Vec::new();

    // Per-event message index over ALL user/assistant events on this path.
    let msg_index: Vec<Option<usize>> = {
        let mut out = Vec::with_capacity(path.len());
        let mut m = 0usize;
        for ev in path.iter() {
            match ev {
                Event::User(_) | Event::Assistant(_) => {
                    out.push(Some(m));
                    m += 1;
                }
                _ => out.push(None),
            }
        }
        out
    };

    let render_assistant_blocks = |a: &super::event::AssistantEvent| -> Vec<RenderedBlock> {
        let mut blocks = Vec::new();
        for blk in &a.message.content {
            match blk {
                Block::Text { text } => {
                    blocks.push(RenderedBlock::Text { text: text.clone() });
                }
                Block::Thinking { thinking } => {
                    blocks.push(RenderedBlock::Thinking {
                        thinking: thinking.clone(),
                    });
                }
                Block::ToolUse { id, name, input } => {
                    let result = tool_results.get(id.as_str()).map(|r| ToolCallResult {
                        text: r.text.clone(),
                        is_error: r.is_error,
                    });
                    blocks.push(RenderedBlock::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input_summary: summarize_tool_input(name, input),
                        input_full: input.clone(),
                        result,
                    });
                }
                Block::Unknown => {}
            }
        }
        blocks
    };

    let mut i = 0;
    while i < path.len() {
        let ev = path[i];
        if suppressed.contains(&i) {
            i += 1;
            continue;
        }
        match ev {
            Event::User(u) => {
                if u.is_meta.unwrap_or(false) {
                    i += 1;
                    continue;
                }
                let text = match &u.message.content {
                    MessageContent::Text(t) => t.clone(),
                    MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            UserBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                if text.starts_with("<command-") || text.starts_with("<local-command-") {
                    i += 1;
                    continue;
                }
                let idx = msg_index[i].expect("user event has a message index");
                turns.push(RenderedTurn::User {
                    timestamp: ev.timestamp(),
                    text,
                    uuid: ev.uuid().map(|u| u.to_string()).unwrap_or_default(),
                    edit_index: idx,
                    fork_index: idx,
                    editable,
                });
                i += 1;
            }
            Event::Assistant(a) => {
                if a.message.model.as_deref() == Some("<synthetic>") {
                    i += 1;
                    continue;
                }
                let timestamp = ev.timestamp();
                let head_uuid = ev.uuid().map(|u| u.to_string()).unwrap_or_default();
                let mut model = a.message.model.clone();
                let mut blocks = render_assistant_blocks(a);
                let first_msg_i = msg_index[i].expect("assistant event has a message index");
                let mut last_msg_i = first_msg_i;
                let block_has_text =
                    |bs: &[RenderedBlock]| bs.iter().any(|b| matches!(b, RenderedBlock::Text { .. }));
                let mut edit_msg_i = if block_has_text(&blocks) {
                    Some(first_msg_i)
                } else {
                    None
                };
                i += 1;
                while i < path.len() {
                    if suppressed.contains(&i) {
                        i += 1;
                        continue;
                    }
                    if let Event::Assistant(next_a) = path[i] {
                        if next_a.message.model.as_deref() == Some("<synthetic>") {
                            i += 1;
                            continue;
                        }
                        let next_blocks = render_assistant_blocks(next_a);
                        let this_msg_i = msg_index[i].expect("assistant event has a message index");
                        if edit_msg_i.is_none() && block_has_text(&next_blocks) {
                            edit_msg_i = Some(this_msg_i);
                        }
                        blocks.extend(next_blocks);
                        if model.is_none() {
                            model = next_a.message.model.clone();
                        }
                        last_msg_i = this_msg_i;
                        i += 1;
                    } else {
                        break;
                    }
                }
                turns.push(RenderedTurn::Assistant {
                    timestamp,
                    model,
                    blocks,
                    uuid: head_uuid,
                    edit_index: edit_msg_i.unwrap_or(first_msg_i),
                    fork_index: last_msg_i,
                    editable,
                });
            }
            Event::System(s) => {
                if s.subtype.as_deref() == Some("local_command")
                    && s.content.contains("<command-name>")
                {
                    turns.push(RenderedTurn::System {
                        timestamp: ev.timestamp(),
                        content: s.content.clone(),
                    });
                }
                i += 1;
            }
            Event::Summary(s) => {
                turns.push(RenderedTurn::Summary {
                    timestamp: ev.timestamp(),
                    content: s.summary.clone().unwrap_or_default(),
                });
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    turns
}

/// Render a fully-parsed session into a flattened, frontend-ready structure.
/// Renders every conversation tree in the file (one extra per auto-compaction),
/// oldest first, with a `Compaction` divider between trees. Only the active
/// (last) segment is editable; stats/title/indices come from the active path.
pub fn render_session(session: &Session) -> RenderedSession {
    let segs = super::tree::segments(&session.events);
    let active_idx = segs.len().saturating_sub(1);

    let mut turns: Vec<RenderedTurn> = Vec::new();
    let mut prev_seg_msgs = 0usize;
    for (s, seg) in segs.iter().enumerate() {
        if s > 0 {
            turns.push(RenderedTurn::Compaction { messages_before: prev_seg_msgs });
        }
        let seg_turns = render_path_turns(seg, s == active_idx);
        prev_seg_msgs = seg_turns
            .iter()
            .filter(|t| matches!(t, RenderedTurn::User { .. } | RenderedTurn::Assistant { .. }))
            .count();
        turns.extend(seg_turns);
    }

    let empty: Vec<&Event> = Vec::new();
    let active_path = segs.get(active_idx).unwrap_or(&empty);
    let stats = compute_stats(active_path);

    RenderedSession {
        project_label: session.project_label.clone(),
        id: session.id.to_string(),
        source_path: session.file_path.to_string_lossy().into_owned(),
        title: session_title(session),
        event_count: session.events.len(),
        stats,
        turns,
    }
}
```

- [ ] **Step 3: Add render tests**

In `crates/core/src/session/render.rs`, add at the end of the file:

```rust
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::session::event::{Message, MessageContent, UserEvent};
    use crate::session::Session;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::time::SystemTime;
    use uuid::Uuid;

    fn uev(uuid: &str, parent: Option<&str>, text: &str) -> Event {
        Event::User(UserEvent {
            uuid: Some(uuid.parse().unwrap()),
            parent_uuid: parent.map(|p| p.parse().unwrap()),
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            is_meta: None,
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Text(text.into()),
            },
            tool_use_result: None,
            cwd: None,
            git_branch: None,
        })
    }

    fn sess(events: Vec<Event>) -> Session {
        let leaf = crate::session::tree::resolve_leaf(&events);
        Session {
            project_label: "p".into(),
            id: Uuid::nil(),
            file_path: PathBuf::from("/x.jsonl"),
            events,
            leaf,
            mtime: SystemTime::now(),
            bytes_read: 0,
        }
    }

    #[test]
    fn single_tree_has_no_divider_sequential_editable_indices() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let r = render_session(&sess(vec![
            uev(a, None, "one"),
            uev(b, Some(a), "two"),
            uev(c, Some(b), "three"),
        ]));
        assert!(!r.turns.iter().any(|t| matches!(t, RenderedTurn::Compaction { .. })));
        let users: Vec<(usize, bool)> = r
            .turns
            .iter()
            .filter_map(|t| match t {
                RenderedTurn::User { edit_index, editable, .. } => Some((*edit_index, *editable)),
                _ => None,
            })
            .collect();
        assert_eq!(users, vec![(0, true), (1, true), (2, true)]);
    }

    #[test]
    fn two_trees_get_divider_precompaction_viewonly_active_indices_reset() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let d = "00000000-0000-0000-0000-0000000000d4";
        // pre-compaction tree a->b ; active tree (new root) c->d.
        let r = render_session(&sess(vec![
            uev(a, None, "old1"),
            uev(b, Some(a), "old2"),
            uev(c, None, "new1"),
            uev(d, Some(c), "new2"),
        ]));
        let mut it = r.turns.iter();
        match it.next().unwrap() {
            RenderedTurn::User { editable, text, .. } => {
                assert!(!editable);
                assert_eq!(text, "old1");
            }
            other => panic!("expected pre-compaction User, got {other:?}"),
        }
        match it.next().unwrap() {
            RenderedTurn::User { editable, .. } => assert!(!editable),
            other => panic!("expected pre-compaction User, got {other:?}"),
        }
        match it.next().unwrap() {
            RenderedTurn::Compaction { messages_before } => assert_eq!(*messages_before, 2),
            other => panic!("expected Compaction, got {other:?}"),
        }
        match it.next().unwrap() {
            RenderedTurn::User { editable, edit_index, text, .. } => {
                assert!(editable);
                assert_eq!(*edit_index, 0); // active indices reset, not continued from pre-compaction
                assert_eq!(text, "new1");
            }
            other => panic!("expected active User, got {other:?}"),
        }
        match it.next().unwrap() {
            RenderedTurn::User { editable, edit_index, .. } => {
                assert!(editable);
                assert_eq!(*edit_index, 1);
            }
            other => panic!("expected active User, got {other:?}"),
        }
        assert!(it.next().is_none());
    }
}
```

- [ ] **Step 4: Run the render + segments tests**

Run: `cargo test -p claude-manager-core 2>&1 | tail -25`
Expected: PASS — all existing tests plus the new `segments_*` and `render_tests::*` green.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/session/render.rs
git commit -m "feat(core): render all conversation trees with compaction dividers; mark active editable"
```

---

## Task 3: webui types — compaction variant + editable

**Files:**
- Modify: `crates/webui/frontend/src/types.ts`

- [ ] **Step 1: Update the `RenderedTurn` union**

In `crates/webui/frontend/src/types.ts`, replace the `RenderedTurn` type (the `export type RenderedTurn = ...` block) with:

```ts
export type RenderedTurn =
  | {
      type: "user";
      timestamp: string | null;
      text: string;
      uuid: string;
      edit_index: number;
      fork_index: number;
      editable: boolean;
    }
  | {
      type: "assistant";
      timestamp: string | null;
      model: string | null;
      blocks: RenderedBlock[];
      uuid: string;
      edit_index: number;
      fork_index: number;
      editable: boolean;
    }
  | { type: "system"; timestamp: string | null; content: string }
  | { type: "summary"; timestamp: string | null; content: string }
  | { type: "compaction"; messages_before: number };
```

- [ ] **Step 2: Type-check**

Run: `(cd crates/webui/frontend && npx tsc --noEmit) 2>&1 | tail -20`
Expected: FAIL — `Transcript.tsx` `Turn`'s `switch (turn.type)` is no longer exhaustive (missing `compaction`), and possibly a missing-property note. This is fixed in Task 4. (If running tasks in order, this red is expected.)

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/types.ts
git commit -m "feat(webui): RenderedTurn compaction variant + editable flag"
```

---

## Task 4: webui Transcript — divider + editable gating

**Files:**
- Modify: `crates/webui/frontend/src/components/Transcript.tsx`

- [ ] **Step 1: Gate `editIndex` on editable (prevents a pre-compaction turn sharing an active turn's edit index)**

In `crates/webui/frontend/src/components/Transcript.tsx`, find the turns map (around line 133):

```tsx
        {visibleTurns.map((turn, i) => {
          const isMessage = turn.type === "user" || turn.type === "assistant";
          const editIndex = isMessage ? turn.edit_index : null;
```

Replace the `editIndex` line with:

```tsx
          const editIndex =
            (turn.type === "user" || turn.type === "assistant") && turn.editable
              ? turn.edit_index
              : null;
```

(Pre-compaction turns carry segment-local indices that can collide with the active segment's; gating here ensures the edit box only ever opens on an active turn.)

- [ ] **Step 2: Gate `TurnActions` on editable in the `user` case**

In the `Turn` function's `case "user":` branch, replace:

```tsx
            <TurnActions onEdit={() => onStartEdit(turn.text)} onFork={onFork} onDelete={onDelete} messageId={turn.uuid} />
```

with:

```tsx
            {turn.editable && (
              <TurnActions onEdit={() => onStartEdit(turn.text)} onFork={onFork} onDelete={onDelete} messageId={turn.uuid} />
            )}
```

- [ ] **Step 3: Gate `TurnActions` on editable in the `assistant` case**

In the `case "assistant":` branch, replace:

```tsx
            <TurnActions
              onEdit={() => {
                const firstText = turn.blocks.find((b) => b.type === "text");
                onStartEdit(firstText && firstText.type === "text" ? firstText.text : "");
              }}
              onFork={onFork}
              onDelete={onDelete}
              messageId={turn.uuid}
            />
```

with:

```tsx
            {turn.editable && (
              <TurnActions
                onEdit={() => {
                  const firstText = turn.blocks.find((b) => b.type === "text");
                  onStartEdit(firstText && firstText.type === "text" ? firstText.text : "");
                }}
                onFork={onFork}
                onDelete={onDelete}
                messageId={turn.uuid}
              />
            )}
```

- [ ] **Step 4: Add the `compaction` case to the `Turn` switch**

In the `Turn` function, after the `case "summary":` branch (and before the switch's closing `}`), add:

```tsx
    case "compaction":
      return (
        <div className="flex items-center gap-3 my-5">
          <div className="flex-1 border-t border-[var(--color-border)]" />
          <span className="text-[10px] uppercase tracking-wider text-[var(--color-text-dim)] whitespace-nowrap">
            context compacted · {turn.messages_before} messages above
          </span>
          <div className="flex-1 border-t border-[var(--color-border)]" />
        </div>
      );
```

- [ ] **Step 5: Type-check + build**

Run: `(cd crates/webui/frontend && npx tsc --noEmit) && echo TSC_OK && npm --prefix crates/webui/frontend run build 2>&1 | grep -iE "error|built|✓" | head`
Expected: `TSC_OK` then a successful `✓ built` line, no errors.

- [ ] **Step 6: Run the frontend unit tests (find matcher must still pass)**

Run: `npm --prefix crates/webui/frontend run test 2>&1 | grep -E "Test Files|Tests "`
Expected: `Tests  14 passed (14)`.

- [ ] **Step 7: Commit**

```bash
git add crates/webui/frontend/src/components/Transcript.tsx
git commit -m "feat(webui): render compaction divider; pre-compaction turns are view-only"
```

---

## Task 5: Build + verification

**Files:** none (verification only).

- [ ] **Step 1: Full workspace check**

Run: `cargo test -p claude-manager-core 2>&1 | tail -5 && cargo build -p claude-manager-webui 2>&1 | tail -3`
Expected: all core tests pass; webui crate builds.

- [ ] **Step 2: Frontend build + tests**

Run: `npm --prefix crates/webui/frontend run build 2>&1 | tail -3 && npm --prefix crates/webui/frontend run test 2>&1 | grep -E "Tests "`
Expected: `✓ built`; `Tests  14 passed (14)`.

- [ ] **Step 3: Launch and verify on the real compacted session**

Run: `make webui` (from repo root). Open session `6180c55c-e7aa-416b-a7ec-8d0ec6a5f20b`.

Verify (this is the real-file check the synthetic tests can't make):
- [ ] The transcript now shows the **pre-compaction history** (≈1268 messages) above a `context compacted · N messages above` divider, then the post-compaction summary + replies.
- [ ] Pre-compaction turns are **view-only** — hovering shows **no** edit / fork / delete / copy-id buttons.
- [ ] The active (post-compaction) conversation still shows action buttons and edit/fork/delete still work (spot-check an edit opens the box on the right message only — no pre-compaction turn opens an edit box).
- [ ] A normal **non-compacted** session looks and behaves exactly as before (actions present, no divider).
- [ ] **find** (`Cmd+F`) now matches inside pre-compaction history and highlights/scrolls to it. Note find **open-time and responsiveness** on this 1271-turn session (react-markdown ×1268 + the `[data-seg-id]` walk is the stress case). If it's janky, say so — collapse-by-default is the escape hatch, not built preemptively.

- [ ] **Step 4: Commit (only if Step 3 surfaced fixes)**

```bash
git add -A
git commit -m "fix(webui): pre-compaction rendering verification fixes"
```

---

## Self-Review

**Spec coverage:**
- `tree::segments` (group-by-root, memoized, active-by-global-leaf, forks stay one tree, dangling parent own root) → Task 1.
- `render_path_turns` extraction + `editable` + `Compaction` divider + stats on active path → Task 2.
- Data-model `compaction` variant + `editable` → Task 2 (Rust) / Task 3 (TS).
- webui divider + hide all mutating controls when `!editable` (edit/fork/delete/copy-id all live in `TurnActions`, gated as a unit) → Task 4.
- editIndex collision guard (segment-local indices) → Task 4 Step 1.
- find covers pre-compaction for free (same flat `turns`, data-seg-ids) → verified Task 5 Step 3.
- Single-tree regression (byte-identical indices) → Task 2 test `single_tree_has_no_divider_sequential_editable_indices`.
- Real-file 2-segment / 1268-msg check → Task 5 Step 3 (GUI).
- Memory follow-up (update/delete `compaction-reroots-session-tree`) → handled post-merge by the controller, noted here.

**Placeholder scan:** none — every code step has complete code.

**Type consistency:** `segments(&[Event]) -> Vec<Vec<&Event>>` used by `render_session`. `RenderedTurn::Compaction { messages_before: usize }` matches TS `{ type: "compaction"; messages_before: number }`. `editable: bool` added on both `User`/`Assistant` (Rust) and TS. `render_path_turns(path, editable)` signature consistent across Task 2. webui reads `turn.editable` and `turn.messages_before` exactly as defined in Task 3.
