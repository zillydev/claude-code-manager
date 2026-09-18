use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::event::{Block, Event, MessageContent, UserBlock};
use super::file_changes::FileChanges;
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

#[derive(Debug, Clone, PartialEq, Serialize)]
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
    /// The span `duration_secs` measures, kept so appended lines can widen it.
    #[serde(skip)]
    span: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelUsage {
    pub model: String,
    pub turn_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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
        /// Attached/pasted images, in content order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<RenderedImage>,
        /// The message line's event uuid (empty if absent).
        uuid: String,
        /// 0-based index among user/assistant message lines on the active path
        /// (display/diagnostic only — mutations resolve by `edit_uuid`/`fork_uuid`).
        edit_index: usize,
        fork_index: usize,
        /// Stable uuid of the edit target / fork cutoff event. Mutations resolve
        /// these against the current file, so they survive the live-tail (which
        /// renders streamed turns with batch-local, unusable indices).
        edit_uuid: String,
        fork_uuid: String,
        /// Every event uuid this turn covers (one for a user turn). Lets the UI
        /// locate the turn by ANY message uuid it contains — needed because a
        /// fork/copy branch point can be a merged-away assistant event.
        uuids: Vec<String>,
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
        /// first text block is shown/edited). Display/diagnostic only.
        edit_index: usize,
        /// Message index of the LAST merged assistant event. Display/diagnostic only.
        fork_index: usize,
        /// Stable uuid of the text-bearing event that an edit rewrites.
        edit_uuid: String,
        /// Stable uuid of the LAST merged event — the fork cutoff (so a fork
        /// includes the whole reply plus its interleaved tool-result lines).
        fork_uuid: String,
        /// Every merged assistant event's uuid (head + all merged-in events).
        /// Lets the UI locate this turn by any of them, not just head/fork.
        uuids: Vec<String>,
        editable: bool,
    },
    #[serde(rename = "system")]
    System {
        timestamp: Option<DateTime<Utc>>,
        content: String,
    },
    /// A slash-command invocation (`/deploy-gateway args`) — display-only. The
    /// raw `<command-…>` echo is machinery; we surface just the command the user
    /// ran (the injected skill body and `<local-command-…>` noise stay hidden).
    #[serde(rename = "command")]
    Command {
        timestamp: Option<DateTime<Utc>>,
        command: String,
        /// The event uuid behind it. Display-only turns still need one: search
        /// hits point at message uuids, and the UI locates a turn by them.
        #[serde(default)]
        uuids: Vec<String>,
    },
    /// A `!` bash-mode block: the command the user ran and, when the next
    /// message carried it, its output. Display-only, like `Command`.
    #[serde(rename = "bash")]
    Bash {
        timestamp: Option<DateTime<Utc>>,
        command: Option<String>,
        stdout: String,
        stderr: String,
        /// Both event uuids when the output was merged in, so a hit on either
        /// half finds this block.
        #[serde(default)]
        uuids: Vec<String>,
    },
    #[serde(rename = "summary")]
    Summary {
        timestamp: Option<DateTime<Utc>>,
        content: String,
    },
    #[serde(rename = "task_notification")]
    TaskNotification {
        timestamp: Option<DateTime<Utc>>,
        uuid: String,
        status: String,
        summary: String,
        result: String,
    },
    /// Claude Code's record of an API failure ("Prompt is too long", "API
    /// Error: 529 Overloaded"). Written as a synthetic assistant line, which
    /// the transcript otherwise skips — but this one explains why a session
    /// stopped, so it renders as its own turn.
    #[serde(rename = "api_error")]
    ApiError {
        timestamp: Option<DateTime<Utc>>,
        text: String,
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
    /// Claude Code's "summarized" line: a thinking block carrying a short
    /// summary of the reasoning, shown in the TUI while real thinking is not.
    #[serde(rename = "narration")]
    Narration { text: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        input_summary: String,
        input_full: serde_json::Value,
        result: Option<ToolCallResult>,
    },
}

