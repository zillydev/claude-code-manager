//! Session "surgery": summarize or prune a range of messages on the active path,
//! producing a new flattened session via `edit::commit` (copy-on-edit).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde_json::Value;
use uuid::Uuid;

use super::edit::{commit, CommitOpts, CommitResult};
use super::raw::{active_path, first_cwd, read_raw, segments, RawLine};
use crate::store::paths;

fn is_message(l: &RawLine) -> bool {
    l.kind == "user" || l.kind == "assistant"
}

/// A user line is a "real" message unless its content is an array consisting
/// entirely of `tool_result` blocks — those belong to the preceding assistant
/// turn, not a user prompt.
fn is_real_user(line: &RawLine) -> bool {
    if line.kind != "user" {
        return false;
    }
    match line.value.pointer("/message/content") {
        Some(Value::Array(blocks)) => {
            let all_tool_results = !blocks.is_empty()
                && blocks
                    .iter()
                    .all(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_result"));
            !all_tool_results
        }
        _ => true, // string content or missing -> a real user message
    }
}

/// Partition the active path into inclusive `(start, end)` turn spans. A real
/// user message is its own single-line turn; an assistant turn begins with an
/// assistant line and extends until the next real user message, absorbing all
/// interleaved tool_result lines and consecutive assistant lines. This mirrors
/// the webui renderer, which merges consecutive assistant events across the
/// suppressed tool_result lines into one displayed turn.
fn turn_spans(active: &[&RawLine]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let n = active.len();
    let mut i = 0;
    while i < n {
        let start = i;
        if is_real_user(active[i]) {
            i += 1;
        } else {
            // Assistant turn: absorb following tool_result lines AND consecutive
            // assistant lines until the next real user message. Mirrors the
            // webui renderer, which merges consecutive assistant events across
            // the suppressed tool_result line into one turn.
            i += 1;
            while i < n && !is_real_user(active[i]) {
                i += 1;
            }
        }
        spans.push((start, i - 1));
    }
    spans
}

/// Resolve two message uuids to a turn-snapped inclusive index range
/// `[a_start, b_end]` on the active path. `a_start` snaps to the first line of
/// the `from` turn; `b_end` snaps to the last line of the `to` turn (including
/// trailing tool_result lines) so no turn is ever half-cut.
pub fn snap_range(active: &[&RawLine], from: Uuid, to: Uuid) -> Result<(usize, usize)> {
    let from_idx = active
        .iter()
        .position(|l| l.uuid == Some(from))
        .ok_or_else(|| anyhow!("from id {} is not on the active path", from))?;
    let to_idx = active
        .iter()
        .position(|l| l.uuid == Some(to))
        .ok_or_else(|| anyhow!("to id {} is not on the active path", to))?;
    let spans = turn_spans(active);
    let a_start = spans
        .iter()
        .find(|(s, e)| from_idx >= *s && from_idx <= *e)
        .map(|(s, _)| *s)
        .expect("from_idx is within some span");
    let b_end = spans
        .iter()
        .find(|(s, e)| to_idx >= *s && to_idx <= *e)
        .map(|(_, e)| *e)
        .expect("to_idx is within some span");
    if a_start > b_end {
        return Err(anyhow!("from id occurs after to id on the active path"));
    }
    Ok((a_start, b_end))
}

const TOOL_TEXT_CAP: usize = 4000;

fn cap(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let head: String = chars[..max].iter().collect();
        format!("{}…[{} more chars]", head, chars.len() - max)
    }
}

