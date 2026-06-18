//! Cleaning of injected "noise" user messages for display: slash-command echoes
//! and background task-notifications. Extract the signal, drop the boilerplate.

/// Inner text of the first `<tag>…</tag>` in `s`, if present.
pub(crate) fn extract_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = s.find(&open)? + open.len();
    let j = s[i..].find(&close)? + i;
    Some(&s[i..j])
}

/// A slash-command echo (`<command-name>/x</command-name> <command-args>…`) as
/// `/x args`. `None` if `text` is not a command echo.
pub fn command_label(text: &str) -> Option<String> {
    if !text.contains("<command-name>") {
        return None;
    }
    let name = extract_tag(text, "command-name").map(str::trim)?;
    let args = extract_tag(text, "command-args").map(str::trim);
    Some(match args {
        Some(a) if !a.is_empty() => format!("{} {}", name, a),
        _ => name.to_string(),
    })
}

const SKILL_PROMPT_PREFIX: &str = "Base directory for this skill:";

/// If `text` is an injected skill prompt (an `isMeta` user message whose first
/// line is `Base directory for this skill: <path>`), return the skill's folder
/// name (the last path component of that line). `None` otherwise.
pub fn skill_prompt_name(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    let path = first.strip_prefix(SKILL_PROMPT_PREFIX)?.trim();
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    Some(if name.is_empty() { "skill".to_string() } else { name.to_string() })
}

/// Parsed background task-notification (signal only).
pub struct TaskNote {
    pub status: String,
    pub summary: String,
    pub result: String,
}

/// Parse a `<task-notification>…</task-notification>` message. `None` if not one.
pub fn parse_task_notification(text: &str) -> Option<TaskNote> {
    if !text.trim_start().starts_with("<task-notification>") {
        return None;
    }
    let tag = |t: &str| extract_tag(text, t).unwrap_or("").trim().to_string();
    Some(TaskNote { status: tag("status"), summary: tag("summary"), result: tag("result") })
}

/// Compact one-line form for previews: command echo → `/cmd args`;
/// task-notification → `task · <status> · <summary>`; else `None` (keep original).
pub fn one_line(text: &str) -> Option<String> {
    if let Some(cmd) = command_label(text) {
        return Some(cmd);
    }
    if let Some(name) = skill_prompt_name(text) {
        return Some(format!("skill: {}", name));
    }
    parse_task_notification(text).map(|tn| format!("task · {} · {}", tn.status, tn.summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_label_extracts_name_and_args() {
        let echo = "<command-name>/compact</command-name>\n  <command-message>compact</command-message>\n  <command-args>foo bar</command-args>";
        assert_eq!(command_label(echo).as_deref(), Some("/compact foo bar"));
        let no_args = "<command-name>/resume</command-name>\n<command-args></command-args>";
        assert_eq!(command_label(no_args).as_deref(), Some("/resume"));
        assert_eq!(command_label("just a normal message"), None);
    }

    #[test]
    fn parse_task_notification_extracts_signal() {
        let tn = "<task-notification>\n<task-id>abc</task-id>\n<tool-use-id>t</tool-use-id>\n<output-file>/tmp/x</output-file>\n<status>completed</status>\n<summary>Agent did a thing</summary>\n<result>the full result body</result>\n</task-notification>";
        let n = parse_task_notification(tn).expect("parses");
        assert_eq!(n.status, "completed");
        assert_eq!(n.summary, "Agent did a thing");
        assert_eq!(n.result, "the full result body");
        assert!(parse_task_notification("hello").is_none());
    }

    #[test]
    fn skill_prompt_name_extracts_folder() {
        let t = "Base directory for this skill: /Users/x/.claude/plugins/superpowers/6.0.2/skills/brainstorming\n\n# Brainstorming\nbody";
        assert_eq!(skill_prompt_name(t).as_deref(), Some("brainstorming"));
        assert_eq!(skill_prompt_name("a normal user message"), None);
    }

    #[test]
    fn one_line_compacts_skill_prompt() {
        let t = "Base directory for this skill: /a/b/skills/writing-plans\n# Writing Plans";
        assert_eq!(one_line(t).as_deref(), Some("skill: writing-plans"));
    }

    #[test]
    fn one_line_compacts_both_kinds() {
        assert_eq!(one_line("<command-name>/compact</command-name>").as_deref(), Some("/compact"));
        let tn = "<task-notification>\n<status>completed</status>\n<summary>did X</summary>\n<result>r</result>\n</task-notification>";
        assert_eq!(one_line(tn).as_deref(), Some("task · completed · did X"));
        assert_eq!(one_line("a normal message"), None);
    }
}
