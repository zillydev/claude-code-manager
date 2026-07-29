use std::path::PathBuf;
use std::time::SystemTime;

use uuid::Uuid;

use super::display::extract_tag;
use super::event::{Event, MessageContent};

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
    /// Timestamp of the last real conversation turn (see `Event::is_activity_turn`).
    /// Drives the sidebar running/unread indicators; `None` for a session with no
    /// turns yet. Never reflects mtime, so a rename never looks like activity.
    pub last_event_at: Option<SystemTime>,
    /// Whether the last real turn is a completed (non-sidechain, `end_turn`)
    /// assistant turn — i.e. claude is done and it's the user's turn. When false
    /// the session is mid-turn (claude working/thinking). Distinguishes "running"
    /// from "unread" so a thinking pause doesn't flip a live session to unread.
    pub awaiting_input: bool,
    /// Current context-window size — the last real assistant turn's full
    /// request input (input + cache_creation + cache_read). Matches the token
    /// count Claude Code's TUI shows, not a cumulative lifetime sum.
    pub total_tokens: u64,
    /// The source session this one was forked from (Claude Code's `forkedFrom`,
    /// also written by claude-code-manager forks/edits). `None` if not derived.
    pub forked_from: Option<String>,
    /// Ancestor session this one's history was copied from, inferred from shared
    /// message uuids (fork / resume / background-restart copies that Claude Code
    /// does not record with an explicit `forkedFrom`). `None` when not a copy or
    /// when `forked_from` already links it. Resolved by `store::discover`.
    pub copied_from: Option<String>,
    /// Number of messages shared with `copied_from` (display: "N shared").
    pub copied_overlap: Option<usize>,
    /// The parent MESSAGE uuid this session forked at (`forkedFrom.messageUuid`).
    /// The branch point for an explicit fork; `None` otherwise.
    pub forked_from_message: Option<String>,
    /// The parent MESSAGE uuid a detected copy branched at — the copy's head
    /// (shared root) uuid, which is a message in `copied_from`. Set by
    /// `store::discover`; `None` otherwise.
    pub copied_from_message: Option<String>,
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
    let mut forked_from: Option<String> = None;
    let mut forked_from_message: Option<String> = None;
    let mut last_event_at: Option<SystemTime> = None;
    let mut awaiting_input = false;

    for ev in events {
        if ev.is_activity_turn() {
            if let Some(ts) = ev.timestamp() {
                last_event_at = Some(SystemTime::from(ts));
            }
            // Last activity turn wins. Shares `awaits_user` with the cheap tail
            // probe in `parse::last_activity` so the two can't disagree.
            awaiting_input = ev.awaits_user();
        }
        if let Some(src) = ev.forked_from_session() {
            if forked_from.is_none() {
                forked_from = Some(src.to_string());
            }
            // `forkedFrom.messageUuid` is per-line provenance — every copied line
            // records its OWN original uuid. The fork POINT is the deepest copied
            // message, so the LAST one wins (forked_from_session is only set on
            // user/assistant events).
            if let Some(m) = ev.forked_from_message() {
                forked_from_message = Some(m.to_string());
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
        match ev {
            // Last wins: Claude Code appends a new custom-title event on each
            // /rename, so the most recent one is the current title.
            Event::CustomTitle(t) => {
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
            Event::Assistant(a)
                // Report the current context-window size, not a cumulative sum:
                // the LAST real assistant turn's full request input
                // (fresh input + cache writes + cache reads). This matches the
                // token count Claude Code's TUI shows ("how big is this
                // conversation now"), rather than total tokens ever processed.
                if a.message.model.as_deref() != Some("<synthetic>") => {
                    if let Some(u) = &a.message.usage {
                        total_tokens = u
                            .input_tokens
                            .saturating_add(u.cache_creation_input_tokens)
                            .saturating_add(u.cache_read_input_tokens);
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
        last_event_at,
        awaiting_input,
        total_tokens,
        forked_from,
        copied_from: None,
        copied_overlap: None,
        forked_from_message,
        copied_from_message: None,
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
        AiTitleEvent, AssistantEvent, AssistantMessage, Block, CustomTitleEvent, ForkedFrom,
        Message, MessageContent, Usage, UserEvent,
    };
    use chrono::{TimeZone, Utc};
    use std::time::UNIX_EPOCH;

    fn assistant_with_usage(input: u64, cache_creation: u64, cache_read: u64) -> Event {
        Event::Assistant(AssistantEvent {
            uuid: Some(uuid::Uuid::new_v4()),
            parent_uuid: None,
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            message: AssistantMessage {
                model: Some("claude-opus-4-8".into()),
                stop_reason: None,
                content: vec![],
                usage: Some(Usage {
                    input_tokens: input,
                    output_tokens: 0,
                    cache_creation_input_tokens: cache_creation,
                    cache_read_input_tokens: cache_read,
                }),
            },
            cwd: None, git_branch: None, forked_from: None,
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
            cwd: None, git_branch: None, forked_from: None,
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
    fn last_custom_title_wins() {
        // Claude Code appends a fresh custom-title event on every /rename, so the
        // CURRENT title is the most recent one. An early stray title (here, a
        // local-command echo that got captured) must not win.
        let events = vec![
            Event::CustomTitle(CustomTitleEvent {
                custom_title: "<local-command-caveat>old garbage".into(),
            }),
            user("a user message", false),
            Event::CustomTitle(CustomTitleEvent { custom_title: "stripe-branch".into() }),
        ];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.title, "stripe-branch");
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
    fn last_event_at_is_last_real_turn_ignoring_title() {
        let t1 = Utc.timestamp_opt(1_000, 0).unwrap();
        let t2 = Utc.timestamp_opt(2_000, 0).unwrap();
        let mut u = match user("hi", false) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        u.timestamp = Some(t1);
        let mut a = match assistant_with_usage(1, 1, 1) {
            Event::Assistant(a) => a,
            _ => unreachable!(),
        };
        a.timestamp = Some(t2);
        // A trailing /rename custom-title line must NOT count as activity.
        let events = vec![
            Event::User(u),
            Event::Assistant(a),
            Event::CustomTitle(CustomTitleEvent { custom_title: "renamed".into() }),
        ];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.last_event_at, Some(SystemTime::from(t2)));
    }

    #[test]
    fn last_event_at_ignores_meta_user() {
        let t1 = Utc.timestamp_opt(1_000, 0).unwrap();
        let t2 = Utc.timestamp_opt(2_000, 0).unwrap();
        let mut real = match user("real", false) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        real.timestamp = Some(t1);
        let mut meta = match user("meta", true) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        meta.timestamp = Some(t2);
        let s = summarize(
            &[Event::User(real), Event::User(meta)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.last_event_at, Some(SystemTime::from(t1)));
    }

    #[test]
    fn last_event_at_none_when_no_turns() {
        let s = summarize(
            &[Event::CustomTitle(CustomTitleEvent { custom_title: "x".into() })],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.last_event_at, None);
    }

    fn assistant_stop(reason: &str, sidechain: bool) -> Event {
        let mut a = match assistant_with_usage(1, 1, 1) {
            Event::Assistant(a) => a,
            _ => unreachable!(),
        };
        a.message.stop_reason = Some(reason.into());
        a.is_sidechain = sidechain;
        Event::Assistant(a)
    }

    #[test]
    fn awaiting_input_true_when_last_turn_is_assistant_end_turn() {
        let s = summarize(
            &[user("hi", false), assistant_stop("end_turn", false)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert!(s.awaiting_input);
    }

    #[test]
    fn awaiting_input_false_when_last_turn_is_user() {
        let s = summarize(
            &[assistant_stop("end_turn", false), user("hi", false)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert!(!s.awaiting_input, "user spoke last → claude's turn (mid-turn)");
    }

    #[test]
    fn awaiting_input_false_when_assistant_still_using_tools() {
        let s = summarize(
            &[assistant_stop("tool_use", false)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert!(!s.awaiting_input, "pending tool_use → mid-turn");
    }

    #[test]
    fn awaiting_input_true_for_a_pending_question() {
        // Same rule as the cheap tail probe: AskUserQuestion stops with
        // `tool_use` but parks on the user, so it is "your turn". Both paths
        // feed the sidebar (`act?.awaiting ?? s.awaiting_input`), so they must
        // agree — this pins the full-parse side.
        let ev = Event::Assistant(AssistantEvent {
            uuid: Some(uuid::Uuid::new_v4()),
            parent_uuid: None,
            timestamp: Some(Utc::now()),
            is_sidechain: false,
            message: AssistantMessage {
                model: Some("claude".into()),
                stop_reason: Some("tool_use".into()),
                content: vec![Block::ToolUse {
                    id: "t1".into(),
                    name: "AskUserQuestion".into(),
                    input: serde_json::Value::Null,
                }],
                usage: None,
            },
            cwd: None,
            git_branch: None,
            forked_from: None,
        });
        let s = summarize(
            &[ev],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert!(s.awaiting_input, "pending question → your turn");
    }

    #[test]
    fn awaiting_input_false_when_only_a_subagent_finished() {
        // A sub-agent's end_turn doesn't mean the main agent is done.
        let s = summarize(
            &[user("hi", false), assistant_stop("end_turn", true)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert!(!s.awaiting_input, "sidechain end_turn → main thread still working");
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
    fn forked_from_read_from_provenance() {
        let mut u = match user("hi", false) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        u.forked_from = Some(ForkedFrom { session_id: "src-session".into(), message_uuid: None });
        let s = summarize(
            &[Event::User(u)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.forked_from.as_deref(), Some("src-session"));

        // A non-derived session leaves it None.
        let plain = summarize(
            &[user("hi", false)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(plain.forked_from, None);
    }

    #[test]
    fn forked_from_message_read_from_provenance() {
        let mut u = match user("hi", false) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        u.forked_from = Some(ForkedFrom {
            session_id: "src-session".into(),
            message_uuid: Some("msg-7".into()),
        });
        let s = summarize(
            &[Event::User(u)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.forked_from.as_deref(), Some("src-session"));
        assert_eq!(s.forked_from_message.as_deref(), Some("msg-7"));

        // No provenance => both None.
        let plain = summarize(
            &[user("hi", false)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(plain.forked_from_message, None);
    }

    #[test]
    fn forked_from_message_is_the_deepest_copied_turn() {
        // Every copied line carries forkedFrom with its OWN original uuid, so the
        // fork POINT is the deepest (last) copied message, not the first.
        let mk = |msg: &str| {
            let mut u = match user("x", false) {
                Event::User(u) => u,
                _ => unreachable!(),
            };
            u.forked_from =
                Some(ForkedFrom { session_id: "src".into(), message_uuid: Some(msg.into()) });
            Event::User(u)
        };
        let events = vec![mk("first-copied"), mk("deepest-copied")];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "p".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.forked_from.as_deref(), Some("src"), "session id is first-wins");
        assert_eq!(
            s.forked_from_message.as_deref(),
            Some("deepest-copied"),
            "fork point is the deepest copied message"
        );
    }

    #[test]
    fn context_tokens_uses_last_turn_full_input() {
        // total_tokens reflects the current context-window size: the LAST real
        // assistant turn's full request input (input + cache_creation +
        // cache_read), not a cumulative sum across turns.
        let events = vec![
            assistant_with_usage(100, 50, 200), // earlier turn — ignored
            assistant_with_usage(10, 5, 700),   // last turn → 10 + 5 + 700 = 715
        ];
        let s = summarize(
            &events,
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.total_tokens, 715);
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