fn tool_result_text(block: &Value) -> String {
    match block.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                    b.get("text").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Render the inclusive line range `[a_start, b_end]` of the active path as
/// readable text for summarization. Tool inputs/results are capped to keep the
/// orchestrating session's context bounded.
/// Render a message's plain text, eliding an injected skill prompt (the full
/// SKILL.md dumped after `Base directory for this skill:`) to a one-liner so a
/// drilled span isn't drowned by static plumbing.
fn render_message_text(t: &str) -> String {
    match super::display::skill_prompt_name(t) {
        Some(name) => format!("[skill prompt elided: {}, {} chars]", name, t.chars().count()),
        None => t.to_string(),
    }
}

fn render_range_text(active: &[&RawLine], a_start: usize, b_end: usize) -> String {
    let mut out = String::new();
    for l in &active[a_start..=b_end] {
        let role = l
            .value
            .pointer("/message/role")
            .and_then(|v| v.as_str())
            .unwrap_or(l.kind.as_str());
        let uuid = l.uuid.map(|u| u.to_string()).unwrap_or_default();
        out.push_str(&format!("[{} {}]\n", role, uuid));
        match l.value.pointer("/message/content") {
            Some(Value::String(s)) => {
                out.push_str(&render_message_text(s));
                out.push('\n');
            }
            Some(Value::Array(blocks)) => {
                for b in blocks {
                    match b.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                out.push_str(&render_message_text(t));
                                out.push('\n');
                            }
                        }
                        Some("tool_use") => {
                            let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                            let input = b.get("input").map(|v| v.to_string()).unwrap_or_default();
                            out.push_str(&format!("▸ {}: {}\n", name, cap(&input, TOOL_TEXT_CAP)));
                        }
                        Some("tool_result") => {
                            out.push_str(&format!("→ result: {}\n", cap(&tool_result_text(b), TOOL_TEXT_CAP)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        out.push('\n');
    }
    out
}

/// Truncate to `max` chars, appending a bare `…` if cut (no char-count noise).
fn ellipsis(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let head: String = chars[..max].iter().collect();
        format!("{}…", head)
    }
}

/// A tool_use block rendered as its NAME + salient argument (not raw JSON):
/// the file for Read/Edit/Write, the command for Bash, the pattern for Grep, the
/// description for Task/Agent, etc. Falls back to the bare name.
fn tool_salient(block: &Value) -> String {
    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
    let input = block.get("input");
    let arg = |key: &str| input.and_then(|i| i.get(key)).and_then(|v| v.as_str());
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    match name {
        "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => arg("file_path")
            .or_else(|| arg("notebook_path"))
            .map(|p| format!("{} {}", name, base(p)))
            .unwrap_or_else(|| name.to_string()),
        "Bash" => arg("command")
            .map(|c| format!("{} `{}`", name, ellipsis(&c.replace('\n', " "), 50)))
            .unwrap_or_else(|| name.to_string()),
        "Grep" | "Glob" => arg("pattern")
            .map(|p| format!("{} {}", name, ellipsis(p, 40)))
            .unwrap_or_else(|| name.to_string()),
        "Task" | "Agent" => arg("description")
            .map(|d| format!("{} {}", name, ellipsis(d, 40)))
            .unwrap_or_else(|| name.to_string()),
        "Skill" => arg("skill")
            .or_else(|| arg("command"))
            .map(|s| format!("{} {}", name, ellipsis(s, 40)))
            .unwrap_or_else(|| name.to_string()),
        "ToolSearch" => arg("query")
            .map(|q| format!("{} {}", name, ellipsis(q, 40)))
            .unwrap_or_else(|| name.to_string()),
        _ => name.to_string(),
    }
}

const OUTLINE_PREVIEW_CAP: usize = 100;
const OUTLINE_TOOL_CAP: usize = 6;

/// Render a compact MAP of the inclusive range for navigating a whole session:
/// per turn, a header (turn index, role, message uuid), an indented text
/// preview, and — for assistant turns — an indented line of tool-call salient
/// args (`▸Read foo.rs · Bash `cargo build` · +N more`). Tool RESULTS and full
/// prose are omitted; this is the "table of contents", not the content.
fn render_outline(active: &[&RawLine], a_start: usize, b_end: usize) -> String {
    let mut out = String::new();
    // `#N` is a display ordinal over the turns actually shown (skipped machinery
    // turns don't leave gaps); targeting is by uuid, not this number.
    let mut shown = 0usize;
    for (s, e) in turn_spans(active).iter() {
        if *s < a_start || *e > b_end {
            continue; // only turns fully within the range
        }
        let head = active[*s];
        let role = head
            .value
            .pointer("/message/role")
            .and_then(|v| v.as_str())
            .unwrap_or(head.kind.as_str());
        let short = head.uuid.map(short8).unwrap_or_default();
        let is_summary =
            head.value.get("isCompactSummary").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut preview = String::new();
        let mut tools: Vec<String> = Vec::new();
        for l in &active[*s..=*e] {
            match l.value.pointer("/message/content") {
                Some(Value::String(t)) => {
                    if preview.is_empty() {
                        preview = t.clone();
                    }
                }
                Some(Value::Array(blocks)) => {
                    for b in blocks {
                        match b.get("type").and_then(|v| v.as_str()) {
                            Some("text") => {
                                if preview.is_empty() {
                                    if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                        preview = t.to_string();
                                    }
                                }
                            }
                            Some("tool_use") => tools.push(tool_salient(b)),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        // Skip slash-command machinery turns (caveat + stdout) — pure noise on the
        // map; the command echo itself still shows (cleaned to `/cmd`). The webui
        // hides these too. (Command echoes start with `<command-`, not
        // `<local-command-`, so they are NOT skipped here.)
        if preview.trim_start().starts_with("<local-command-") {
            continue;
        }
        // Skip empty machinery turns — compact_boundary system markers (the
        // segment divider already marks them), attachments, and other lines with
        // no message text and no tool calls. Bare headers add only noise.
        if tools.is_empty() && preview.trim().is_empty() {
            continue;
        }
        let tag = if is_summary { " · compaction summary" } else { "" };
        out.push_str(&format!("#{} {} {}{}\n", shown, role, short, tag));
        shown += 1;
        let cleaned =
            super::display::one_line(&preview).unwrap_or_else(|| preview.replace('\n', " "));
        let prev = ellipsis(cleaned.trim(), OUTLINE_PREVIEW_CAP);
        if !prev.is_empty() {
            out.push_str(&format!("     {}\n", prev));
        }
        if !tools.is_empty() {
            let line = if tools.len() > OUTLINE_TOOL_CAP {
                let rest = tools.len() - OUTLINE_TOOL_CAP;
                format!("{} · +{} more", tools[..OUTLINE_TOOL_CAP].join(" · "), rest)
            } else {
                tools.join(" · ")
            };
            out.push_str(&format!("     ▸{}\n", line));
        }
        out.push('\n');
    }
    out
}

fn render_seg(seg: &[&RawLine], a: usize, b: usize, outline: bool) -> String {
    if outline {
        render_outline(seg, a, b)
    } else {
        render_range_text(seg, a, b)
    }
}

/// Resolve a (possibly absent) range against a single path. Both absent = the
/// whole path; both present = turn-snapped; one present = error.
fn resolve_range(path: &[&RawLine], from: Option<Uuid>, to: Option<Uuid>) -> Result<(usize, usize)> {
    match (from, to) {
        (Some(f), Some(t)) => snap_range(path, f, t),
        (None, None) => {
            if path.is_empty() {
                return Err(anyhow!("session has no messages on the active path"));
            }
            Ok((0, path.len() - 1))
        }
        _ => Err(anyhow!("provide both --from and --to, or neither")),
    }
}

/// The 8-char short id (first hex group) of a uuid — what the outline shows and
/// what `--from/--to` prefixes are typically copied from.
fn short8(u: Uuid) -> String {
    u.to_string()[..8].to_string()
}

/// Resolve a uuid prefix (case-insensitive) to the single matching message uuid
/// in `lines`. A full 36-char uuid is a prefix of itself. Errors if the prefix
/// matches none or more than one message.
fn resolve_id(lines: &[RawLine], prefix: &str) -> Result<Uuid> {
    let p = prefix.to_ascii_lowercase();
    let mut hits: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    for l in lines {
        if let Some(u) = l.uuid {
            if u.to_string().starts_with(&p) {
                hits.insert(u);
            }
        }
    }
    match hits.len() {
        1 => Ok(*hits.iter().next().unwrap()),
        0 => Err(anyhow!("no message id matches '{}'", prefix)),
        n => Err(anyhow!(
            "ambiguous id prefix '{}' matches {} messages — use more characters",
            prefix,
            n
        )),
    }
}

/// Read a session and render the full text or a compact outline. `from`/`to` are
/// uuid PREFIXES (resolved to a unique message; a full uuid works too). With
/// `all_segments`, covers every conversation tree, dividing whole-session output
/// by segment; otherwise the active path only. Both `from`/`to` = turn-snapped
/// range; neither = whole path/session; one = error.
pub fn extract(
    path: &Path,
    from: Option<&str>,
    to: Option<&str>,
    outline: bool,
    all_segments: bool,
) -> Result<String> {
    let lines = read_raw(path)?;
    let from_id = from.map(|p| resolve_id(&lines, p)).transpose()?;
    let to_id = to.map(|p| resolve_id(&lines, p)).transpose()?;

    if !all_segments {
        let active = active_path(&lines);
        // A resolved id that exists in the file but not on the active path is in
        // an earlier (pre-compaction) tree — point the caller at --all-segments.
        for (label, id) in [("from", from_id), ("to", to_id)] {
            if let Some(u) = id {
                if !active.iter().any(|l| l.uuid == Some(u)) {
                    return Err(anyhow!(
                        "{} id {} is not on the active path — it's in an earlier (pre-compaction) tree; pass --all-segments to reach it",
                        label,
                        u
                    ));
                }
            }
        }
        let (a, b) = resolve_range(&active, from_id, to_id)?;
        return Ok(render_seg(&active, a, b, outline));
    }

    let segs = segments(&lines);
    if segs.is_empty() {
        return Err(anyhow!("session has no messages"));
    }
    match (from_id, to_id) {
        (Some(f), Some(t)) => {
            let seg_of = |u: Uuid| segs.iter().position(|s| s.iter().any(|l| l.uuid == Some(u)));
            let fi = seg_of(f).ok_or_else(|| anyhow!("from id {} is not in any conversation tree", f))?;
            let ti = seg_of(t).ok_or_else(|| anyhow!("to id {} is not in any conversation tree", t))?;
            if fi != ti {
                let n = segs.len();
                return Err(anyhow!(
                    "from {} is in segment {}/{}, to {} is in segment {}/{} — a range can't cross a compaction boundary",
                    short8(f), fi + 1, n, short8(t), ti + 1, n
                ));
            }
            let (a, b) = snap_range(&segs[fi], f, t)?;
            Ok(render_seg(&segs[fi], a, b, outline))
        }
        (None, None) => {
            let n = segs.len();
            let mut out = String::new();
            for (k, seg) in segs.iter().enumerate() {
                let kind = if k + 1 == n { "active" } else { "pre-compaction" };
                out.push_str(&format!("── segment {}/{} · {} ──\n", k + 1, n, kind));
                let end = seg.len().saturating_sub(1);
                out.push_str(&render_seg(seg, 0, end, outline));
            }
            Ok(out)
        }
        _ => Err(anyhow!("provide both --from and --to, or neither")),
    }
}

/// Resolve a session reference (a full file path, or a bare session id) to its
/// jsonl path under `<root>/projects/**`.
pub fn resolve_session_path(root: &Path, session: &str) -> Result<PathBuf> {
    let direct = PathBuf::from(session);
    if direct.is_file() {
        return Ok(direct);
    }
    let file_name = format!("{}.jsonl", session);
    let projects = paths::projects_dir(root);
    for entry in std::fs::read_dir(&projects)?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let candidate = dir.join(&file_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(anyhow!(
        "session '{}' not found: not an existing path, and no {} under {}",
        session,
        file_name,
        projects.display()
    ))
}

/// Which surgery to perform on the range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RewriteMode {
    /// Delete the range entirely.
    Prune,
    /// Replace the range with a single summary message.
    Summary,
}

/// Result of a rewrite: the committed new session plus before/after sizing.
#[derive(Debug, Clone)]
pub struct RewriteOutcome {
    pub commit: CommitResult,
    pub removed_bytes: usize,
    pub messages_before: usize,
    pub messages_after: usize,
}

/// Rewrite a session by pruning or summarizing the turn-snapped range
/// `[from, to]`. Always copy-on-edit: writes a new flattened session via
/// `edit::commit`; the source file is read-only.
pub fn rewrite(
    root: &Path,
    path: &Path,
    from: &str,
    to: &str,
    mode: RewriteMode,
    summary: Option<&str>,
) -> Result<RewriteOutcome> {
    let lines = read_raw(path)?;
    let from_id = resolve_id(&lines, from)?;
    let to_id = resolve_id(&lines, to)?;
    let active = active_path(&lines);
    for (label, id) in [("from", from_id), ("to", to_id)] {
        if !active.iter().any(|l| l.uuid == Some(id)) {
            return Err(anyhow!(
                "{} id {} is not on the active path — surgery rewrites the active conversation only",
                label,
                id
            ));
        }
    }
    let (a, b) = snap_range(&active, from_id, to_id)?;

    let messages_before = active.iter().filter(|l| is_message(l)).count();
    let removed_bytes: usize = active[a..=b].iter().map(|l| l.value.to_string().len()).sum();

    // Inherit the source session's LAUNCH cwd (first cwd in file order) so the
    // new session lands in the same project dir and stays resumable. Using the
    // active path's first cwd is wrong for a compacted session: the active path
    // begins post-compaction, often in a directory the agent had cd'd into, so
    // the new session would be filed under the wrong project. Falls back to "."
    // only for a session with no cwd anywhere (e.g. a metadata-only stub).
    let cwd = first_cwd(&lines).unwrap_or_else(|| ".".to_string());
    let git_branch = active
        .iter()
        .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let opts = CommitOpts { target_cwd: cwd, git_branch, custom_title: None };

    // The summary line is owned and must outlive the &RawLine refs vec below.
    let summary_line: Option<RawLine> = match mode {
        RewriteMode::Summary => {
            let text = summary.ok_or_else(|| anyhow!("summary mode requires summary text"))?;
            let marked = format!(
                "[Earlier session context — summarized to save tokens. Treat as established history.]\n\n{}",
                text
            );
            let v = serde_json::json!({
                "type": "user",
                "message": { "role": "user", "content": marked }
            });
            Some(RawLine::from_value(v))
        }
        RewriteMode::Prune => None,
    };

    let mut path_refs: Vec<&RawLine> = Vec::new();
    path_refs.extend(active[..a].iter().copied());
    if let Some(sl) = &summary_line {
        path_refs.push(sl);
    }
    path_refs.extend(active[b + 1..].iter().copied());

    if path_refs.iter().filter(|l| is_message(l)).count() == 0 {
        return Err(anyhow!("refusing: the result would contain no messages"));
    }

    // A conversation must start with a user message. Pruning from the head can
    // leave a leading assistant message (parentUuid becomes null), which Claude
    // Code may reject on resume. Refuse with a clear, recoverable message.
    if let Some(first) = path_refs.iter().find(|l| is_message(l)) {
        if first.kind == "assistant" {
            return Err(anyhow!(
                "refusing: the result would start with an assistant message — include the first user turn in the kept range, or use summary mode (which inserts a user message)"
            ));
        }
    }

    let commit_result = commit(root, &path_refs, &opts)?;

    let result_lines = read_raw(Path::new(&commit_result.new_path))?;
    let messages_after = result_lines
        .iter()
        .filter(|l| l.kind == "user" || l.kind == "assistant")
        .count();

    Ok(RewriteOutcome {
        commit: commit_result,
        removed_bytes,
        messages_before,
        messages_after,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use super::super::edit::validate;
    use super::super::raw::{active_path, read_raw};

    fn write_tmp(name: &str, body: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let seq = CTR.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("cm-surgery-{}-{}-{}.jsonl", name, std::process::id(), seq));
        std::fs::write(&p, body).unwrap();
        p
    }

    // user A -> assistant B (tool_use T1) -> user(tool_result T1) -> assistant C(text) -> user D
    fn fixture() -> PathBuf {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"A"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"assistant","content":[{"type":"tool_use","id":"T1","name":"Bash","input":{}}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b3","parentUuid":"00000000-0000-0000-0000-0000000000b2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"T1","content":"out"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000c4","parentUuid":"00000000-0000-0000-0000-0000000000b3","message":{"role":"assistant","content":[{"type":"text","text":"C"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000d5","parentUuid":"00000000-0000-0000-0000-0000000000c4","message":{"role":"user","content":"D"}}"#, "\n"
        );
        write_tmp("fix", body)
    }

    #[test]
    fn snap_expands_assistant_turn_to_include_tool_result() {
        let p = fixture();
        let lines = read_raw(&p).unwrap();
        let active = active_path(&lines);
        // from = assistant B head, to = assistant B head. B's turn absorbs the
        // tool_result line AND the following assistant text C (mirrors the
        // renderer merging consecutive assistant events) → indices 1..=3.
        let b = "00000000-0000-0000-0000-0000000000b2".parse().unwrap();
        let (a_start, b_end) = snap_range(&active, b, b).unwrap();
        assert_eq!((a_start, b_end), (1, 3));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn snap_real_user_is_single_line_turn() {
        let p = fixture();
        let lines = read_raw(&p).unwrap();
        let active = active_path(&lines);
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        assert_eq!(snap_range(&active, a, a).unwrap(), (0, 0));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn snap_rejects_reversed_and_offpath() {
        let p = fixture();
        let lines = read_raw(&p).unwrap();
        let active = active_path(&lines);
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        let d = "00000000-0000-0000-0000-0000000000d5".parse().unwrap();
        // reversed: from D to A
        assert!(snap_range(&active, d, a).is_err());
        // off-path uuid
        let ghost = "00000000-0000-0000-0000-0000000000ff".parse().unwrap();
        assert!(snap_range(&active, a, ghost).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_renders_range_with_roles() {
        let p = fixture();
        let a = "00000000-0000-0000-0000-0000000000a1";
        let c = "00000000-0000-0000-0000-0000000000c4";
        let text = extract(&p, Some(a), Some(c), false, false).unwrap();
        assert!(text.contains("[user 00000000-0000-0000-0000-0000000000a1]"));
        assert!(text.contains("A"));
        assert!(text.contains("▸ Bash"));
        assert!(text.contains("→ result: out"));
        assert!(text.contains("C"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_whole_path_when_no_range() {
        let p = fixture();
        let text = extract(&p, None, None, false, false).unwrap();
        // First and last turns of the fixture are present.
        assert!(text.contains("A"));
        assert!(text.contains("D"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_rejects_one_sided_range() {
        let p = fixture();
        let a = "00000000-0000-0000-0000-0000000000a1";
        assert!(extract(&p, Some(a), None, false, false).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_outline_is_compact() {
        let p = fixture();
        let text = extract(&p, None, None, true, false).unwrap();
        // Per-turn header (role + short id), salient tool line, NO results / no full body.
        assert!(text.contains("#0 user 00000000\n"));
        assert!(text.contains("▸Bash"));
        assert!(!text.contains("→ result"), "outline omits tool results");
        assert!(!text.contains("[59 more chars]"), "outline shows salient args, not raw JSON");
        // The last turn (user "D") shows its preview on an indented line.
        assert!(text.contains("\n     D\n"));
        std::fs::remove_file(&p).ok();
    }

    fn two_tree_fixture() -> PathBuf {
        // Pre-compaction tree (a1->b2), then a fresh null-parent root (c3->d4).
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"PRE1"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"assistant","content":[{"type":"text","text":"PRE2"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":null,"message":{"role":"user","content":"ACT1"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000d4","parentUuid":"00000000-0000-0000-0000-0000000000c3","message":{"role":"assistant","content":[{"type":"text","text":"ACT2"}]}}"#, "\n",
        );
        write_tmp("twotree", body)
    }

    #[test]
    fn all_segments_outline_shows_both_trees_with_dividers() {
        let p = two_tree_fixture();
        let text = extract(&p, None, None, true, true).unwrap();
        assert!(text.contains("── segment 1/2 · pre-compaction ──"));
        assert!(text.contains("── segment 2/2 · active ──"));
        assert!(text.contains("PRE1"), "pre-compaction tree's turns appear");
        assert!(text.contains("ACT1"), "active tree's turns appear");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn all_segments_range_within_precompaction_tree() {
        let p = two_tree_fixture();
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let text = extract(&p, Some(a), Some(b), false, true).unwrap();
        assert!(text.contains("PRE1"));
        assert!(text.contains("PRE2"));
        assert!(!text.contains("ACT"), "range stays within the pre-compaction tree");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn all_segments_range_across_trees_errors() {
        let p = two_tree_fixture();
        let a = "00000000-0000-0000-0000-0000000000a1";
        let d = "00000000-0000-0000-0000-0000000000d4";
        let err = extract(&p, Some(a), Some(d), false, true).unwrap_err().to_string();
        assert!(err.contains("segment 1/2") && err.contains("compaction boundary"), "got: {err}");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn outline_tags_compaction_summary() {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"isCompactSummary":true,"message":{"role":"user","content":"This session is being continued from a previous conversation…"}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"user","content":"hello"}}"#, "\n",
        );
        let p = write_tmp("summarytag", body);
        let text = extract(&p, None, None, true, false).unwrap();
        assert!(text.contains("· compaction summary"), "isCompactSummary turn is tagged");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_elides_skill_prompt() {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill: /x/y/skills/brainstorming\n# Brainstorming\nlong body body body"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"user","content":"normal message"}}"#, "\n",
        );
        let p = write_tmp("skillprompt", body);

        // Drill-down: the skill prompt collapses; its body is gone; normal text stays.
        let full = extract(&p, None, None, false, false).unwrap();
        assert!(full.contains("[skill prompt elided: brainstorming,"), "got: {full}");
        assert!(!full.contains("# Brainstorming"), "skill body must not appear");
        assert!(full.contains("normal message"), "normal text stays verbatim");

        // Outline: the skill-prompt turn shows `skill: brainstorming`.
        let outline = extract(&p, None, None, true, false).unwrap();
        assert!(outline.contains("skill: brainstorming"), "got: {outline}");
        assert!(!outline.contains("Base directory for this skill"), "no raw path");

        std::fs::remove_file(&p).ok();
    }

    fn idfix() -> PathBuf {
        // Three uuids with DISTINCT leading hex so prefixes are meaningful:
        // aaaaaaaa…1, bbbbbbbb…2, aaaaaaab…3 (shares "aaaaaaa" with the first).
        let body = concat!(
            r#"{"type":"user","uuid":"aaaaaaaa-0000-0000-0000-000000000001","parentUuid":null,"message":{"role":"user","content":"x"}}"#, "\n",
            r#"{"type":"user","uuid":"bbbbbbbb-0000-0000-0000-000000000002","parentUuid":"aaaaaaaa-0000-0000-0000-000000000001","message":{"role":"user","content":"y"}}"#, "\n",
            r#"{"type":"user","uuid":"aaaaaaab-0000-0000-0000-000000000003","parentUuid":"bbbbbbbb-0000-0000-0000-000000000002","message":{"role":"user","content":"z"}}"#, "\n",
        );
        write_tmp("idfix", body)
    }

    #[test]
    fn resolve_id_prefix_unique_ambiguous_none() {
        let p = idfix();
        let lines = read_raw(&p).unwrap();
        assert_eq!(
            resolve_id(&lines, "bbbbbbbb").unwrap().to_string(),
            "bbbbbbbb-0000-0000-0000-000000000002"
        );
        assert_eq!(
            resolve_id(&lines, "aaaaaaaa").unwrap().to_string(),
            "aaaaaaaa-0000-0000-0000-000000000001"
        );
        assert!(resolve_id(&lines, "aaaaaaa").is_err(), "ambiguous (aaaaaaaa + aaaaaaab)");
        assert!(resolve_id(&lines, "zzzz").is_err(), "no match");
        assert_eq!(
            resolve_id(&lines, "aaaaaaab-0000-0000-0000-000000000003").unwrap().to_string(),
            "aaaaaaab-0000-0000-0000-000000000003"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_accepts_uuid_prefix_range() {
        let p = idfix();
        let text = extract(&p, Some("aaaaaaaa"), Some("bbbbbbbb"), false, false).unwrap();
        assert!(text.contains("x") && text.contains("y"));
        assert!(!text.contains("z"), "range stops at the 'to' turn");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_hints_all_segments_for_precompaction_id() {
        let p = two_tree_fixture();
        let a = "00000000-0000-0000-0000-0000000000a1"; // in the pre-compaction tree
        let err = extract(&p, Some(a), Some(a), false, false).unwrap_err().to_string();
        assert!(err.contains("pass --all-segments"), "got: {err}");
        std::fs::remove_file(&p).ok();
    }

    fn noisy_fixture() -> PathBuf {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args></command-args>"}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"user","content":"<task-notification>\n<status>completed</status>\n<summary>did the thing</summary>\n<result>body</result>\n</task-notification>"}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":"00000000-0000-0000-0000-0000000000b2","message":{"role":"user","content":"<local-command-caveat>Caveat: machinery boilerplate</local-command-caveat>"}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000d4","parentUuid":"00000000-0000-0000-0000-0000000000c3","message":{"role":"user","content":"<local-command-stdout>some command output</local-command-stdout>"}}"#, "\n",
        );
        write_tmp("noisy", body)
    }

    #[test]
    fn outline_compacts_command_echo_and_task_notification() {
        let p = noisy_fixture();
        let text = extract(&p, None, None, true, false).unwrap();
        assert!(text.contains("/compact"), "command echo → /compact");
        assert!(text.contains("task · completed · did the thing"), "task-notif → one line");
        assert!(!text.contains("<command-name>"), "no raw command tags");
        assert!(!text.contains("<task-notification>"), "no raw task tags");
        // local-command machinery (caveat + stdout) is skipped entirely.
        assert!(!text.contains("<local-command-"), "no raw local-command tags");
        assert!(!text.contains("Caveat:"), "caveat turn skipped");
        assert!(!text.contains("some command output"), "stdout turn skipped");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn outline_skips_empty_system_and_attachment_turns() {
        // A real user turn, then an empty compact_boundary system marker and an
        // empty attachment — both should be skipped (bare headers are noise).
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"real message"}}"#, "\n",
            r#"{"type":"system","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","subtype":"compact_boundary","content":"Conversation compacted"}"#, "\n",
            r#"{"type":"attachment","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":"00000000-0000-0000-0000-0000000000b2"}"#, "\n",
        );
        let p = write_tmp("empty", body);
        let text = extract(&p, None, None, true, false).unwrap();
        assert!(text.contains("real message"), "the real user turn shows");
        assert!(!text.contains("system"), "empty compact_boundary turn skipped");
        assert!(!text.contains("attachment"), "empty attachment turn skipped");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn cap_truncates_long_text() {
        let s = "x".repeat(5000);
        let out = cap(&s, 4000);
        assert!(out.contains("more chars]"));
        assert!(out.chars().count() < 5000);
    }

    #[test]
    fn resolve_session_path_accepts_direct_file() {
        let p = fixture();
        let root = std::env::temp_dir();
        assert_eq!(resolve_session_path(&root, p.to_str().unwrap()).unwrap(), p);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn prune_drops_the_range_and_revalidates_clean() {
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-p-{}", std::process::id()));
        let b = "00000000-0000-0000-0000-0000000000b2";
        // Active path message lines: A(user), B(assistant), tool_result(user),
        // C(assistant), D(user) = 5. Pruning B's turn (B + tool_result + C, since
        // turns merge consecutive assistants across the tool_result) leaves A, D.
        let out = rewrite(&root, &p, b, b, RewriteMode::Prune, None).unwrap();
        assert_eq!(out.messages_after, 2);
        let written = read_raw(std::path::Path::new(&out.commit.new_path)).unwrap();
        let vals: Vec<serde_json::Value> = written
            .iter()
            .filter(|l| l.kind == "user" || l.kind == "assistant")
            .map(|l| l.value.clone())
            .collect();
        assert!(validate(&vals).is_empty());
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn summary_message_has_timestamp() {
        // The inserted summary line is built without a timestamp; commit must
        // backfill one, or Claude Code's discovery rejects the resumed file.
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-ts-{}", std::process::id()));
        let b = "00000000-0000-0000-0000-0000000000b2";
        let out = rewrite(&root, &p, b, b, RewriteMode::Summary, Some("the summary")).unwrap();
        let written = read_raw(std::path::Path::new(&out.commit.new_path)).unwrap();
        // Every emitted message line — including the inserted summary — has a ts.
        for m in written.iter().filter(|l| l.kind == "user" || l.kind == "assistant") {
            let ts = m.value.get("timestamp").and_then(|v| v.as_str());
            assert!(
                ts.map(|s| !s.is_empty()).unwrap_or(false),
                "message line missing timestamp: {}",
                m.value
            );
        }
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn summary_inserts_one_message() {
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-s-{}", std::process::id()));
        let b = "00000000-0000-0000-0000-0000000000b2";
        // summarize B's turn (B + tool_result + C) -> one summary line; A and D remain.
        let out = rewrite(&root, &p, b, b, RewriteMode::Summary, Some("did the thing")).unwrap();
        let written = read_raw(std::path::Path::new(&out.commit.new_path)).unwrap();
        let msgs: Vec<&RawLine> = written
            .iter()
            .filter(|l| l.kind == "user" || l.kind == "assistant")
            .collect();
        // A(user) + summary(user) + D(user) = 3 message lines
        assert_eq!(msgs.len(), 3);
        let joined: String = written.iter().map(|l| l.value.to_string()).collect();
        assert!(joined.contains("summarized to save tokens"));
        assert!(joined.contains("did the thing"));
        let vals: Vec<serde_json::Value> = msgs.iter().map(|l| l.value.clone()).collect();
        assert!(validate(&vals).is_empty());
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn summary_requires_text() {
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-e-{}", std::process::id()));
        let a = "00000000-0000-0000-0000-0000000000a1";
        assert!(rewrite(&root, &p, a, a, RewriteMode::Summary, None).is_err());
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn prune_keeps_tool_pairs_intact_no_repairs() {
        // Pruning B's turn removes the tool_use T1 AND its tool_result together
        // (turn-snapping absorbs the tool_result), so no orphan is created and
        // the commit pipeline performs no tool-pairing repairs.
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-np-{}", std::process::id()));
        let b = "00000000-0000-0000-0000-0000000000b2";
        let out = rewrite(&root, &p, b, b, RewriteMode::Prune, None).unwrap();
        assert!(out.commit.repairs.is_empty(), "clean turn-aligned cut should need no repairs");
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn prune_from_head_leaving_leading_assistant_is_refused() {
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-h-{}", std::process::id()));
        let a = "00000000-0000-0000-0000-0000000000a1";
        // Prune just user A (turn 0). The first surviving message becomes
        // assistant B → must be refused.
        let err = rewrite(&root, &p, a, a, RewriteMode::Prune, None);
        assert!(err.is_err(), "pruning the leading user turn should be refused");
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("start with an assistant message"), "got: {}", msg);
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rewrite_files_new_session_under_launch_cwd_not_active_path_cwd() {
        // A compacted session: the pre-compaction tree (launch cwd, FIRST in
        // file order) is orphaned in the same file; the active path is the
        // post-compaction tree, whose cwd is an inner dir the agent had cd'd
        // into. The new session MUST land in the launch project dir so
        // `claude --resume` from the launch dir finds it — not the inner dir.
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"cwd":"/launch","message":{"role":"user","content":"OLD"}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":null,"cwd":"/launch/inner","message":{"role":"user","content":"NEW1"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":"00000000-0000-0000-0000-0000000000b2","cwd":"/launch/inner","message":{"role":"assistant","content":[{"type":"text","text":"R"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000d4","parentUuid":"00000000-0000-0000-0000-0000000000c3","cwd":"/launch/inner","message":{"role":"user","content":"NEW2"}}"#, "\n"
        );
        let src = write_tmp("compact-cwd", body);
        let root = std::env::temp_dir().join(format!("cm-surg-root-cwd-{}", std::process::id()));
        let d4 = "00000000-0000-0000-0000-0000000000d4";
        let out = rewrite(&root, &src, d4, d4, RewriteMode::Prune, None).unwrap();
        let parent = std::path::Path::new(&out.commit.new_path)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap()
            .to_string();
        assert_eq!(
            parent, "-launch",
            "new session must be filed under the launch cwd's project dir, got {parent}"
        );
        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rewrite_accepts_uuid_prefix() {
        // Distinct leading hex so an 8-char prefix is unambiguous.
        let body = concat!(
            r#"{"type":"user","uuid":"aaaaaaaa-0000-0000-0000-000000000001","parentUuid":null,"message":{"role":"user","content":"A"}}"#, "\n",
            r#"{"type":"assistant","uuid":"bbbbbbbb-0000-0000-0000-000000000002","parentUuid":"aaaaaaaa-0000-0000-0000-000000000001","message":{"role":"assistant","content":[{"type":"text","text":"B"}]}}"#, "\n",
            r#"{"type":"user","uuid":"cccccccc-0000-0000-0000-000000000003","parentUuid":"bbbbbbbb-0000-0000-0000-000000000002","message":{"role":"user","content":"C"}}"#, "\n",
        );
        let p = write_tmp("rwprefix", body);
        let root = std::env::temp_dir().join(format!("cm-surg-root-rwpfx-{}", std::process::id()));
        // Prune the assistant turn by an 8-char prefix; a full uuid would also work.
        let out = rewrite(&root, &p, "bbbbbbbb", "bbbbbbbb", RewriteMode::Prune, None).unwrap();
        assert_eq!(out.messages_after, 2, "assistant turn pruned; A + C remain");
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rewrite_rejects_off_active_path_id() {
        let p = two_tree_fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-offpath-{}", std::process::id()));
        let a = "00000000-0000-0000-0000-0000000000a1"; // in the pre-compaction tree
        let err = rewrite(&root, &p, a, a, RewriteMode::Prune, None).unwrap_err().to_string();
        assert!(err.contains("not on the active path"), "got: {err}");
        assert!(err.contains("active conversation only"), "got: {err}");
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
