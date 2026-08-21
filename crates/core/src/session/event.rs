// This module models the on-disk jsonl wire format. Several fields are
// documented here for future use (warnings page, cost roll-ups, attachment
// rendering) but not consumed in the current UI; the dead-code lint is
// silenced module-wide so the schema stays expressive.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

/// One line of a Claude Code session jsonl.
///
/// Forward-compat: unknown `type` values fall through to `Unknown` so that a
/// CLI version bump that introduces a new event type doesn't break us.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Event {
    User(UserEvent),
    Assistant(AssistantEvent),
    System(SystemEvent),
    Attachment(AttachmentEvent),
    Summary(SummaryEvent),
    AiTitle(AiTitleEvent),
    CustomTitle(CustomTitleEvent),
    AgentName(AgentNameEvent),
    LastPrompt(LastPromptEvent),
    PermissionMode(PermissionModeEvent),
    FileHistorySnapshot(FileHistorySnapshotEvent),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct UserEvent {
    #[serde(default)]
    pub uuid: Option<Uuid>,
    #[serde(default, rename = "parentUuid")]
    pub parent_uuid: Option<Uuid>,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default, rename = "isSidechain")]
    pub is_sidechain: bool,
    #[serde(default, rename = "isMeta")]
    pub is_meta: Option<bool>,
    pub message: Message,
    #[serde(default, rename = "toolUseResult")]
    pub tool_use_result: Option<serde_json::Value>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, rename = "gitBranch")]
    pub git_branch: Option<String>,
    /// Claude Code provenance: the source session/message this line was forked
    /// from (also written by claude-code-manager's own copy-on-edit; see `edit::commit`).
    #[serde(default, rename = "forkedFrom")]
    pub forked_from: Option<ForkedFrom>,
    /// Set when this user line records an INTERRUPT (Esc / "for tool use"):
    /// the id of the assistant message it cut short. Its presence is the
    /// structural signal that claude stopped and the turn is the user's again —
    /// the `[Request interrupted by user]` text is just its rendering.
    #[serde(default, rename = "interruptedMessageId")]
    pub interrupted_message_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct AssistantEvent {
    #[serde(default)]
    pub uuid: Option<Uuid>,
    #[serde(default, rename = "parentUuid")]
    pub parent_uuid: Option<Uuid>,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default, rename = "isSidechain")]
    pub is_sidechain: bool,
    pub message: AssistantMessage,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, rename = "gitBranch")]
    pub git_branch: Option<String>,
    #[serde(default, rename = "forkedFrom")]
    pub forked_from: Option<ForkedFrom>,
}

