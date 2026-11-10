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

/// A `!` bash-mode turn: either the command the user ran, or its output.
/// Claude Code writes these as ordinary user messages wrapping the text in
/// `<bash-input>` / `<bash-stdout>` + `<bash-stderr>` tags.
#[derive(Debug, Clone, PartialEq)]
pub enum BashBlock {
    Input(String),
    Output { stdout: String, stderr: String },
}

/// Claude Code HTML-escapes the text inside these blocks, so a URL full of
/// query parameters arrives as `a&amp;b`. Undo the entities it uses, or every
/// such line reads as markup.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        // Last: an escaped ampersand can be part of any of the above.
        .replace("&amp;", "&")
}

/// Parse a `!` bash-mode message. `None` if `text` isn't one.
pub fn bash_block(text: &str) -> Option<BashBlock> {
    if let Some(cmd) = extract_tag(text, "bash-input") {
        return Some(BashBlock::Input(unescape(cmd.trim())));
    }
    let stdout = extract_tag(text, "bash-stdout");
    let stderr = extract_tag(text, "bash-stderr");
    if stdout.is_none() && stderr.is_none() {
        return None;
    }
    Some(BashBlock::Output {
        stdout: unescape(stdout.unwrap_or("").trim_end()),
        stderr: unescape(stderr.unwrap_or("").trim_end()),
    })
}

