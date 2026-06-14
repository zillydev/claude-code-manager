use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use claude_manager_core::session::event::{Block as ABlock, Event, MessageContent, UserBlock};
use claude_manager_core::session::render::compute_stats;
use claude_manager_core::session::Session;
use crate::ui::theme;

pub enum TranscriptAction {
    None,
    Back,
    Quit,
}

/// One navigable item in the rendered transcript — a "place" the cursor can land.
#[derive(Debug, Clone)]
struct RenderItem {
    /// First line index for this item within `lines`.
    start: usize,
    /// Inclusive last line index.
    end: usize,
    kind: ItemKind,
}

#[derive(Debug, Clone)]
enum ItemKind {
    Turn,
    ToolCall { id: String },
}

pub struct TranscriptView {
    pub session: Session,
    expanded: HashSet<String>,
    show_details: bool,

    cache_width: u16,
    lines: Vec<Line<'static>>,
    items: Vec<RenderItem>,
    cursor_item: usize,
    scroll: u16,
    last_viewport_h: u16,
}

impl TranscriptView {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            expanded: HashSet::new(),
            show_details: true,
            cache_width: 0,
            lines: Vec::new(),
            items: Vec::new(),
            cursor_item: 0,
            scroll: 0,
            last_viewport_h: 1,
        }
    }

    /// Called by the app when new events have been appended (live tail).
    pub fn invalidate(&mut self) {
        self.cache_width = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TranscriptAction {
        match key.code {
            KeyCode::Char('q') => TranscriptAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                TranscriptAction::Quit
            }
            KeyCode::Esc => TranscriptAction::Back,
            KeyCode::Char('d') => {
                self.show_details = !self.show_details;
                TranscriptAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_item(1);
                TranscriptAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_item(-1);
                TranscriptAction::None
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(self.last_viewport_h.saturating_sub(2));
                self.clamp_scroll();
                TranscriptAction::None
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(self.last_viewport_h.saturating_sub(2));
                TranscriptAction::None
            }
            KeyCode::Char('g') => {
                self.cursor_item = 0;
                self.scroll = 0;
                TranscriptAction::None
            }
            KeyCode::Char('G') => {
                if !self.items.is_empty() {
                    self.cursor_item = self.items.len() - 1;
                }
                self.scroll_to_cursor();
                TranscriptAction::None
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(item) = self.items.get(self.cursor_item) {
                    if let ItemKind::ToolCall { id } = &item.kind {
                        let id = id.clone();
                        if self.expanded.contains(&id) {
                            self.expanded.remove(&id);
                        } else {
                            self.expanded.insert(id);
                        }
                        self.invalidate();
                    }
                }
                TranscriptAction::None
            }
            _ => TranscriptAction::None,
        }
    }

    fn move_item(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as i32;
        let mut next = self.cursor_item as i32 + delta;
        if next < 0 {
            next = 0;
        } else if next >= len {
            next = len - 1;
        }
        self.cursor_item = next as usize;
        self.scroll_to_cursor();
    }

    fn scroll_to_cursor(&mut self) {
        let Some(item) = self.items.get(self.cursor_item) else {
            return;
        };
        let start = item.start as u16;
        let end = item.end as u16;
        let h = self.last_viewport_h.max(1);
        if start < self.scroll {
            self.scroll = start;
        } else if end >= self.scroll.saturating_add(h) {
            self.scroll = end.saturating_sub(h - 1);
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let total = self.lines.len() as u16;
        let h = self.last_viewport_h.max(1);
        let max_scroll = total.saturating_sub(h);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let detail_height = if self.show_details { 4u16 } else { 1u16 };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(detail_height), // header + stats
                Constraint::Min(1),                // body
                Constraint::Length(1),             // keys
            ])
            .split(area);

        if self.show_details {
            let path = self.session.current_path();
            let stats = compute_stats(&path);
            let mut header_lines = Vec::new();

            // Line 1: project + title + events
            header_lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    self.session.project_label.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" › "),
                Span::raw(self.session.id.to_string()),
                Span::raw("  "),
                Span::styled(
                    format!("({} events)", self.session.events.len()),
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]));

            // Line 2: cwd + branch + duration
            let mut meta: Vec<Span> = vec![Span::raw(" ")];
            if let Some(cwd) = &stats.cwd {
                let short = cwd
                    .strip_prefix(&dirs::home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default())
                    .map(|rest| format!("~{}", rest))
                    .unwrap_or_else(|| cwd.clone());
                meta.push(Span::styled("cwd ", Style::default().add_modifier(Modifier::DIM)));
                meta.push(Span::raw(short));
                meta.push(Span::raw("  "));
            }
            if let Some(branch) = &stats.git_branch {
                meta.push(Span::styled("branch ", Style::default().add_modifier(Modifier::DIM)));
                meta.push(Span::styled(branch.clone(), theme::user()));
                meta.push(Span::raw("  "));
            }
            if let Some(dur) = stats.duration_secs {
                meta.push(Span::styled("duration ", Style::default().add_modifier(Modifier::DIM)));
                meta.push(Span::raw(format_duration(dur)));
            }
            header_lines.push(Line::from(meta));

            // Line 3: models
            let mut model_spans: Vec<Span> = vec![
                Span::raw(" "),
                Span::styled("models ", Style::default().add_modifier(Modifier::DIM)),
            ];
            for m in &stats.models {
                let short = m.model.strip_prefix("claude-").unwrap_or(&m.model);
                model_spans.push(Span::styled(
                    format!("{}×{}", short, m.turn_count),
                    theme::assistant(),
                ));
                model_spans.push(Span::raw("  "));
            }
            header_lines.push(Line::from(model_spans));

            // Line 4: tokens
            let total = stats.tokens.input + stats.tokens.output + stats.tokens.cache_creation;
            header_lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("tokens ", Style::default().add_modifier(Modifier::DIM)),
                Span::raw(format!("{} total  ", compact(total))),
                Span::styled(
                    format!(
                        "{} in · {} out · {} cache-w · {} cache-r",
                        compact(stats.tokens.input),
                        compact(stats.tokens.output),
                        compact(stats.tokens.cache_creation),
                        compact(stats.tokens.cache_read),
                    ),
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]));

            let header = Paragraph::new(header_lines).style(theme::header());
            f.render_widget(header, chunks[0]);
        } else {
            let header = Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    self.session.project_label.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(self.session.id.to_string()),
                Span::raw("  "),
                Span::styled(
                    format!("({} events)", self.session.events.len()),
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]))
            .style(theme::header());
            f.render_widget(header, chunks[0]);
        }

        let body_area = chunks[1];
        self.last_viewport_h = body_area.height;

        // Rebuild render cache if width changed or invalidated.
        if self.cache_width != body_area.width {
            self.rebuild(body_area.width);
            self.scroll_to_cursor();
        }

        let body = Paragraph::new(self.lines.clone())
            .block(Block::default().borders(Borders::NONE))
            .scroll((self.scroll, 0));
        f.render_widget(body, body_area);

        let mut keys = " ↑↓/jk move   Enter expand   d details   g/G top/bot   Esc back   q quit ".to_string();
        if self.show_details {
            keys.push_str("[details] ");
        }
        let keys_p = Paragraph::new(keys).style(theme::footer());
        f.render_widget(keys_p, chunks[2]);
    }

    fn rebuild(&mut self, width: u16) {
        self.cache_width = width;
        let inner_width = width.saturating_sub(2).max(20) as usize; // a small margin

        // Index tool_results from user blocks so we can pair them.
        let path = self.session.current_path();
        let tool_results = collect_tool_results(&path);

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut items: Vec<RenderItem> = Vec::new();

        // Identify user events that are pure tool_result containers (these are
        // already represented by the paired ToolUse and should not be rendered
        // as their own turn).
        let mut suppressed_user: HashSet<usize> = HashSet::new();
        for (idx, ev) in path.iter().enumerate() {
            if let Event::User(u) = ev {
                if let MessageContent::Blocks(blocks) = &u.message.content {
                    let only_tool_results = !blocks.is_empty()
                        && blocks
                            .iter()
                            .all(|b| matches!(b, UserBlock::ToolResult { .. }));
                    if only_tool_results {
                        suppressed_user.insert(idx);
                    }
                }
            }
        }

        for (idx, ev) in path.iter().enumerate() {
            if suppressed_user.contains(&idx) {
                continue;
            }
            match ev {
                Event::User(u) => {
                    if u.is_meta.unwrap_or(false) {
                        continue;
                    }
                    let start = lines.len();
                    let ts = format_ts(ev.timestamp());
                    let role_line = Line::from(vec![
                        Span::styled(format!("[{}] user", ts), theme::user()),
                    ]);
                    lines.push(role_line);
                    match &u.message.content {
                        MessageContent::Text(t) => {
                            push_wrapped(&mut lines, t, "  ", inner_width, Style::default());
                        }
                        MessageContent::Blocks(blocks) => {
                            for b in blocks {
                                if let UserBlock::Text { text } = b {
                                    push_wrapped(&mut lines, text, "  ", inner_width, Style::default());
                                }
                            }
                        }
                    }
                    lines.push(Line::from(""));
                    items.push(RenderItem {
                        start,
                        end: lines.len().saturating_sub(1),
                        kind: ItemKind::Turn,
                    });
                }
                Event::Assistant(a) => {
                    let start = lines.len();
                    let ts = format_ts(ev.timestamp());
                    let model = a.message.model.as_deref().unwrap_or("");
                    let header_text = if model.is_empty() {
                        format!("[{}] assistant", ts)
                    } else {
                        format!("[{}] assistant ({})", ts, short_model(model))
                    };
                    lines.push(Line::from(vec![Span::styled(header_text, theme::assistant())]));

                    // Track each tool call's *exact* line index as we push it, so
                    // that multiple calls with the same tool name resolve back to
                    // the right line later. (id, line_idx_before_expansion).
                    let mut tool_emits: Vec<(String, usize)> = Vec::new();

                    for blk in &a.message.content {
                        match blk {
                            ABlock::Text { text } => {
                                push_wrapped(&mut lines, text, "  ", inner_width, Style::default());
                            }
                            ABlock::Thinking { .. } => {}
                            ABlock::ToolUse { id, name, input } => {
                                let summary = summarize_tool_input(name, input);
                                let arrow = if self.expanded.contains(id) { "▾" } else { "▸" };
                                lines.push(Line::from(vec![
                                    Span::raw("  "),
                                    Span::styled(format!("{} {}: ", arrow, name), theme::tool_use()),
                                    Span::raw(summary),
                                ]));
                                tool_emits.push((id.clone(), lines.len() - 1));
                            }
                            ABlock::Unknown => {}
                        }
                    }

                    // Expand any expanded tool results. Insert results in reverse
                    // line order so earlier indices remain valid.
                    //
                    // We also keep `tool_emits` updated: after each insertion of
                    // N lines at position `at`, every tool_emit with line >= at
                    // shifts by N. Since we walk in reverse-line order, only the
                    // tools at higher line indices have already been processed,
                    // so we shift in-place for any later-inserted tools by
                    // recomputing offsets from the original emit positions.
                    let mut expansions: Vec<(usize, Vec<Line<'static>>)> = Vec::new();
                    for (id, line_idx) in &tool_emits {
                        if !self.expanded.contains(id) {
                            continue;
                        }
                        let (result_text, is_error) = tool_results
                            .get(id.as_str())
                            .map(|r| (r.text.clone(), r.is_error))
                            .unwrap_or_else(|| ("(no result)".to_string(), false));
                        let style = if is_error {
                            Style::default()
                                .fg(ratatui::style::Color::Red)
                                .add_modifier(Modifier::DIM)
                        } else {
                            theme::tool_result()
                        };
                        let mut payload: Vec<Line<'static>> = Vec::new();
                        push_wrapped(&mut payload, &result_text, "    │ ", inner_width, style);
                        expansions.push((*line_idx + 1, payload));
                    }
                    expansions.sort_by(|a, b| b.0.cmp(&a.0));

                    // Apply expansions and adjust tool_emits indices accordingly.
                    for (at, payload) in expansions {
                        let len = payload.len();
                        for (i, l) in payload.into_iter().enumerate() {
                            lines.insert(at + i, l);
                        }
                        // Shift any tool emits whose line >= at by len.
                        for (_, line_idx) in tool_emits.iter_mut() {
                            if *line_idx >= at {
                                *line_idx += len;
                            }
                        }
                    }

                    // Build items: one Turn covering header + body up to the first tool,
                    // then one ToolCall item per tool emit (covering its line + any expanded result).
                    let turn_end = tool_emits
                        .first()
                        .map(|(_, li)| li.saturating_sub(1))
                        .unwrap_or(lines.len().saturating_sub(1));
                    items.push(RenderItem {
                        start,
                        end: turn_end,
                        kind: ItemKind::Turn,
                    });

                    for (i, (id, li)) in tool_emits.iter().enumerate() {
                        let next_li = tool_emits.get(i + 1).map(|(_, n)| *n);
                        let end = next_li.map(|n| n - 1).unwrap_or(lines.len() - 1);
                        items.push(RenderItem {
                            start: *li,
                            end,
                            kind: ItemKind::ToolCall { id: id.clone() },
                        });
                    }
                    lines.push(Line::from(""));
                }
                Event::System(s) => {
                    if s.subtype.as_deref() == Some("local_command")
                        && s.content.contains("<command-name>")
                    {
                        let start = lines.len();
                        let ts = format_ts(ev.timestamp());
                        let cmd = extract_tag(&s.content, "command-name").unwrap_or("");
                        lines.push(Line::from(Span::styled(
                            format!("[{}] system: /{}", ts, cmd),
                            theme::system(),
                        )));
                        lines.push(Line::from(""));
                        items.push(RenderItem {
                            start,
                            end: lines.len() - 1,
                            kind: ItemKind::Turn,
                        });
                    }
                    // Other system entries (e.g. resume stdout) are silently dropped.
                }
                Event::Attachment(_) => {
                    // SessionStart hooks etc. — suppressed in v1.
                }
                Event::Summary(s) => {
                    let start = lines.len();
                    lines.push(Line::from(Span::styled("── compaction summary ──", theme::meta())));
                    if let Some(text) = &s.summary {
                        push_wrapped(&mut lines, text, "  ", inner_width, theme::meta());
                    }
                    lines.push(Line::from(""));
                    items.push(RenderItem {
                        start,
                        end: lines.len() - 1,
                        kind: ItemKind::Turn,
                    });
                }
                _ => {}
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "(empty conversation)",
                theme::meta(),
            )));
        }

        // Apply cursor highlight: invert the first line of the focused item.
        if let Some(item) = items.get(self.cursor_item) {
            if let Some(line) = lines.get_mut(item.start) {
                for span in line.spans.iter_mut() {
                    span.style = span.style.add_modifier(Modifier::REVERSED);
                }
            }
        }

        self.lines = lines;
        self.items = items;
        if self.cursor_item >= self.items.len() && !self.items.is_empty() {
            self.cursor_item = self.items.len() - 1;
        }
    }
}

