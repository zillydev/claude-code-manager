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
                    ToolResultBlock::Unknown => None,
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
}
