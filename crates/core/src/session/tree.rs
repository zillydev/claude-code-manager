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
pub fn current_path<'a>(events: &'a [Event], leaf: Uuid) -> Vec<&'a Event> {
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
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Text("x".into()),
            },
            tool_use_result: None,
            cwd: None, git_branch: None,
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
}