struct ToolResultIndex<'a> {
    text: String,
    is_error: bool,
    _phantom: std::marker::PhantomData<&'a ()>,
}

fn collect_tool_results<'a>(path: &[&'a Event]) -> HashMap<String, ToolResultIndex<'a>> {
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
                            ToolResultIndex {
                                text: content.as_text(),
                                is_error: *is_error,
                                _phantom: std::marker::PhantomData,
                            },
                        );
                    }
                }
            }
        }
    }
    out
}

fn format_ts(ts: Option<chrono::DateTime<chrono::Utc>>) -> String {
    use chrono::Local;
    match ts {
        Some(t) => t.with_timezone(&Local).format("%H:%M").to_string(),
        None => "--:--".to_string(),
    }
}

fn short_model(model: &str) -> &str {
    // claude-opus-4-7 -> opus-4-7
    model.strip_prefix("claude-").unwrap_or(model)
}

fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    // Heuristics matching common tools.
    let candidate_keys: &[&str] = match name {
        "Bash" => &["command", "description"],
        "Read" => &["file_path"],
        "Write" => &["file_path"],
        "Edit" => &["file_path"],
        "Glob" => &["pattern", "path"],
        "Grep" => &["pattern", "path"],
        _ => &["query", "file_path", "path", "command", "description", "prompt"],
    };
    for k in candidate_keys {
        if let Some(v) = input.get(*k) {
            if let Some(s) = v.as_str() {
                return truncate(s, 80);
            }
        }
    }
    // Fallback: stringify compactly.
    let s = input.to_string();
    truncate(&s, 80)
}