/// Claude Code persists real thinking as an empty string and its "summarized"
/// line as a text-bearing thinking block; since 2.1.263 the signature's
/// protobuf header also names the kind. Text is the primary signal; a header
/// that explicitly says `thinking` overrides it.
fn is_narration(thinking: &str, signature: Option<&str>) -> bool {
    use base64::Engine as _;
    if thinking.trim().is_empty() {
        return false;
    }
    let Some(sig) = signature else { return true };
    // The kind sits in the first few dozen bytes; decode a 4-aligned prefix.
    let head = &sig[..sig.len().min(64) & !3];
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(head) else { return true };
    !bytes.windows(8).any(|w| w == b"thinking")
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallResult {
    pub text: String,
    pub is_error: bool,
    /// Images the tool returned (e.g. browser screenshots), base64.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<RenderedImage>,
    /// Files a shell command changed; boxed so `RenderedBlock::ToolCall` stays small.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_changes: Option<Box<FileChanges>>,
}

/// A base64 image carried by a user message or tool result, ready for a
/// `data:<media_type>;base64,<data>` URI. Non-base64 sources are skipped at
/// extraction (nothing renderable without fetching).
#[derive(Debug, Clone, Serialize)]
pub struct RenderedImage {
    pub media_type: String,
    pub data: String,
}

impl RenderedImage {
    fn from_source(src: &super::event::ImageSource) -> Option<Self> {
        if src.kind != "base64" || src.data.is_empty() {
            return None;
        }
        let media_type = if src.media_type.is_empty() {
            "image/png".to_string()
        } else {
            src.media_type.clone()
        };
        Some(RenderedImage { media_type, data: src.data.clone() })
    }
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
                Block::Thinking { thinking, signature } => {
                    if is_narration(thinking, signature.as_deref()) {
                        blocks.push(RenderedBlock::Narration { text: thinking.trim().to_string() });
                    } else {
                        blocks.push(RenderedBlock::Thinking { thinking: thinking.clone() });
                    }
                }
                Block::ToolUse { id, name, input } => {
                    let result = tool_results.get(id.as_str()).map(|r| ToolCallResult {
                        text: r.text.clone(),
                        is_error: r.is_error,
                        images: r.images.clone(),
                        file_changes: r.file_changes.clone().map(Box::new),
                    });
                    blocks.push(RenderedBlock::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input_summary: crate::session::display::tool_input_summary(name, input),
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
                if ev.hidden_from_transcript() {
                    i += 1;
                    continue;
                }
                let text = u.message.content.text();
                let images: Vec<RenderedImage> = match &u.message.content {
                    MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            UserBlock::Image { source } => RenderedImage::from_source(source),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                if text.starts_with("<command-") {
                    // A real slash-command invocation, shown as the compact turn
                    // the terminal shows. The machinery around it — the injected
                    // body, `<local-command-…>` stdout, a nameless echo — is
                    // already gone via `hidden_from_transcript`.
                    if let Some(command) = crate::session::display::command_label(&text) {
                        turns.push(RenderedTurn::Command {
                            timestamp: ev.timestamp(),
                            command,
                            uuids: ev.uuid().map(|u| u.to_string()).into_iter().collect(),
                        });
                    }
                    i += 1;
                    continue;
                }
                // `!` bash-mode: the command and its output arrive as two
                // consecutive user messages. Merge them into one block —
                // separately they read as two unrelated turns, and the output
                // one is not something the user "said".
                if let Some(block) = crate::session::display::bash_block(&text) {
                    let (command, mut stdout, mut stderr) = match block {
                        crate::session::display::BashBlock::Input(c) => {
                            (Some(c), String::new(), String::new())
                        }
                        crate::session::display::BashBlock::Output { stdout, stderr } => {
                            (None, stdout, stderr)
                        }
                    };
                    let mut consumed = 1;
                    let mut uuids: Vec<String> =
                        ev.uuid().map(|u| u.to_string()).into_iter().collect();
                    if command.is_some() {
                        if let Some(Event::User(next)) = path.get(i + 1).copied() {
                            let next_text = next.message.content.text();
                            if let Some(crate::session::display::BashBlock::Output { stdout: o, stderr: e }) =
                                crate::session::display::bash_block(&next_text)
                            {
                                stdout = o;
                                stderr = e;
                                consumed = 2;
                                uuids.extend(next.uuid.map(|u| u.to_string()));
                            }
                        }
                    }
                    turns.push(RenderedTurn::Bash {
                        timestamp: ev.timestamp(),
                        command,
                        stdout,
                        stderr,
                        uuids,
                    });
                    i += consumed;
                    continue;
                }
                if let Some(tn) = crate::session::display::parse_task_notification(&text) {
                    let uuid = ev.uuid().map(|u| u.to_string()).unwrap_or_default();
                    turns.push(RenderedTurn::TaskNotification {
                        timestamp: ev.timestamp(),
                        uuid,
                        status: tn.status,
                        summary: tn.summary,
                        result: tn.result,
                    });
                    i += 1;
                    continue;
                }
                let idx = msg_index[i].expect("user event has a message index");
                let uuid = ev.uuid().map(|u| u.to_string()).unwrap_or_default();
                turns.push(RenderedTurn::User {
                    timestamp: ev.timestamp(),
                    text,
                    images,
                    uuid: uuid.clone(),
                    edit_index: idx,
                    fork_index: idx,
                    edit_uuid: uuid.clone(),
                    uuids: vec![uuid.clone()],
                    fork_uuid: uuid,
                    editable,
                });
                i += 1;
            }
            Event::Assistant(a) => {
                if a.message.model.as_deref() == Some("<synthetic>") {
                    // Synthetic lines are client-side filler and stay hidden —
                    // except an API failure, which is the reason the session
                    // stopped and is otherwise invisible in the transcript.
                    if a.is_api_error {
                        turns.push(RenderedTurn::ApiError {
                            timestamp: ev.timestamp(),
                            text: assistant_text(a),
                        });
                    }
                    i += 1;
                    continue;
                }
                let timestamp = ev.timestamp();
                let head_uuid = ev.uuid().map(|u| u.to_string()).unwrap_or_default();
                let mut model = a.message.model.clone();
                let mut blocks = render_assistant_blocks(a);
                let first_msg_i = msg_index[i].expect("assistant event has a message index");
                let mut last_msg_i = first_msg_i;
                // Track the cutoff/edit-target uuids alongside the indices.
                let mut fork_uuid = head_uuid.clone();
                // Every merged event's uuid, so the UI can find this turn by any.
                let mut uuids = vec![head_uuid.clone()];
                let block_has_text =
                    |bs: &[RenderedBlock]| bs.iter().any(|b| matches!(b, RenderedBlock::Text { .. }));
                let has_text_first = block_has_text(&blocks);
                let mut edit_msg_i = if has_text_first { Some(first_msg_i) } else { None };
                let mut edit_uuid: Option<String> = if has_text_first { Some(head_uuid.clone()) } else { None };
                i += 1;
                while i < path.len() {
                    if suppressed.contains(&i) {
                        // This turn RENDERS the tool_result inline with the call
                        // it answers, so the result is part of the turn: carry the
                        // fork cutoff over it. Cutting at the last *assistant*
                        // event instead ends the fork on an unanswered tool_use,
                        // which the API rejects when the copy is resumed.
                        if let Some(u) = path[i].uuid() {
                            let u = u.to_string();
                            uuids.push(u.clone());
                            fork_uuid = u;
                            if let Some(m) = msg_index[i] {
                                last_msg_i = m;
                            }
                        }
                        i += 1;
                        continue;
                    }
                    if let Event::Assistant(next_a) = path[i] {
                        if next_a.message.model.as_deref() == Some("<synthetic>") {
                            // An API failure ends the reply: break so it lands
                            // AFTER this turn as its own line (the outer loop
                            // renders it), instead of being merged away.
                            if next_a.is_api_error {
                                break;
                            }
                            i += 1;
                            continue;
                        }
                        let next_blocks = render_assistant_blocks(next_a);
                        let this_msg_i = msg_index[i].expect("assistant event has a message index");
                        let this_uuid = path[i].uuid().map(|u| u.to_string()).unwrap_or_default();
                        if edit_msg_i.is_none() && block_has_text(&next_blocks) {
                            edit_msg_i = Some(this_msg_i);
                            edit_uuid = Some(this_uuid.clone());
                        }
                        blocks.extend(next_blocks);
                        if model.is_none() {
                            model = next_a.message.model.clone();
                        }
                        last_msg_i = this_msg_i;
                        uuids.push(this_uuid.clone());
                        fork_uuid = this_uuid;
                        i += 1;
                    } else {
                        break;
                    }
                }
                turns.push(RenderedTurn::Assistant {
                    timestamp,
                    model,
                    blocks,
                    uuid: head_uuid.clone(),
                    edit_index: edit_msg_i.unwrap_or(first_msg_i),
                    fork_index: last_msg_i,
                    edit_uuid: edit_uuid.unwrap_or(head_uuid),
                    fork_uuid,
                    uuids,
                    editable,
                });
            }
            Event::Attachment(at) => {
                // A message typed while the agent was running: Claude Code
                // delivers it as an attachment line `{attachment: {type:
                // "queued_command", prompt, origin}}` (paired queue-operation
                // enqueue/remove lines are bookkeeping) — there is NO ordinary
                // user line for it, so skipping attachments silently dropped
                // the user's own words from the transcript. Render it as a
                // user turn; not editable, since edit/delete rewrite message
                // lines and this isn't one (forking by uuid still works).
                let att = &at.attachment;
                if att.get("type").and_then(|v| v.as_str()) == Some("queued_command") {
                    // The queue also carries task-notifications (870 of 1050 in
                    // the corpus) — those already render from their own user
                    // lines, so only genuine typed prompts pass. Content-based,
                    // not origin-based: pre-`origin` lines exist. A block-list
                    // prompt (text + pasted image) contributes its text blocks.
                    let text = match att.get("prompt") {
                        Some(serde_json::Value::String(t))
                            if !t.trim_start().starts_with("<task-notification>") =>
                        {
                            Some(t.clone())
                        }
                        Some(serde_json::Value::Array(blocks)) => {
                            let joined: Vec<&str> = blocks
                                .iter()
                                .filter(|b| {
                                    b.get("type").and_then(|v| v.as_str()) == Some("text")
                                })
                                .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                                .collect();
                            if joined.is_empty() { None } else { Some(joined.join("\n")) }
                        }
                        _ => None,
                    };
                    if let Some(prompt) = text {
                        let prompt = prompt.as_str();
                        let uuid = at.uuid.map(|u| u.to_string()).unwrap_or_default();
                        turns.push(RenderedTurn::User {
                            timestamp: at.timestamp,
                            text: prompt.to_string(),
                            images: Vec::new(),
                            uuid: uuid.clone(),
                            edit_index: 0,
                            fork_index: 0,
                            edit_uuid: uuid.clone(),
                            fork_uuid: uuid.clone(),
                            uuids: vec![uuid],
                            editable: false,
                        });
                    }
                }
                i += 1;
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
/// `session.leaf` selects which branch of its tree renders — the file's own tip
/// unless a caller asked for a rewound one.
pub fn render_session(session: &Session) -> RenderedSession {
    // A leaf other than the file's own tip means the caller asked to see a
    // branch this session rewound away. That path renders read-only: an edit
    // resolves by uuid and would rewrite history the session already left.
    let viewing_branch =
        session.leaf.is_some() && session.leaf != super::tree::resolve_leaf(&session.events);
    let segs = super::tree::segments_with_leaf(&session.events, session.leaf);
    let active_idx = segs.len().saturating_sub(1);

    let mut turns: Vec<RenderedTurn> = Vec::new();
    let mut prev_seg_msgs = 0usize;
    for (s, seg) in segs.iter().enumerate() {
        if s > 0 {
            turns.push(RenderedTurn::Compaction { messages_before: prev_seg_msgs });
        }
        let seg_turns = render_path_turns(seg, s == active_idx && !viewing_branch);
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
    let mut stats = SessionStats::empty();
    stats.fold(path);
    stats
}

/// `stats` once `appended` — lines just written to the end of the file — has landed. Appends extend the
/// active path, except a null-parent message, which is a new tree (an auto-compaction) the stats restart at.
pub fn extend_stats(stats: &mut SessionStats, appended: &[Event]) {
    let restart = appended.iter().rposition(|e| e.uuid().is_some() && e.parent_uuid().is_none());
    if restart.is_some() {
        *stats = SessionStats::empty();
    }
    let path: Vec<&Event> = appended[restart.unwrap_or(0)..]
        .iter()
        .filter(|e| e.uuid().is_some() && !e.is_sidechain())
        .collect();
    stats.fold(&path);
}

impl SessionStats {
    fn empty() -> Self {
        SessionStats {
            cwd: None,
            git_branch: None,
            duration_secs: None,
            models: Vec::new(),
            tokens: TokenBreakdown { input: 0, output: 0, cache_creation: 0, cache_read: 0 },
            context_tokens: 0,
            span: None,
        }
    }

    /// Adds `path` — events on the active branch, oldest first — to the totals.
    fn fold(&mut self, path: &[&Event]) {
        let mut model_counts: HashMap<String, u32> =
            self.models.drain(..).map(|m| (m.model, m.turn_count)).collect();
        for ev in path {
            if let Some(ts) = ev.timestamp() {
                self.span = Some(match self.span {
                    None => (ts, ts),
                    Some((first, last)) => (first.min(ts), last.max(ts)),
                });
            }

            if self.cwd.is_none() {
                if let Some(c) = ev.cwd() {
                    self.cwd = Some(c.to_string());
                }
            }
            if self.git_branch.is_none() {
                if let Some(b) = ev.git_branch() {
                    if !b.is_empty() && b != "HEAD" {
                        self.git_branch = Some(b.to_string());
                    }
                }
            }

            if let Event::Assistant(a) = ev {
                if let Some(model) = &a.message.model {
                    *model_counts.entry(model.clone()).or_insert(0) += 1;
                }
                if let Some(usage) = &a.message.usage {
                    let t = &mut self.tokens;
                    t.input = t.input.saturating_add(usage.input_tokens);
                    t.output = t.output.saturating_add(usage.output_tokens);
                    t.cache_creation = t.cache_creation.saturating_add(usage.cache_creation_input_tokens);
                    t.cache_read = t.cache_read.saturating_add(usage.cache_read_input_tokens);
                    // Current context-window size = the last real turn's full input.
                    if a.message.model.as_deref() != Some("<synthetic>") {
                        self.context_tokens = usage
                            .input_tokens
                            .saturating_add(usage.cache_creation_input_tokens)
                            .saturating_add(usage.cache_read_input_tokens);
                    }
                }
            }
        }

        self.duration_secs = self.span.map(|(first, last)| (last - first).num_seconds());
        self.models = model_counts
            .into_iter()
            .map(|(model, turn_count)| ModelUsage { model, turn_count })
            .collect();
        self.models.sort_by_key(|m| std::cmp::Reverse(m.turn_count));
    }
}

fn session_title(session: &Session) -> String {
    super::summary::session_title(&session.events).unwrap_or_else(|| "(untitled)".to_string())
}

struct ToolResultEntry {
    text: String,
    is_error: bool,
    images: Vec<RenderedImage>,
    file_changes: Option<FileChanges>,
}

/// A tool_result observed in a batch of events. Used by the live tail to
/// back-fill a tool_call rendered in an EARLIER batch: a slow tool's result
/// lands minutes after its call, in a separate tail read, so the mini-render
/// of that batch alone can never pair it (the cause of a permanent
/// "(no result)" on long-running calls in a watched session).
#[derive(Debug, Clone, Serialize)]
pub struct ToolResultUpdate {
    pub tool_use_id: String,
    pub text: String,
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<RenderedImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_changes: Option<FileChanges>,
}

/// Extract every tool_result in `events` (a tail batch), keyed for back-fill.
pub fn tool_result_updates(events: &[Event]) -> Vec<ToolResultUpdate> {
    let refs: Vec<&Event> = events.iter().collect();
    collect_tool_results(&refs)
        .into_iter()
        .map(|(id, e)| ToolResultUpdate {
            tool_use_id: id,
            text: e.text,
            is_error: e.is_error,
            images: e.images,
            file_changes: e.file_changes,
        })
        .collect()
}

fn collect_tool_results(path: &[&Event]) -> HashMap<String, ToolResultEntry> {
    let mut out = HashMap::new();
    for ev in path {
        if let Event::User(u) = ev {
            if let MessageContent::Blocks(blocks) = &u.message.content {
                // `toolUseResult` belongs to the line, so it's only attributable when the line holds one result.
                let results_on_line = blocks
                    .iter()
                    .filter(|b| matches!(b, UserBlock::ToolResult { .. }))
                    .count();
                for b in blocks {
                    if let UserBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = b
                    {
                        let images = match content {
                            super::event::ToolResultContent::Blocks(bs) => bs
                                .iter()
                                .filter_map(|b| match b {
                                    super::event::ToolResultBlock::Image { source } => {
                                        RenderedImage::from_source(source)
                                    }
                                    _ => None,
                                })
                                .collect(),
                            _ => Vec::new(),
                        };
                        out.insert(
                            tool_use_id.clone(),
                            ToolResultEntry {
                                text: content.as_text(),
                                is_error: *is_error,
                                images,
                                file_changes: if results_on_line == 1 {
                                    super::file_changes::from_tool_use_result(
                                        u.tool_use_result.as_ref(),
                                    )
                                } else {
                                    None
                                },
                            },
                        );
                    }
                }
            }
        }
    }
    out
}

/// The plain text of an assistant event's blocks, joined — used for the API
/// error line, whose content is a single text block.
fn assistant_text(a: &super::event::AssistantEvent) -> String {
    a.message
        .content
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::session::event::{
        AssistantEvent, AssistantMessage, Block, CustomTitleEvent, Message, MessageContent,
        ToolResultContent, UserBlock, UserEvent,
    };
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
            is_compact_summary: None,
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Text(text.into()),
            },
            tool_use_result: None,
            cwd: None,
            git_branch: None,
            forked_from: None,
            interrupted_message_id: None,
        })
    }

    fn aev(uuid: &str, parent: Option<&str>, text: &str) -> Event {
        Event::Assistant(AssistantEvent {
            uuid: Some(uuid.parse().unwrap()),
            parent_uuid: parent.map(|p| p.parse().unwrap()),
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            message: AssistantMessage {
                model: Some("claude".into()),
                stop_reason: None,
                content: vec![Block::Text { text: text.into() }],
                usage: None,
            },
            cwd: None,
            git_branch: None,
            forked_from: None,
            is_api_error: false,
        })
    }

    /// An assistant event whose only block is a tool call.
    fn atool(uuid: &str, parent: Option<&str>, tool_use_id: &str) -> Event {
        Event::Assistant(AssistantEvent {
            uuid: Some(uuid.parse().unwrap()),
            parent_uuid: parent.map(|p| p.parse().unwrap()),
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            message: AssistantMessage {
                model: Some("claude".into()),
                stop_reason: None,
                content: vec![Block::ToolUse {
                    id: tool_use_id.into(),
                    name: "Bash".into(),
                    input: serde_json::Value::Null,
                }],
                usage: None,
            },
            cwd: None,
            git_branch: None,
            forked_from: None,
            is_api_error: false,
        })
    }

    /// A user event that is a pure tool_result container (what claude appends
    /// when a tool finishes).
    fn trev(uuid: &str, parent: Option<&str>, tool_use_id: &str, text: &str, is_error: bool) -> Event {
        Event::User(UserEvent {
            uuid: Some(uuid.parse().unwrap()),
            parent_uuid: parent.map(|p| p.parse().unwrap()),
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            is_meta: None,
            is_compact_summary: None,
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Blocks(vec![UserBlock::ToolResult {
                    tool_use_id: tool_use_id.into(),
                    content: ToolResultContent::Text(text.into()),
                    is_error,
                }]),
            },
            tool_use_result: None,
            cwd: None,
            git_branch: None,
            forked_from: None,
            interrupted_message_id: None,
        })
    }

    #[test]
    fn bash_mode_command_and_output_merge_into_one_turn() {
        // `!cmd` writes the command and its output as two consecutive user
        // messages; a reader wants them as one block.
        let a = "00000000-0000-0000-0000-0000000000e1";
        let b = "00000000-0000-0000-0000-0000000000e2";
        let events = [
            uev(a, None, "<bash-input> ls -la</bash-input>"),
            uev(b, Some(a), "<bash-stdout>total 8\nfile</bash-stdout><bash-stderr></bash-stderr>"),
        ];
        let path: Vec<&Event> = events.iter().collect();
        let turns = render_path_turns(&path, true);
        assert_eq!(turns.len(), 1, "the two messages render as one block");
        match &turns[0] {
            RenderedTurn::Bash { command, stdout, stderr, uuids, .. } => {
                assert_eq!(command.as_deref(), Some("ls -la"));
                assert_eq!(stdout, "total 8\nfile");
                assert!(stderr.is_empty());
                // A find hit on either half has to resolve to this one block.
                assert_eq!(uuids, &vec![a.to_string(), b.to_string()]);
            }
            other => panic!("expected a bash turn, got {other:?}"),
        }
    }

    #[test]
    fn bash_output_without_a_command_still_renders() {
        // Output can arrive alone — e.g. the command scrolled off before a
        // compaction boundary.
        let a = "00000000-0000-0000-0000-0000000000e3";
        let events = [uev(a, None, "<bash-stdout>orphan</bash-stdout>")];
        let path: Vec<&Event> = events.iter().collect();
        match &render_path_turns(&path, true)[0] {
            RenderedTurn::Bash { command, stdout, .. } => {
                assert!(command.is_none());
                assert_eq!(stdout, "orphan");
            }
            other => panic!("expected a bash turn, got {other:?}"),
        }
    }

    #[test]
    fn a_bash_command_with_no_output_message_stands_alone() {
        let a = "00000000-0000-0000-0000-0000000000e4";
        let b = "00000000-0000-0000-0000-0000000000e5";
        let events = [
            uev(a, None, "<bash-input>echo hi</bash-input>"),
            uev(b, Some(a), "a normal message"),
        ];
        let path: Vec<&Event> = events.iter().collect();
        let turns = render_path_turns(&path, true);
        assert_eq!(turns.len(), 2, "the next message is NOT swallowed");
        assert!(matches!(turns[0], RenderedTurn::Bash { .. }));
        assert!(matches!(turns[1], RenderedTurn::User { .. }));
    }

    #[test]
    fn user_images_and_tool_result_images_are_extracted() {
        // Real shapes: an attached image is a user-content image block; a
        // browser screenshot is an image block inside a tool_result's content.
        let user_line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c1","message":{"role":"user","content":[{"type":"text","text":"see this"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}}]}}"#;
        let asst_line = r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000c2","parentUuid":"00000000-0000-0000-0000-0000000000c1","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_ss","name":"screenshot","input":{}}]}}"#;
        let result_line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":"00000000-0000-0000-0000-0000000000c2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_ss","content":[{"type":"text","text":"captured"},{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"BBBB"}}]}]}}"#;
        let events: Vec<Event> = [user_line, asst_line, result_line]
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let path: Vec<&Event> = events.iter().collect();
        let turns = render_path_turns(&path, true);
        let RenderedTurn::User { images, text, .. } = &turns[0] else { panic!("user turn") };
        assert_eq!(text, "see this");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].media_type, "image/png");
        assert_eq!(images[0].data, "AAAA");
        let RenderedTurn::Assistant { blocks, .. } = &turns[1] else { panic!("assistant turn") };
        let RenderedBlock::ToolCall { result: Some(r), .. } = &blocks[0] else { panic!("tool call") };
        assert_eq!(r.text, "captured");
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].media_type, "image/jpeg");
        // The tail back-fill carries them too.
        let ups = tool_result_updates(&events);
        assert_eq!(ups[0].images.len(), 1);
    }

    /// A Bash call and its result line, whose `toolUseResult` records one changed file.
    fn bash_with_file_changes() -> Vec<Event> {
        let asst = r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000d1","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_sed","name":"Bash","input":{"command":"sed -i '' s/a/b/ x.md"}}]}}"#;
        let result = r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000d2","parentUuid":"00000000-0000-0000-0000-0000000000d1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_sed","content":"","is_error":false}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"bashEditDiff":{"files":[{"filePath":"/repo/x.md","hunks":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-a","+b"]}]}],"moreFiles":0,"changedFiles":["/repo/x.md"]}}}"#;
        [asst, result]
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn a_tool_result_carries_the_files_its_command_changed() {
        let events = bash_with_file_changes();
        let path: Vec<&Event> = events.iter().collect();
        let turns = render_path_turns(&path, true);
        let RenderedTurn::Assistant { blocks, .. } = &turns[0] else { panic!("assistant turn") };
        let RenderedBlock::ToolCall { result: Some(r), .. } = &blocks[0] else { panic!("tool call") };
        let changes = r.file_changes.as_ref().expect("file changes");
        assert_eq!(changes.files[0].path, "/repo/x.md");
        assert_eq!(changes.files[0].hunks[0].lines, vec!["-a", "+b"]);
    }

    #[test]
    fn the_tail_back_fill_carries_file_changes() {
        let ups = tool_result_updates(&bash_with_file_changes());
        assert_eq!(ups[0].file_changes.as_ref().map(|c| c.files.len()), Some(1));
    }

    #[test]
    fn a_line_with_several_tool_results_attributes_no_file_changes() {
        // `toolUseResult` sits on the line, not on a result, so with two it can't be placed.
        let line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000d3","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"a"},{"type":"tool_result","tool_use_id":"toolu_2","content":"b"}]},"toolUseResult":{"bashEditDiff":{"files":[{"filePath":"/repo/x.md","hunks":[]}],"moreFiles":0}}}"#;
        let events: Vec<Event> = vec![serde_json::from_str(line).unwrap()];
        let ups = tool_result_updates(&events);
        assert_eq!(ups.len(), 2);
        assert!(ups.iter().all(|u| u.file_changes.is_none()));
    }

    #[test]
    fn non_base64_or_empty_image_sources_are_skipped() {
        let line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c4","message":{"role":"user","content":[{"type":"image","source":{"type":"url","url":"https://x/y.png"}},{"type":"text","text":"hi"}]}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        let events = [ev];
        let path: Vec<&Event> = events.iter().collect();
        let turns = render_path_turns(&path, true);
        let RenderedTurn::User { images, .. } = &turns[0] else { panic!("user turn") };
        assert!(images.is_empty());
    }

    #[test]
    fn tool_result_updates_extracts_results_from_a_batch() {
        // A tail batch holding ONLY the late result of a slow tool call (its
        // tool_use streamed in an earlier batch) must surface it for back-fill.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let events = vec![
            trev(a, None, "toolu_slow", "== output ==", false),
            aev(b, Some(a), "done"),
        ];
        let ups = tool_result_updates(&events);
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0].tool_use_id, "toolu_slow");
        assert_eq!(ups[0].text, "== output ==");
        assert!(!ups[0].is_error);
    }

    #[test]
    fn tool_result_updates_empty_for_plain_turns() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        assert!(tool_result_updates(&[uev(a, None, "hi")]).is_empty());
    }

    #[test]
    fn merged_assistant_carries_head_edit_and_fork_uuids() {
        // Two consecutive assistant events merge into one turn. uuid=head,
        // edit_uuid=first text-bearing event, fork_uuid=LAST merged event — so a
        // fork includes the whole reply. These uuids (not indices) drive fork/edit.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let r = render_session(&sess(vec![
            uev(a, None, "hi"),
            aev(b, Some(a), "first"),
            aev(c, Some(b), "second"),
        ]));
        let got = r
            .turns
            .iter()
            .find_map(|t| match t {
                RenderedTurn::Assistant { uuid, edit_uuid, fork_uuid, .. } => {
                    Some((uuid.clone(), edit_uuid.clone(), fork_uuid.clone()))
                }
                _ => None,
            })
            .expect("an assistant turn");
        assert_eq!(got.0, b, "head uuid");
        assert_eq!(got.1, b, "edit target = first text-bearing event");
        assert_eq!(got.2, c, "fork cutoff = last merged event");
    }

    #[test]
    fn queued_command_attachment_renders_as_a_user_turn() {
        // A message typed while the agent runs arrives ONLY as a
        // queued_command attachment (no ordinary user line) — skipping
        // attachments dropped the user's own words. Task-notification
        // payloads in the queue stay hidden (their real user lines render).
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let mk_att = |uuid: &str, parent: &str, prompt: &str| {
            Event::Attachment(crate::session::event::AttachmentEvent {
                uuid: Some(uuid.parse().unwrap()),
                parent_uuid: Some(parent.parse().unwrap()),
                timestamp: Some(Utc::now()),
                attachment: serde_json::json!({
                    "type": "queued_command",
                    "prompt": prompt,
                    "origin": { "kind": "human" }
                }),
            })
        };
        let r = render_session(&sess(vec![
            uev(a, None, "start"),
            mk_att(b, a, "queued while running"),
            mk_att(c, b, "<task-notification>\n<task-id>x</task-id>"),
        ]));
        let users: Vec<&str> = r
            .turns
            .iter()
            .filter_map(|t| match t {
                RenderedTurn::User { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(users, vec!["start", "queued while running"]);
    }

    #[test]
    fn fork_cutoff_includes_the_turns_trailing_tool_result() {
        // The turn RENDERS the tool call together with its result, so forking at
        // the turn must include the result line. Cutting at the last assistant
        // event instead leaves the fork ending on an unanswered tool_use, which
        // the API rejects on resume.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let d = "00000000-0000-0000-0000-0000000000d4";
        let r = render_session(&sess(vec![
            uev(a, None, "run it"),
            atool(b, Some(a), "toolu_1"),
            trev(c, Some(b), "toolu_1", "== output ==", false),
            uev(d, Some(c), "thanks"),
        ]));
        let (fork_uuid, uuids) = r
            .turns
            .iter()
            .find_map(|t| match t {
                RenderedTurn::Assistant { fork_uuid, uuids, .. } => {
                    Some((fork_uuid.clone(), uuids.clone()))
                }
                _ => None,
            })
            .expect("an assistant turn");
        assert_eq!(fork_uuid, c, "fork cutoff must cover the tool_result");
        assert!(uuids.contains(&c.to_string()), "uuids[] must cover it too");
        assert!(uuids.contains(&fork_uuid), "fork_uuid is always one of uuids[]");
    }

    #[test]
    fn fork_cutoff_stops_at_the_next_real_turn() {
        // Extending past trailing tool_results must not swallow the next user
        // prompt — the cutoff stays inside the turn it belongs to.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let r = render_session(&sess(vec![
            uev(a, None, "hi"),
            aev(b, Some(a), "done"),
            uev(c, Some(b), "next"),
        ]));
        let fork_uuid = r
            .turns
            .iter()
            .find_map(|t| match t {
                RenderedTurn::Assistant { fork_uuid, .. } => Some(fork_uuid.clone()),
                _ => None,
            })
            .expect("an assistant turn");
        assert_eq!(fork_uuid, b, "a turn with no tool_result ends at its last event");
    }

    #[test]
    fn user_turn_edit_and_fork_uuid_equal_its_own_uuid() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let r = render_session(&sess(vec![uev(a, None, "hi")]));
        match &r.turns[0] {
            RenderedTurn::User { uuid, edit_uuid, fork_uuid, .. } => {
                assert_eq!(uuid, a);
                assert_eq!(edit_uuid, a);
                assert_eq!(fork_uuid, a);
            }
            _ => panic!("expected user turn"),
        }
    }

    #[test]
    fn slash_command_invocation_renders_as_command_turn() {
        // A slash-command-started session: the <command-…> echo + the isMeta skill
        // injection precede the assistant's first reply. The command surfaces as a
        // compact Command turn; the injection and <local-command-…> machinery drop.
        let cmd = "00000000-0000-0000-0000-0000000000a1";
        let meta = "00000000-0000-0000-0000-0000000000b2";
        let local = "00000000-0000-0000-0000-0000000000c3";
        let asst = "00000000-0000-0000-0000-0000000000d4";
        let meta_ev = Event::User(UserEvent {
            uuid: Some(meta.parse().unwrap()),
            parent_uuid: Some(cmd.parse().unwrap()),
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            is_meta: Some(true),
            is_compact_summary: None,
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Text(
                    "Base directory for this skill: /x/y/skills/deploy-gateway\nbody".into(),
                ),
            },
            tool_use_result: None,
            cwd: None,
            git_branch: None,
            forked_from: None,
            interrupted_message_id: None,
        });
        let r = render_session(&sess(vec![
            uev(
                cmd,
                None,
                "<command-message>deploy-gateway</command-message>\n<command-name>/deploy-gateway</command-name>\n<command-args>dev</command-args>",
            ),
            meta_ev,
            uev(local, Some(meta), "<local-command-stdout>noise</local-command-stdout>"),
            aev(asst, Some(local), "I'll deploy the gateway"),
        ]));
        let cmds: Vec<&String> = r
            .turns
            .iter()
            .filter_map(|t| match t {
                RenderedTurn::Command { command, .. } => Some(command),
                _ => None,
            })
            .collect();
        assert_eq!(cmds, vec![&"/deploy-gateway dev".to_string()]);
        // It is display-only, but a global-find hit still points at the echo's
        // uuid — without one, clicking that result scrolled nowhere.
        match r.turns.iter().find(|t| matches!(t, RenderedTurn::Command { .. })).unwrap() {
            RenderedTurn::Command { uuids, .. } => assert_eq!(uuids, &vec![cmd.to_string()]),
            _ => unreachable!(),
        }
        // The skill-injection (meta) and <local-command-…> turns are not rendered.
        let users = r.turns.iter().filter(|t| matches!(t, RenderedTurn::User { .. })).count();
        assert_eq!(users, 0, "no raw user/meta/machinery turns");
        let assts = r.turns.iter().filter(|t| matches!(t, RenderedTurn::Assistant { .. })).count();
        assert_eq!(assts, 1, "assistant reply still renders");
    }

    #[test]
    fn rendering_at_a_branch_leaf_shows_the_rewound_prompt() {
        //   a ─┬─ b   the question you rewound past
        //      └─ c   what you asked instead
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let events = || {
            vec![
                uev(a, None, "one"),
                uev(b, Some(a), "rewound question"),
                uev(c, Some(a), "asked again"),
            ]
        };
        let texts = |r: &RenderedSession| -> Vec<String> {
            r.turns
                .iter()
                .filter_map(|t| match t {
                    RenderedTurn::User { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };

        let live = render_session(&sess(events()));
        assert_eq!(texts(&live), vec!["one", "asked again"], "the live branch");

        let mut s = sess(events());
        s.leaf = crate::session::tree::branch_leaf(&s.events, b.parse().unwrap());
        let shown = render_session(&s);
        assert_eq!(texts(&shown), vec!["one", "rewound question"], "its own branch");
    }

    #[test]
    fn a_rewound_branch_is_marked_not_editable() {
        // Marked the way pre-compaction segments are: an edit resolves by uuid
        // and would rewrite history the session already left behind.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let mut s = sess(vec![
            uev(a, None, "one"),
            uev(b, Some(a), "rewound question"),
            uev(c, Some(a), "asked again"),
        ]);
        s.leaf = crate::session::tree::branch_leaf(&s.events, b.parse().unwrap());
        let r = render_session(&s);
        assert!(
            r.turns.iter().all(|t| !matches!(t, RenderedTurn::User { editable: true, .. })),
            "no turn on a rewound branch is editable"
        );
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

    #[test]
    fn an_api_error_renders_as_its_own_turn_after_the_reply() {
        // The failure line is synthetic — normally hidden — but it is the only
        // record of why the session stopped, so it renders, and it must not be
        // merged into the assistant turn above it.
        let a1 = "00000000-0000-0000-0000-0000000000d1";
        let a2 = "00000000-0000-0000-0000-0000000000d2";
        let err = format!(
            r#"{{"type":"assistant","uuid":"{a2}","parentUuid":"{a1}","timestamp":"2026-08-21T13:29:12.117Z","isSidechain":false,"isApiErrorMessage":true,"message":{{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{{"type":"text","text":"Prompt is too long"}}]}}}}"#
        );
        let events = [aev(a1, None, "working on it"), serde_json::from_str(&err).unwrap()];
        let path: Vec<&Event> = events.iter().collect();
        let turns = render_path_turns(&path, true);

        assert_eq!(turns.len(), 2, "the error is its own turn, not merged away");
        assert!(matches!(turns[0], RenderedTurn::Assistant { .. }));
        let RenderedTurn::ApiError { text, timestamp } = &turns[1] else { panic!("api error turn") };
        assert_eq!(text, "Prompt is too long");
        assert!(timestamp.is_some());
    }

    #[test]
    fn ordinary_synthetic_lines_stay_hidden() {
        let a1 = "00000000-0000-0000-0000-0000000000d3";
        let a2 = "00000000-0000-0000-0000-0000000000d4";
        let filler = format!(
            r#"{{"type":"assistant","uuid":"{a2}","parentUuid":"{a1}","timestamp":"2026-08-21T13:29:12.117Z","isSidechain":false,"message":{{"role":"assistant","model":"<synthetic>","content":[{{"type":"text","text":"(no content)"}}]}}}}"#
        );
        let events = [aev(a1, None, "hi"), serde_json::from_str(&filler).unwrap()];
        let path: Vec<&Event> = events.iter().collect();
        assert_eq!(render_path_turns(&path, true).len(), 1);
    }

    #[test]
    fn task_notification_user_event_becomes_task_turn() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let tn = "<task-notification>\n<status>completed</status>\n<summary>did X</summary>\n<result>body</result>\n</task-notification>";
        let turns = render_path_turns(&[&uev(a, None, tn)], true);
        assert_eq!(turns.len(), 1);
        match &turns[0] {
            RenderedTurn::TaskNotification { status, summary, result, .. } => {
                assert_eq!(status, "completed");
                assert_eq!(summary, "did X");
                assert_eq!(result, "body");
            }
            other => panic!("expected TaskNotification, got {other:?}"),
        }
    }

    #[test]
    fn narration_blocks_render_and_real_thinking_stays_hidden() {
        // Claude Code stores its "summarized" line as a thinking block whose
        // text is present. Real thinking is persisted as an empty string, and
        // from 2.1.263 the signed blob tags each: `narration` vs `thinking`.
        let narration_sig = "CAQS3AsKEQgRGAI4AUIJbmFycmF0aW9uEgxeLYZBo19ITjALvVkaDGi9gbiBSXwmSG4ckSIw";
        let thinking_sig = "CAQSgA8KEAgRGAI4AUIIdGhpbmtpbmcSDNqP4yjVRuVuvUJ/";
        let mk = |thinking: &str, sig: Option<&str>| -> RenderedBlock {
            let sig = sig.map(|s| format!(r#","signature":"{s}""#)).unwrap_or_default();
            let line = format!(
                r#"{{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000d1","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"{thinking}"{sig}}}]}}}}"#
            );
            let ev: Event = serde_json::from_str(&line).unwrap();
            let events = [ev];
            let path: Vec<&Event> = events.iter().collect();
            let mut turns = render_path_turns(&path, true);
            let RenderedTurn::Assistant { blocks, .. } = turns.remove(0) else { panic!("assistant turn") };
            blocks.into_iter().next().unwrap()
        };

        // Tagged narration (2.1.263+).
        let RenderedBlock::Narration { text } = mk("Found the bug.", Some(narration_sig)) else {
            panic!("tagged narration renders")
        };
        assert_eq!(text, "Found the bug.");

        // Untagged but text-bearing (2.1.251 – 2.1.260 wrote summaries without the tag).
        assert!(matches!(mk("Older summary.", None), RenderedBlock::Narration { .. }));
        let opaque = "CAQS3AsKEQgRGAI4AUIJ";
        assert!(matches!(mk("Older summary.", Some(opaque)), RenderedBlock::Narration { .. }));

        // Empty text is redacted thinking whatever the tag says.
        assert!(matches!(mk("", Some(thinking_sig)), RenderedBlock::Thinking { .. }));
        assert!(matches!(mk("", None), RenderedBlock::Thinking { .. }));

        // Text explicitly tagged `thinking` is reasoning, not a summary.
        assert!(matches!(mk("raw reasoning", Some(thinking_sig)), RenderedBlock::Thinking { .. }));
    }
}

#[cfg(test)]
mod stats_tests {
    use super::*;

    fn line(json: &str) -> Event {
        serde_json::from_str(json).unwrap()
    }

    fn user(uuid: u8, parent: Option<u8>, at: &str) -> Event {
        let parent = parent.map_or("null".to_string(), |p| format!(r#""00000000-0000-0000-0000-0000000000{p:02x}""#));
        line(&format!(
            r#"{{"type":"user","uuid":"00000000-0000-0000-0000-0000000000{uuid:02x}","parentUuid":{parent},"timestamp":"2026-09-18T{at}Z","cwd":"/tmp/proj","gitBranch":"main","message":{{"role":"user","content":"go"}}}}"#
        ))
    }

    fn assistant(uuid: u8, parent: u8, at: &str, input: u64, output: u64, cache_read: u64) -> Event {
        line(&format!(
            r#"{{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000{uuid:02x}","parentUuid":"00000000-0000-0000-0000-0000000000{parent:02x}","timestamp":"2026-09-18T{at}Z","message":{{"role":"assistant","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":100,"cache_read_input_tokens":{cache_read}}}}}}}"#
        ))
    }

    fn refs(events: &[Event]) -> Vec<&Event> {
        events.iter().collect()
    }

    #[test]
    fn extending_with_appended_lines_matches_computing_over_the_whole_file() {
        let all = vec![
            user(1, None, "10:00:00"),
            assistant(2, 1, "10:00:05", 10, 200, 0),
            user(3, Some(2), "10:01:00"),
            assistant(4, 3, "10:01:30", 5, 50, 900),
        ];
        let mut stats = compute_stats(&refs(&all[..2]));
        extend_stats(&mut stats, &all[2..]);
        assert_eq!(stats, compute_stats(&refs(&all)));
        assert_eq!((stats.context_tokens, stats.duration_secs), (1005, Some(90)));
    }

    #[test]
    fn a_compaction_in_the_appended_lines_restarts_the_stats_at_its_tree() {
        let before = vec![user(1, None, "10:00:00"), assistant(2, 1, "10:00:05", 10, 200, 0)];
        // An auto-compaction starts a fresh null-parent tree in the same file.
        let after = vec![user(9, None, "10:30:00"), assistant(10, 9, "10:30:20", 7, 70, 3000)];
        let mut stats = compute_stats(&refs(&before));
        extend_stats(&mut stats, &after);
        assert_eq!(stats, compute_stats(&refs(&after)));
    }

    #[test]
    fn lines_outside_the_conversation_leave_the_stats_alone() {
        let before = vec![user(1, None, "10:00:00"), assistant(2, 1, "10:00:05", 10, 200, 0)];
        let mut stats = compute_stats(&refs(&before));
        extend_stats(&mut stats, &[line(r#"{"type":"ai-title","aiTitle":"Continue"}"#)]);
        assert_eq!(stats, compute_stats(&refs(&before)));
    }
}
