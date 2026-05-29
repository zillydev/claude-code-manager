use std::path::PathBuf;
use std::time::SystemTime;

use uuid::Uuid;

use super::event::{Event, MessageContent, Usage};

/// One row of the sessions list.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub file_path: PathBuf,
    pub project_label: String,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub title: String,
    pub mtime: SystemTime,
    pub total_tokens: u64,
}

const TITLE_MAX_CHARS: usize = 60;

/// Walk events once and produce the display summary.
pub fn summarize(
    events: &[Event],
    id: Uuid,
    file_path: PathBuf,
    project_label: String,
    mtime: SystemTime,
) -> SessionSummary {
    let mut custom_title = None;
    let mut last_user_text = None;
    let mut last_prompt = None;
    let mut total_tokens: u64 = 0;
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;

    for ev in events {
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
        match ev {
            Event::CustomTitle(t) if custom_title.is_none() => {
                custom_title = Some(t.custom_title.clone());
            }
            Event::AiTitle(_) => {}
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
                if u.is_meta.unwrap_or(false) {
                    continue;
                }
                if u.is_sidechain {
                    continue;
                }
                if let MessageContent::Text(t) = &u.message.content {
                    let trimmed = t.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed.starts_with("<command-")
                        || trimmed.starts_with("<local-command-")
                    {
                        continue;
                    }
                    last_user_text = Some(trimmed.to_string());
                }
            }
            Event::Assistant(a) => {
                if let Some(Usage {
                    input_tokens,
                    output_tokens,
                    cache_creation_input_tokens,
                    ..
                }) = a.message.usage
                {
                    total_tokens = total_tokens
                        .saturating_add(input_tokens)
                        .saturating_add(output_tokens)
                        .saturating_add(cache_creation_input_tokens);
                }
            }
            _ => {}
        }
    }

    let title = custom_title
        .or(last_prompt)
        .or(last_user_text)
        .map(strip_command_markup)
        .map(truncate_title)
        .unwrap_or_else(|| "(untitled)".to_string());

    SessionSummary {
        id,
        file_path,
        project_label,
        cwd,
        git_branch,
        title,
        mtime,
        total_tokens,
    }
}

/// Slash-command echoes are wrapped in tags like
/// `<command-message>foo</command-message> <command-name>/foo</command-name>
/// <command-args>bar</command-args>`. When such markup appears, pull out the
/// command name + args for display and drop the rest.
fn strip_command_markup(s: String) -> String {
    if !s.contains("<command-") && !s.contains("<local-command-") {
        return s;
    }
    let name = extract_tag(&s, "command-name").map(str::trim);
    let args = extract_tag(&s, "command-args").map(str::trim);
    match (name, args) {
        (Some(n), Some(a)) if !a.is_empty() => format!("{} {}", n, a),
        (Some(n), _) => n.to_string(),
        _ => s,
    }
}

fn extract_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = s.find(&open)? + open.len();
    let j = s[i..].find(&close)? + i;
    Some(&s[i..j])
}

fn truncate_title(s: String) -> String {
    let collapsed: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let collapsed = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= TITLE_MAX_CHARS {
        collapsed
    } else {
        let truncated: String = collapsed.chars().take(TITLE_MAX_CHARS - 1).collect();
        format!("{}…", truncated)
    }
}

/// "2m", "3h", "5d" relative time string.
pub fn relative_time(t: SystemTime) -> String {
    let now = SystemTime::now();
    let dur = now.duration_since(t).unwrap_or_default();
    let secs = dur.as_secs();
    if secs < 60 {
        return format!("{}s", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m", mins);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{}h", hours);
    }
    let days = hours / 24;
    format!("{}d", days)
}

/// "48k", "1.2M" compact token count.
pub fn compact_tokens(n: u64) -> String {
    if n < 1_000 {
        format!("{}", n)
    } else if n < 1_000_000 {
        format!("{}k", n / 1_000)
    } else {
        format!("{:.1}M", (n as f64) / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::event::{
        AiTitleEvent, AssistantEvent, AssistantMessage, CustomTitleEvent, Message, MessageContent,
        Usage, UserEvent,
    };
    use chrono::Utc;
    use std::time::UNIX_EPOCH;

    fn assistant_with_tokens(input: u64, output: u64) -> Event {
        Event::Assistant(AssistantEvent {
            uuid: Some(uuid::Uuid::new_v4()),
            parent_uuid: None,
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            message: AssistantMessage {
                model: Some("claude-opus-4-7".into()),
                stop_reason: None,
                content: vec![],
                usage: Some(Usage {
                    input_tokens: input,
                    output_tokens: output,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                }),
            },
            cwd: None, git_branch: None,
        })
    }

    fn user(text: &str, meta: bool) -> Event {
        Event::User(UserEvent {
            uuid: Some(uuid::Uuid::new_v4()),
            parent_uuid: None,
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            is_meta: Some(meta),
            message: Message {
                role: Some("user".into()),
                content: MessageContent::Text(text.into()),
            },
            tool_use_result: None,
            cwd: None, git_branch: None,
        })
    }

    #[test]
    fn custom_title_wins() {
        let events = vec![
            Event::AiTitle(AiTitleEvent { ai_title: "ai-one".into() }),
            Event::CustomTitle(CustomTitleEvent { custom_title: "custom".into() }),
            user("a user message", false),
        ];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.title, "custom");
    }

    #[test]
    fn ai_title_ignored() {
        let events = vec![
            Event::AiTitle(AiTitleEvent { ai_title: "ai-one".into() }),
            user("a user message", false),
        ];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.title, "a user message");
    }

    #[test]
    fn last_user_message_used() {
        let events = vec![user("first msg", false), user("last msg", false)];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.title, "last msg");
    }

    #[test]
    fn meta_user_message_skipped() {
        let events = vec![user("a meta thing", true), user("real user", false)];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.title, "real user");
    }

    #[test]
    fn untitled_when_no_signal() {
        let events: Vec<Event> = vec![];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.title, "(untitled)");
    }

    #[test]
    fn tokens_sum_excludes_cache_read() {
        let events = vec![
            assistant_with_tokens(10, 20),
            assistant_with_tokens(5, 5),
        ];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.total_tokens, 40);
    }

    #[test]
    fn truncate_long_title() {
        let long = "x".repeat(100);
        let out = truncate_title(long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), TITLE_MAX_CHARS);
    }

    #[test]
    fn compact_tokens_works() {
        assert_eq!(compact_tokens(500), "500");
        assert_eq!(compact_tokens(48_000), "48k");
        assert_eq!(compact_tokens(1_234_000), "1.2M");
    }
}