fn truncate(s: &str, max: usize) -> String {
    let one_line: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if one_line.chars().count() <= max {
        one_line
    } else if max <= 1 {
        "…".to_string()
    } else {
        let head: String = one_line.chars().take(max - 1).collect();
        format!("{}…", head)
    }
}

fn format_duration(secs: i64) -> String {
    if secs < 60 {
        return format!("{}s", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m", mins);
    }
    let hours = mins / 60;
    let rem = mins % 60;
    if rem > 0 {
        format!("{}h {}m", hours, rem)
    } else {
        format!("{}h", hours)
    }
}

fn compact(n: u64) -> String {
    if n < 1_000 {
        format!("{}", n)
    } else if n < 1_000_000 {
        format!("{}k", n / 1_000)
    } else {
        format!("{:.1}M", (n as f64) / 1_000_000.0)
    }
}

fn extract_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = s.find(&open)? + open.len();
    let j = s[i..].find(&close)? + i;
    Some(&s[i..j])
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_manager_core::session::event::{
        AssistantEvent, AssistantMessage, Block, Message, MessageContent, ToolResultContent,
        UserBlock, UserEvent,
    };
    use claude_manager_core::session::Session;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;
    use uuid::Uuid;

    fn make_session_with_two_bash_calls() -> Session {
        let a_uuid = Uuid::new_v4();
        let u_uuid = Uuid::new_v4();
        let assistant = Event::Assistant(AssistantEvent {
            uuid: Some(a_uuid),
            parent_uuid: None,
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            cwd: None, git_branch: None, forked_from: None,
            message: AssistantMessage {
                model: Some("claude-opus-4-7".into()),
                stop_reason: None,
                content: vec![
                    Block::Text { text: "Running two commands.".into() },
                    Block::ToolUse {
                        id: "tool_A".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({"command": "ls", "description": "list files"}),
                    },
                    Block::ToolUse {
                        id: "tool_B".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({"command": "pwd", "description": "print cwd"}),
                    },
                ],
                usage: None,
            },
        });
        let user_result = Event::User(UserEvent {
            uuid: Some(u_uuid),
            parent_uuid: Some(a_uuid),
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            is_meta: None,
            cwd: None, git_branch: None, forked_from: None,
            tool_use_result: None,
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Blocks(vec![
                    UserBlock::ToolResult {
                        tool_use_id: "tool_A".into(),
                        content: ToolResultContent::Text("AAAA result".into()),
                        is_error: false,
                    },
                    UserBlock::ToolResult {
                        tool_use_id: "tool_B".into(),
                        content: ToolResultContent::Text("BBBB result".into()),
                        is_error: false,
                    },
                ]),
            },
        });

        Session {
            project_label: "test".into(),
            id: Uuid::new_v4(),
            file_path: PathBuf::from("/tmp/x.jsonl"),
            events: vec![assistant, user_result],
            leaf: Some(u_uuid),
            mtime: UNIX_EPOCH,
            bytes_read: 0,
        }
    }

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn multiple_tool_calls_with_same_name_each_track_their_own_line() {
        let session = make_session_with_two_bash_calls();
        let mut view = TranscriptView::new(session);
        view.rebuild(80);

        // Two distinct ToolCall items, in order.
        let tool_items: Vec<&RenderItem> = view
            .items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::ToolCall { .. }))
            .collect();
        assert_eq!(tool_items.len(), 2, "expected 2 tool-call items");

        let ids: Vec<&str> = tool_items
            .iter()
            .filter_map(|i| match &i.kind {
                ItemKind::ToolCall { id } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec!["tool_A", "tool_B"]);

        // The start line of each item should land on the right rendered tool line.
        let line_a = line_text(&view.lines[tool_items[0].start]);
        let line_b = line_text(&view.lines[tool_items[1].start]);
        assert!(line_a.contains("ls"), "first tool item should be the `ls` line, got: {}", line_a);
        assert!(line_b.contains("pwd"), "second tool item should be the `pwd` line, got: {}", line_b);
    }

    #[test]
    fn expanding_second_tool_attaches_correct_result() {
        let session = make_session_with_two_bash_calls();
        let mut view = TranscriptView::new(session);
        view.expanded.insert("tool_B".to_string());
        view.rebuild(80);

        // Find the line carrying `pwd`. Its next line should contain "BBBB result"
        // (not "AAAA result").
        let pwd_line = view
            .lines
            .iter()
            .position(|l| line_text(l).contains("pwd"))
            .expect("pwd tool line missing");
        let next = line_text(&view.lines[pwd_line + 1]);
        assert!(
            next.contains("BBBB"),
            "expected BBBB result after pwd tool line, got: {}",
            next
        );

        // And the `ls` line should NOT have an expanded result attached.
        let ls_line = view
            .lines
            .iter()
            .position(|l| line_text(l).contains("ls"))
            .expect("ls tool line missing");
        // The line after `ls` should be the `pwd` tool line, not a result line.
        let after_ls = line_text(&view.lines[ls_line + 1]);
        assert!(
            !after_ls.contains("AAAA"),
            "ls (not expanded) should not have its result inline, got: {}",
            after_ls
        );
    }

    #[test]
    fn expanding_both_tools_attaches_each_to_its_own_call() {
        let session = make_session_with_two_bash_calls();
        let mut view = TranscriptView::new(session);
        view.expanded.insert("tool_A".to_string());
        view.expanded.insert("tool_B".to_string());
        view.rebuild(80);

        let ls_line = view
            .lines
            .iter()
            .position(|l| line_text(l).contains(" ls"))
            .expect("ls tool line missing");
        assert!(line_text(&view.lines[ls_line + 1]).contains("AAAA"));

        let pwd_line = view
            .lines
            .iter()
            .position(|l| line_text(l).contains(" pwd"))
            .expect("pwd tool line missing");
        assert!(line_text(&view.lines[pwd_line + 1]).contains("BBBB"));
    }
}

fn push_wrapped(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    indent: &'static str,
    width: usize,
    style: Style,
) {
    let body_width = width.saturating_sub(indent.chars().count()).max(20);
    for raw_line in text.lines() {
        if raw_line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }
        let mut current = String::new();
        let mut current_chars = 0;
        for word in raw_line.split(' ') {
            let w_chars = word.chars().count();
            if current_chars == 0 {
                current.push_str(word);
                current_chars = w_chars;
            } else if current_chars + 1 + w_chars <= body_width {
                current.push(' ');
                current.push_str(word);
                current_chars += 1 + w_chars;
            } else {
                lines.push(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(std::mem::take(&mut current), style),
                ]));
                current.push_str(word);
                current_chars = w_chars;
            }
        }
        if !current.is_empty() {
            lines.push(Line::from(vec![
                Span::raw(indent),
                Span::styled(current, style),
            ]));
        }
    }
}

