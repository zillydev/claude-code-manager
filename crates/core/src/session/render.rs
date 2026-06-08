use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::event::{Block, Event, MessageContent, UserBlock};
use super::Session;

#[derive(Debug, Clone, Serialize)]
pub struct RenderedSession {
    pub project_label: String,
    pub id: String,
    pub source_path: String,
    pub title: String,
    pub event_count: usize,
    pub stats: SessionStats,
    pub turns: Vec<RenderedTurn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub duration_secs: Option<i64>,
    pub models: Vec<ModelUsage>,
    /// Cumulative token usage across every turn (lifetime processed).
    pub tokens: TokenBreakdown,
    /// Current context-window size: the last real assistant turn's full request
    /// input (input + cache_creation + cache_read). Matches Claude Code's TUI.
    pub context_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelUsage {
    pub model: String,
    pub turn_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenBreakdown {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum RenderedBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        input_summary: String,
        input_full: serde_json::Value,
        result: Option<ToolCallResult>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallResult {
    pub text: String,
    pub is_error: bool,
}

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

pub fn compute_stats(path: &[&Event]) -> SessionStats {
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut first_ts: Option<DateTime<Utc>> = None;
    let mut last_ts: Option<DateTime<Utc>> = None;
    let mut model_counts: HashMap<String, u32> = HashMap::new();
    let mut context_tokens: u64 = 0;
    let mut tokens = TokenBreakdown {
        input: 0,
        output: 0,
        cache_creation: 0,
        cache_read: 0,
    };

    for ev in path {
        if let Some(ts) = ev.timestamp() {
            if first_ts.is_none() || ts < first_ts.unwrap() {
                first_ts = Some(ts);
            }
            if last_ts.is_none() || ts > last_ts.unwrap() {
                last_ts = Some(ts);
            }
        }

        if cwd.is_none() {
            if let Some(c) = ev.cwd() {
                cwd = Some(c.to_string());
            }
        }
        if git_branch.is_none() {
            if let Some(b) = ev.git_branch() {
                if !b.is_empty() && b != "HEAD" {
                    git_branch = Some(b.to_string());
                }
            }
        }

        if let Event::Assistant(a) = ev {
            if let Some(model) = &a.message.model {
                *model_counts.entry(model.clone()).or_insert(0) += 1;
            }
            if let Some(usage) = &a.message.usage {
                tokens.input = tokens.input.saturating_add(usage.input_tokens);
                tokens.output = tokens.output.saturating_add(usage.output_tokens);
                tokens.cache_creation = tokens
                    .cache_creation
                    .saturating_add(usage.cache_creation_input_tokens);
                tokens.cache_read = tokens
                    .cache_read
                    .saturating_add(usage.cache_read_input_tokens);
                // Current context-window size = the last real turn's full input.
                if a.message.model.as_deref() != Some("<synthetic>") {
                    context_tokens = usage
                        .input_tokens
                        .saturating_add(usage.cache_creation_input_tokens)
                        .saturating_add(usage.cache_read_input_tokens);
                }
            }
        }
    }

    let duration_secs = match (first_ts, last_ts) {
        (Some(f), Some(l)) => Some((l - f).num_seconds()),
        _ => None,
    };

    let mut models: Vec<ModelUsage> = model_counts
        .into_iter()
        .map(|(model, turn_count)| ModelUsage { model, turn_count })
        .collect();
    models.sort_by(|a, b| b.turn_count.cmp(&a.turn_count));

    SessionStats {
        cwd,
        git_branch,
        duration_secs,
        models,
        tokens,
        context_tokens,
    }
}

fn session_title(session: &Session) -> String {
    let mut custom = None;
    let mut last_prompt = None;
    let mut last_user = None;

    for ev in &session.events {
        match ev {
            // Last wins: the most recent /rename is the current title.
            Event::CustomTitle(t) => custom = Some(t.custom_title.clone()),
            Event::LastPrompt(lp) => {
                if let Some(p) = &lp.last_prompt {
                    let trimmed = p.trim();
                    if !trimmed.is_empty()
                        && !trimmed.starts_with("<command-")
                        && !trimmed.starts_with("<local-command-")
                    {
                        last_prompt = Some(trimmed.to_string());
                    }
                }
            }
            Event::User(u) => {
                if u.is_meta.unwrap_or(false) || u.is_sidechain {
                    continue;
                }
                if let MessageContent::Text(t) = &u.message.content {
                    let trimmed = t.trim();
                    if !trimmed.is_empty()
                        && !trimmed.starts_with("<command-")
                        && !trimmed.starts_with("<local-command-")
                    {
                        last_user = Some(trimmed.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    custom
        .or(last_prompt)
        .or(last_user)
        .unwrap_or_else(|| "(untitled)".to_string())
}

struct ToolResultEntry {
    text: String,
    is_error: bool,
}

fn collect_tool_results(path: &[&Event]) -> HashMap<String, ToolResultEntry> {
    let mut out = HashMap::new();
    for ev in path {
        if let Event::User(u) = ev {
            if let MessageContent::Blocks(blocks) = &u.message.content {
                for b in blocks {
                    if let UserBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = b
                    {
                        out.insert(
                            tool_use_id.clone(),
                            ToolResultEntry {
                                text: content.as_text(),
                                is_error: *is_error,
                            },
                        );
                    }
                }
            }
        }
    }
    out
}

fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    let keys: &[&str] = match name {
        "Bash" => &["command", "description"],
        "Read" => &["file_path"],
        "Write" => &["file_path"],
        "Edit" => &["file_path"],
        _ => &["query", "file_path", "path", "command", "description", "prompt"],
    };
    for k in keys {
        if let Some(v) = input.get(*k) {
            if let Some(s) = v.as_str() {
                return truncate(s, 120);
            }
        }
    }
    truncate(&input.to_string(), 120)
}

fn truncate(s: &str, max: usize) -> String {
    let one_line: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    if one_line.chars().count() <= max {
        one_line
    } else {
        let head: String = one_line.chars().take(max - 1).collect();
        format!("{}…", head)
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::session::event::{CustomTitleEvent, Message, MessageContent, UserEvent};
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

    #[test]
    fn title_uses_last_custom_title() {
        // The current title is the most recent custom-title event, not the first.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let events = vec![
            Event::CustomTitle(CustomTitleEvent {
                custom_title: "<local-command-caveat>old garbage".into(),
            }),
            uev(a, None, "hi"),
            Event::CustomTitle(CustomTitleEvent { custom_title: "stripe-branch".into() }),
        ];
        let r = render_session(&sess(events));
        assert_eq!(r.title, "stripe-branch");
    }
}
