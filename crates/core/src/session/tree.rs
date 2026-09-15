use std::collections::HashMap;

use uuid::Uuid;

use super::event::Event;

/// Resolve the leaf UUID of the "current" conversation path.
///
/// The transcript jsonl is append-only: the active branch's newest node is
/// always the last event written, even after a rewind (Claude Code appends the
/// new branch's messages at the end). So the true tip is simply the UUID of the
/// last event that has one.
///
/// We deliberately do NOT use `LastPrompt.leaf_uuid` here: that field bookmarks
/// where a prompt was inserted (an ancestor), so any assistant turns appended
/// after it are descendants of the bookmark. Since `current_path` walks only
/// ancestors, preferring the bookmark would drop the most recent reply.
pub fn resolve_leaf(events: &[Event]) -> Option<Uuid> {
    let mut last_uuid = None;
    for ev in events {
        if let Some(u) = ev.uuid() {
            last_uuid = Some(u);
        }
    }
    last_uuid
}

/// Walk the parent chain back from `leaf` and return the events on that path
/// in chronological order (oldest first). Events with no UUID (titles,
/// last-prompt, etc.) are filtered out — only nodes with identity participate
/// in the tree.
pub fn current_path(events: &[Event], leaf: Uuid) -> Vec<&Event> {
    let mut by_uuid: HashMap<Uuid, &Event> = HashMap::with_capacity(events.len());
    for ev in events {
        if let Some(u) = ev.uuid() {
            by_uuid.insert(u, ev);
        }
    }

    let mut path = Vec::new();
    let mut cursor = Some(leaf);
    let mut guard = 0u32;
    while let Some(u) = cursor {
        guard += 1;
        if guard > 100_000 {
            break; // safety: shouldn't loop
        }
        let Some(ev) = by_uuid.get(&u) else { break };
        path.push(*ev);
        cursor = ev.parent_uuid();
    }
    path.reverse();
    path
}

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
    segments_with_leaf(events, None)
}

/// [`segments`], but rendering `leaf`'s branch in place of the live one for the
/// tree that contains it. Every other tree, and the order they come in, is
/// unchanged. `None` — or a leaf the file doesn't have — behaves exactly like
/// [`segments`].
pub fn segments_with_leaf(events: &[Event], leaf: Option<Uuid>) -> Vec<Vec<&Event>> {
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

    // The tree whose branch the caller asked to see instead of its live one.
    let shown_root = leaf
        .filter(|l| by_uuid.contains_key(l))
        .map(|l| root_of(l, &by_uuid, &mut cache));

    let mut out: Vec<Vec<&Event>> = Vec::with_capacity(ordered.len());
    for r in ordered {
        // The tree's leaf is its last-in-file-order member, unless the caller
        // named one inside it.
        let tree_leaf = match shown_root {
            Some(sr) if sr == r => leaf,
            _ => groups[&r].last().and_then(|e| e.uuid()),
        };
        if let Some(tree_leaf) = tree_leaf {
            out.push(current_path(events, tree_leaf));
        }
    }
    out
}

/// Leaf of the branch containing `uuid`: the last-in-file-order event whose
/// ancestor chain passes through it (the message itself when nothing descends
/// from it). `None` when the file holds no such message.
///
/// A rewind leaves the abandoned branch in the file while the transcript
/// renders only the path to [`resolve_leaf`]. Rendering to THIS leaf instead is
/// what puts a rewound message on screen for a find result to scroll to.
pub fn branch_leaf(events: &[Event], uuid: Uuid) -> Option<Uuid> {
    let mut by_uuid: HashMap<Uuid, &Event> = HashMap::with_capacity(events.len());
    for ev in events {
        if let Some(u) = ev.uuid() {
            by_uuid.insert(u, ev);
        }
    }
    by_uuid.get(&uuid)?;

    // Memoized "is this node under `uuid`?", so a long file costs one parent
    // walk per node rather than one per (node, ancestor) pair.
    let mut cache: HashMap<Uuid, bool> = HashMap::new();
    cache.insert(uuid, true);
    let mut leaf = uuid;
    for ev in events {
        let Some(u) = ev.uuid() else { continue };
        if descends(u, &by_uuid, &mut cache) {
            leaf = u; // file order is chronological, so the last one wins
        }
    }
    Some(leaf)
}