/// Fork provenance recorded on a copied message line. Claude Code's `/branch` and
/// `--fork-session` write this; claude-code-manager's `commit` writes it too.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct ForkedFrom {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(default, rename = "messageUuid")]
    pub message_uuid: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct SystemEvent {
    #[serde(default)]
    pub uuid: Option<Uuid>,
    #[serde(default, rename = "parentUuid")]
    pub parent_uuid: Option<Uuid>,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct AttachmentEvent {
    #[serde(default)]
    pub uuid: Option<Uuid>,
    #[serde(default, rename = "parentUuid")]
    pub parent_uuid: Option<Uuid>,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub attachment: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct SummaryEvent {
    #[serde(default)]
    pub uuid: Option<Uuid>,
    #[serde(default, rename = "parentUuid")]
    pub parent_uuid: Option<Uuid>,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct AiTitleEvent {
    #[serde(rename = "aiTitle")]
    pub ai_title: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct CustomTitleEvent {
    #[serde(rename = "customTitle")]
    pub custom_title: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct AgentNameEvent {
    #[serde(default, rename = "agentName")]
    pub agent_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct LastPromptEvent {
    #[serde(rename = "leafUuid")]
    pub leaf_uuid: Uuid,
    /// The prompt text Claude Code caches inline for the resume picker. This is
    /// the most reliable source for the "last user message" used as a title,
    /// since it survives even when the underlying user event is edited.
    #[serde(default, rename = "lastPrompt")]
    pub last_prompt: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct PermissionModeEvent {
    #[serde(rename = "permissionMode")]
    pub mode: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct FileHistorySnapshotEvent {
    #[serde(default, rename = "messageId")]
    pub message_id: Option<Uuid>,
}

/// The `message` field of a `user` event.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub role: Option<String>,
    pub content: MessageContent,
}

/// The `message` field of an `assistant` event.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub content: Vec<Block>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// User message content is either a plain string or an array of blocks
/// (tool_result, sometimes text). We model both shapes.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<UserBlock>),
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserBlock {
    Text {
        text: String,
    },
    /// An attached/pasted image (base64 in the jsonl).
    Image {
        source: ImageSource,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Unknown,
}

/// An image block's source. CC stores attachments inline: `{type:"base64",
/// media_type:"image/png", data:"…"}`. Fields are defaulted so an unexpected
/// variant (e.g. a future `type:"url"`) parses instead of failing the line.
#[derive(Debug, Clone, serde::Serialize, Deserialize, Default)]
pub struct ImageSource {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub media_type: String,
    #[serde(default)]
    pub data: String,
}

/// Tool result content can be a string or an array of typed blocks (text/image).
#[derive(Debug, Clone, serde::Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum ToolResultContent {
    #[default]
    None,
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultBlock {
    Text { text: String },
    /// An image a tool returned (e.g. a browser screenshot).
    Image { source: ImageSource },
    #[serde(other)]
    Unknown,
}

impl ToolResultContent {
    /// Best-effort flatten into a single string for rendering.
    pub fn as_text(&self) -> String {
        match self {
            ToolResultContent::None => String::new(),
            ToolResultContent::Text(s) => s.clone(),
            ToolResultContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ToolResultBlock::Text { text } => Some(text.as_str()),
                    ToolResultBlock::Image { .. } | ToolResultBlock::Unknown => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

/// Tools whose `tool_use` parks the session on the USER rather than on claude:
/// the client renders them as a selection / approval UI and nothing advances
/// until the user acts on it.
pub const USER_BLOCKING_TOOLS: &[&str] = &["AskUserQuestion", "ExitPlanMode"];

impl Event {
    /// UUID of this event, if any.
    pub fn uuid(&self) -> Option<Uuid> {
        match self {
            Event::User(e) => e.uuid,
            Event::Assistant(e) => e.uuid,
            Event::System(e) => e.uuid,
            Event::Attachment(e) => e.uuid,
            Event::Summary(e) => e.uuid,
            _ => None,
        }
    }

    /// Parent UUID, if any.
    pub fn parent_uuid(&self) -> Option<Uuid> {
        match self {
            Event::User(e) => e.parent_uuid,
            Event::Assistant(e) => e.parent_uuid,
            Event::System(e) => e.parent_uuid,
            Event::Attachment(e) => e.parent_uuid,
            Event::Summary(e) => e.parent_uuid,
            _ => None,
        }
    }

    pub fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            Event::User(e) => e.timestamp,
            Event::Assistant(e) => e.timestamp,
            Event::System(e) => e.timestamp,
            Event::Attachment(e) => e.timestamp,
            Event::Summary(e) => e.timestamp,
            _ => None,
        }
    }

    pub fn is_sidechain(&self) -> bool {
        match self {
            Event::User(e) => e.is_sidechain,
            Event::Assistant(e) => e.is_sidechain,
            _ => false,
        }
    }

    /// True if this event is a real conversation turn — the signal for session
    /// "activity" (sidebar running/unread). A non-meta user turn, or a
    /// non-synthetic assistant turn. Excludes summaries, system lines, titles,
    /// meta turns. Sidechain (sub-agent) turns DO count — that's claude working.
    /// Whether this turn leaves the conversation waiting on the USER.
    ///
    /// True for a completed main-thread assistant turn (`end_turn`), and for one
    /// parked on a [user-blocking tool](USER_BLOCKING_TOOLS) — those stop with
    /// `tool_use`, not `end_turn`, so without this they read as claude still
    /// working: "running" until the staleness window expires, then no indicator
    /// at all, in the one state that most needs one.
    ///
    /// Sidechain turns never count — a subagent's question never reaches the UI.
    ///
    /// Single source of truth for the sidebar's running/unread dots: the cheap
    /// tail probe ([`parse::last_activity`](super::parse::last_activity)) and the
    /// full parse ([`summary`](super::summary)) both call this, so a session
    /// can't read differently depending on which one ran.
    pub fn awaits_user(&self) -> bool {
        match self {
            // An interrupt ends the turn as surely as `end_turn` does: claude
            // stopped and the user is back in control. Without this the line
            // reads as an ordinary user turn — i.e. "claude is working" — and
            // a cancelled session showed as running indefinitely (the more so
            // once hosted sessions stopped relying on the staleness backstop).
            Event::User(u) if !u.is_sidechain && u.interrupted_message_id.is_some() => true,
            Event::Assistant(a) if !a.is_sidechain => {
                a.message.stop_reason.as_deref() == Some("end_turn")
                    || a.message.content.iter().any(|b| {
                        matches!(b, Block::ToolUse { name, .. }
                            if USER_BLOCKING_TOOLS.contains(&name.as_str()))
                    })
            }
            _ => false,
        }
    }

    pub fn is_activity_turn(&self) -> bool {
        match self {
            Event::User(e) => !e.is_meta.unwrap_or(false),
            Event::Assistant(e) => e.message.model.as_deref() != Some("<synthetic>"),
            _ => false,
        }
    }

    pub fn cwd(&self) -> Option<&str> {
        match self {
            Event::User(e) => e.cwd.as_deref(),
            Event::Assistant(e) => e.cwd.as_deref(),
            _ => None,
        }
    }

    pub fn git_branch(&self) -> Option<&str> {
        match self {
            Event::User(e) => e.git_branch.as_deref(),
            Event::Assistant(e) => e.git_branch.as_deref(),
            _ => None,
        }
    }

    /// The source session id this line was forked from, if any.
    pub fn forked_from_session(&self) -> Option<&str> {
        match self {
            Event::User(e) => e.forked_from.as_ref().map(|f| f.session_id.as_str()),
            Event::Assistant(e) => e.forked_from.as_ref().map(|f| f.session_id.as_str()),
            _ => None,
        }
    }

    /// The source MESSAGE uuid this line was forked at (`forkedFrom.messageUuid`),
    /// if any. This is the branch point in the source session.
    pub fn forked_from_message(&self) -> Option<&str> {
        match self {
            Event::User(e) => e.forked_from.as_ref().and_then(|f| f.message_uuid.as_deref()),
            Event::Assistant(e) => e.forked_from.as_ref().and_then(|f| f.message_uuid.as_deref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_text() {
        let line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-000000000001","parentUuid":null,"timestamp":"2026-05-22T10:48:22.486Z","isSidechain":false,"message":{"role":"user","content":"hello"}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        match ev {
            Event::User(u) => match u.message.content {
                MessageContent::Text(t) => assert_eq!(t, "hello"),
                _ => panic!("expected text"),
            },
            _ => panic!("expected user"),
        }
    }

    #[test]
    fn parse_user_tool_result() {
        let line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-000000000002","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"abc","content":"out"}]}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        match ev {
            Event::User(u) => match u.message.content {
                MessageContent::Blocks(blocks) => {
                    assert_eq!(blocks.len(), 1);
                    match &blocks[0] {
                        UserBlock::ToolResult { tool_use_id, content, .. } => {
                            assert_eq!(tool_use_id, "abc");
                            assert_eq!(content.as_text(), "out");
                        }
                        _ => panic!("expected tool_result"),
                    }
                }
                _ => panic!("expected blocks"),
            },
            _ => panic!("expected user"),
        }
    }

    #[test]
    fn parse_assistant_thinking_then_text() {
        let line = r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-000000000003","message":{"model":"claude-opus-4-7","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"hi"}],"usage":{"input_tokens":1,"output_tokens":2}}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        match ev {
            Event::Assistant(a) => {
                assert_eq!(a.message.model.as_deref(), Some("claude-opus-4-7"));
                assert_eq!(a.message.content.len(), 2);
                match &a.message.content[0] {
                    Block::Thinking { thinking } => assert_eq!(thinking, "hmm"),
                    _ => panic!("expected thinking"),
                }
                match &a.message.content[1] {
                    Block::Text { text } => assert_eq!(text, "hi"),
                    _ => panic!("expected text"),
                }
            }
            _ => panic!("expected assistant"),
        }
    }

    #[test]
    fn parse_ai_title() {
        let line = r#"{"type":"ai-title","aiTitle":"my title","sessionId":"x"}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        match ev {
            Event::AiTitle(t) => assert_eq!(t.ai_title, "my title"),
            _ => panic!("expected ai-title"),
        }
    }

    #[test]
    fn parse_unknown_type_falls_through() {
        let line = r#"{"type":"some-future-type","x":1}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert!(matches!(ev, Event::Unknown));
    }

    #[test]
    fn an_interrupted_user_line_awaits_the_user() {
        // Real shape: Esc mid-turn writes a USER line carrying
        // `interruptedMessageId`. Without it this reads as an ordinary user
        // turn (claude working) and a cancelled session shows as running.
        let line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000e1","parentUuid":"00000000-0000-0000-0000-0000000000e0","timestamp":"2026-08-20T23:28:34.090Z","isSidechain":false,"interruptedMessageId":"msg_x","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert!(ev.awaits_user(), "an interrupt hands the turn back to the user");
        assert!(ev.is_activity_turn(), "and it is still a real turn");

        // An ordinary prompt does NOT await the user — claude is now working.
        let line = r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000e2","timestamp":"2026-08-20T23:28:35.090Z","isSidechain":false,"message":{"role":"user","content":"do the thing"}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert!(!ev.awaits_user());
    }

    #[test]
    fn parse_forked_from_provenance() {
        let line = r#"{"type":"user","sessionId":"new","forkedFrom":{"sessionId":"src-session","messageUuid":"m1"},"message":{"role":"user","content":"hi"}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert_eq!(ev.forked_from_session(), Some("src-session"));

        // Absent forkedFrom -> None, and a line without it still parses.
        let plain = r#"{"type":"user","message":{"role":"user","content":"hi"}}"#;
        let ev2: Event = serde_json::from_str(plain).unwrap();
        assert_eq!(ev2.forked_from_session(), None);
    }
}