/// Terminal output as the transcript shows it, unstyled: escapes dropped, a `\r`-redrawn line
/// reduced to its last redraw. Mirrors the frontend's `ansiText` so search matches what is drawn.
pub fn strip_ansi(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains(['\x1b', '\r']) {
        return std::borrow::Cow::Borrowed(s);
    }
    let b = s.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(n);
    let mut line_start = 0;
    let mut pending_cr = false;
    let mut i = 0;
    while i < n {
        match b[i] {
            0x1b => i = escape_end(b, i),
            b'\r' => {
                pending_cr = true;
                i += 1;
            }
            b'\n' => {
                pending_cr = false;
                out.push('\n');
                line_start = out.len();
                i += 1;
            }
            _ => {
                if pending_cr {
                    out.truncate(line_start);
                    pending_cr = false;
                }
                // Copy up to the next control byte we act on; all are ASCII, so this stays on char boundaries.
                let j = b[i..]
                    .iter()
                    .position(|&c| matches!(c, 0x1b | b'\r' | b'\n'))
                    .map_or(n, |k| i + k);
                out.push_str(&s[i..j]);
                i = j;
            }
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Index just past the escape sequence starting at `b[i]` (an ESC).
fn escape_end(b: &[u8], i: usize) -> usize {
    let n = b.len();
    match b.get(i + 1) {
        None => n,
        Some(b'[') => {
            let mut j = i + 2;
            while j < n && (0x20..=0x3f).contains(&b[j]) {
                j += 1;
            }
            match b.get(j) {
                None => n,
                Some(c) if (0x40..=0x7e).contains(c) => j + 1,
                Some(_) => j,
            }
        }
        Some(b']') => {
            let mut j = i + 2;
            while j < n {
                if b[j] == 0x07 {
                    return j + 1;
                }
                if b[j] == 0x1b && b.get(j + 1) == Some(&b'\\') {
                    return j + 2;
                }
                j += 1;
            }
            n
        }
        Some(c) if (0x20..=0x2f).contains(c) => {
            let mut j = i + 1;
            while j < n && (0x20..=0x2f).contains(&b[j]) {
                j += 1;
            }
            match b.get(j) {
                Some(c) if c.is_ascii() => j + 1,
                _ => j,
            }
        }
        Some(c) if (0x30..=0x7e).contains(c) => i + 2,
        Some(_) => i + 1,
    }
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

/// One line naming what a tool call did, for collapsed tool headers and search previews.
pub fn tool_input_summary(name: &str, input: &serde_json::Value) -> String {
    // The first entry of a list-shaped input is what a reader would recognise the call by.
    let first_of = |list: &str, field: &str| {
        input.get(list)?.as_array()?.first()?.get(field)?.as_str().map(str::to_string)
    };
    let listed = match name {
        "AskUserQuestion" => first_of("questions", "question"),
        "TodoWrite" => first_of("todos", "content"),
        _ => None,
    };
    if let Some(s) = listed {
        return truncate(&s, 120);
    }
    let keys: &[&str] = match name {
        "Bash" => &["command", "description"],
        "Read" | "Write" | "Edit" | "MultiEdit" => &["file_path"],
        "NotebookEdit" => &["notebook_path"],
        "Grep" | "Glob" => &["pattern"],
        "WebFetch" => &["url"],
        "WebSearch" => &["query"],
        "Agent" | "Task" => &["description", "prompt"],
        _ => &["query", "file_path", "path", "command", "description", "prompt"],
    };
    for k in keys {
        if let Some(v) = input.get(*k) {
            if let Some(s) = v.as_str() {
                // A path's end names the file; its start is the part a reader can spare.
                let is_path = matches!(*k, "file_path" | "notebook_path" | "path");
                return if is_path { truncate_front(s, 120) } else { truncate(s, 120) };
            }
        }
    }
    truncate(&input.to_string(), 120)
}

fn truncate_front(s: &str, max: usize) -> String {
    let one_line: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let n = one_line.chars().count();
    if n <= max {
        one_line
    } else {
        let tail: String = one_line.chars().skip(n - (max - 1)).collect();
        format!("…{tail}")
    }
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
mod tests {

    #[test]
    fn bash_block_reads_input_and_output() {
        assert_eq!(
            bash_block("<bash-input> ls -la</bash-input>"),
            Some(BashBlock::Input("ls -la".into()))
        );
        assert_eq!(
            bash_block("<bash-stdout>one\ntwo\n</bash-stdout><bash-stderr>bad</bash-stderr>"),
            Some(BashBlock::Output { stdout: "one\ntwo".into(), stderr: "bad".into() })
        );
    }

    #[test]
    fn bash_block_undoes_the_html_escaping() {
        // A URL's query string is the common case: `&` arrives as `&amp;`.
        assert_eq!(
            bash_block("<bash-stdout>x?a=1&amp;b=2 &lt;tag&gt; &quot;q&quot;</bash-stdout>"),
            Some(BashBlock::Output {
                stdout: "x?a=1&b=2 <tag> \"q\"".into(),
                stderr: String::new(),
            })
        );
    }

    #[test]
    fn strip_ansi_keeps_what_the_transcript_shows() {
        // Same vectors as the frontend's ansi.test.ts, so search agrees with the renderer.
        let vitest = "\x1b[2m Test Files \x1b[22m \x1b[1m\x1b[32m9 passed\x1b[39m\x1b[22m\x1b[90m (9)\x1b[39m";
        assert_eq!(strip_ansi(vitest), " Test Files  9 passed (9)");
        assert!(matches!(strip_ansi("plain\ntext"), std::borrow::Cow::Borrowed(_)));
        assert_eq!(strip_ansi("a\x1b[2Kb\x1b[1Ac\x1b[Gd\x1b(Be\x1b7f\x1b8g\x1b[?25lh"), "abcdefgh");
        assert_eq!(strip_ansi("see \x1b]8;;https://x\x07label\x1b]8;;\x07 here"), "see label here");
        assert_eq!(strip_ansi("st \x1b]8;;https://x\x1b\\label\x1b]8;;\x1b\\"), "st label");
        assert_eq!(strip_ansi("tail\x1b"), "tail");
        assert_eq!(strip_ansi("cut\x1b[3"), "cut");
        assert_eq!(strip_ansi("a\r\nb"), "a\nb");
        assert_eq!(strip_ansi("10%\r50%\r100%\ndone"), "100%\ndone");
        assert_eq!(strip_ansi("abc\r"), "abc");
        assert_eq!(strip_ansi("abc\r\r\ndef"), "abc\ndef");
        assert_eq!(strip_ansi("12345\rab"), "ab");
        assert_eq!(strip_ansi("x\n\x1b[32m10%\r\x1b[1m100%\x1b[0m"), "x\n100%");
        assert_eq!(strip_ansi("\x1b(é ok"), "é ok");
    }

    #[test]
    fn bash_block_ignores_other_text() {
        assert_eq!(bash_block("just a message"), None);
        assert_eq!(bash_block("<command-name>/x</command-name>"), None);
    }

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

    #[test]
    fn tool_input_summary_names_what_the_call_did() {
        use serde_json::json;
        assert_eq!(tool_input_summary("Bash", &json!({"command": "git status", "description": "Show status"})), "git status");
        assert_eq!(tool_input_summary("Edit", &json!({"file_path": "src/a.ts", "old_string": "x"})), "src/a.ts");
        assert_eq!(tool_input_summary("Grep", &json!({"pattern": "TODO", "path": "src"})), "TODO");
        assert_eq!(tool_input_summary("Glob", &json!({"pattern": "**/*.rs"})), "**/*.rs");
        assert_eq!(tool_input_summary("WebFetch", &json!({"url": "https://x.dev", "prompt": "p"})), "https://x.dev");
        assert_eq!(tool_input_summary("Agent", &json!({"description": "Find callers", "prompt": "long"})), "Find callers");
        assert_eq!(
            tool_input_summary("TodoWrite", &json!({"todos": [{"content": "Write tests", "status": "pending"}]})),
            "Write tests"
        );
    }

    #[test]
    fn tool_input_summary_shows_an_askuserquestion_question_not_its_json() {
        let input = serde_json::json!({"questions": [
            {"question": "Which plan should new orgs start on?", "header": "Plan", "options": []},
            {"question": "Second?", "header": "B", "options": []}
        ]});
        assert_eq!(tool_input_summary("AskUserQuestion", &input), "Which plan should new orgs start on?");
    }

    #[test]
    fn a_long_path_keeps_its_file_name() {
        let deep = format!("/Users/me/{}/deploy-panel-v2/components/OfferCard.tsx", "nested/".repeat(20));
        let s = tool_input_summary("Edit", &serde_json::json!({ "file_path": deep }));
        assert_eq!(s.chars().count(), 120, "capped");
        assert!(s.starts_with('…'), "cut from the front, where the least telling part is");
        assert!(s.ends_with("/components/OfferCard.tsx"), "the file name survives: {s}");
    }

    #[test]
    fn tool_input_summary_falls_back_to_common_keys_then_json_on_one_capped_line() {
        use serde_json::json;
        assert_eq!(tool_input_summary("mcp__srv__search", &json!({"query": "needle"})), "needle");
        assert_eq!(tool_input_summary("Mystery", &json!({"n": 1})), r#"{"n":1}"#);
        let long = tool_input_summary("Bash", &json!({"command": format!("echo {}\nnext", "a".repeat(200))}));
        assert_eq!(long.chars().count(), 120, "capped");
        assert!(long.ends_with('…') && !long.contains('\n'), "one line, marked as cut");
    }
}