/// Walk up from `start` until the answer is known, then record it for the whole
/// chain. Cycle-guarded, like [`segments`]'s root resolution.
fn descends(
    start: Uuid,
    by_uuid: &HashMap<Uuid, &Event>,
    cache: &mut HashMap<Uuid, bool>,
) -> bool {
    let mut chain: Vec<Uuid> = Vec::new();
    let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    let mut cur = start;
    let verdict = loop {
        if let Some(v) = cache.get(&cur) {
            break *v;
        }
        if !seen.insert(cur) {
            break false; // cycle
        }
        chain.push(cur);
        match by_uuid.get(&cur).and_then(|e| e.parent_uuid()) {
            Some(p) => cur = p,
            None => break false,
        }
    };
    for c in chain {
        cache.insert(c, verdict);
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::event::{Message, MessageContent, UserEvent};
    use chrono::Utc;

    fn user(uuid: &str, parent: Option<&str>) -> Event {
        Event::User(UserEvent {
            uuid: Some(uuid.parse().unwrap()),
            parent_uuid: parent.map(|p| p.parse().unwrap()),
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            is_meta: None,
            is_compact_summary: None,
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Text("x".into()),
            },
            tool_use_result: None,
            cwd: None, git_branch: None, forked_from: None,
            interrupted_message_id: None,
        })
    }

    #[test]
    fn linear_chain_resolves() {
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        let c = "00000000-0000-0000-0000-000000000003";
        let events = vec![user(a, None), user(b, Some(a)), user(c, Some(b))];

        let leaf = resolve_leaf(&events).unwrap();
        assert_eq!(leaf.to_string(), c);

        let path = current_path(&events, leaf);
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].uuid().unwrap().to_string(), a);
        assert_eq!(path[2].uuid().unwrap().to_string(), c);
    }

    fn last_prompt(leaf: &str) -> Event {
        Event::LastPrompt(crate::session::event::LastPromptEvent {
            leaf_uuid: leaf.parse().unwrap(),
            last_prompt: None,
        })
    }

    #[test]
    fn messages_after_last_prompt_bookmark_are_included() {
        // Reproduces the real-world layout: a `last-prompt` event bookmarks an
        // earlier node, then the assistant reply is appended as its descendant.
        // The leaf must be the appended reply, not the bookmark.
        let a = "00000000-0000-0000-0000-000000000001"; // user prompt
        let b = "00000000-0000-0000-0000-000000000002"; // assistant reply
        let events = vec![
            user(a, None),
            last_prompt(a), // bookmark points at the prompt (an ancestor of b)
            user(b, Some(a)),
        ];

        let leaf = resolve_leaf(&events).unwrap();
        assert_eq!(leaf.to_string(), b, "leaf should be the appended reply");

        let path = current_path(&events, leaf);
        assert_eq!(path.len(), 2);
        assert_eq!(path[1].uuid().unwrap().to_string(), b);
    }

    #[test]
    fn forked_chain_only_returns_active_leaf_path() {
        // a -> b -> c (abandoned)
        //  \-> d (active)
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        let c = "00000000-0000-0000-0000-000000000003";
        let d = "00000000-0000-0000-0000-000000000004";
        let events = vec![
            user(a, None),
            user(b, Some(a)),
            user(c, Some(b)),
            user(d, Some(a)),
        ];

        let path = current_path(&events, d.parse().unwrap());
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].uuid().unwrap().to_string(), a);
        assert_eq!(path[1].uuid().unwrap().to_string(), d);
    }

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

    #[test]
    fn branch_leaf_finds_the_tip_of_a_rewound_branch() {
        //   a ─┬─ b ── b2      rewound away: in the file, not in the transcript
        //      └─ c            the branch the transcript renders
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        let b2 = "00000000-0000-0000-0000-000000000003";
        let c = "00000000-0000-0000-0000-000000000004";
        let events =
            vec![user(a, None), user(b, Some(a)), user(b2, Some(b)), user(c, Some(a))];

        assert_eq!(resolve_leaf(&events).unwrap().to_string(), c, "the live branch");

        let leaf = branch_leaf(&events, b.parse().unwrap()).expect("b is in the file");
        assert_eq!(leaf.to_string(), b2, "the rewound branch's own tip");

        let path: Vec<String> =
            current_path(&events, leaf).iter().map(|e| e.uuid().unwrap().to_string()).collect();
        assert_eq!(path, vec![a.to_string(), b.to_string(), b2.to_string()]);
    }

    #[test]
    fn branch_leaf_of_a_message_with_no_descendants_is_itself() {
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        let events = vec![user(a, None), user(b, Some(a))];
        assert_eq!(branch_leaf(&events, b.parse().unwrap()).unwrap().to_string(), b);
    }

    #[test]
    fn branch_leaf_of_an_unknown_message_is_none() {
        let a = "00000000-0000-0000-0000-000000000001";
        let absent = "00000000-0000-0000-0000-0000000000ff";
        let events = vec![user(a, None)];
        assert!(branch_leaf(&events, absent.parse().unwrap()).is_none());
    }

    #[test]
    fn segments_with_leaf_swaps_only_the_branch_that_leaf_belongs_to() {
        // One tree, rewound; overriding the leaf renders the abandoned path
        // instead of the live one, and nothing else moves.
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        let c = "00000000-0000-0000-0000-000000000003";
        let events = vec![user(a, None), user(b, Some(a)), user(c, Some(a))];

        let live = segments_with_leaf(&events, None);
        assert_eq!(live.len(), 1);
        let ids: Vec<String> =
            live[0].iter().map(|e| e.uuid().unwrap().to_string()).collect();
        assert_eq!(ids, vec![a.to_string(), c.to_string()], "default is unchanged");

        let shown = segments_with_leaf(&events, Some(b.parse().unwrap()));
        let ids: Vec<String> =
            shown[0].iter().map(|e| e.uuid().unwrap().to_string()).collect();
        assert_eq!(ids, vec![a.to_string(), b.to_string()], "the rewound path renders");
    }
}
