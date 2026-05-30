//! Session "surgery": summarize or prune a range of messages on the active path,
//! producing a new flattened session via `edit::commit` (copy-on-edit).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde_json::Value;
use uuid::Uuid;

use super::edit::{commit, CommitOpts, CommitResult};
use super::raw::{active_path, read_raw, RawLine};
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
fn render_range_text(active: &[&RawLine], a_start: usize, b_end: usize) -> String {
    let mut out = String::new();
    for l in &active[a_start..=b_end] {
        let role = l
            .value
            .pointer("/message/role")
            .and_then(|v| v.as_str())
            .unwrap_or(l.kind.as_str());
        out.push_str(&format!("[{}]\n", role));
        match l.value.pointer("/message/content") {
            Some(Value::String(s)) => {
                out.push_str(s);
                out.push('\n');
            }
            Some(Value::Array(blocks)) => {
                for b in blocks {
                    match b.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                out.push_str(t);
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

/// Read a session, resolve the range, and render it as text.
pub fn extract(path: &Path, from: Uuid, to: Uuid) -> Result<String> {
    let lines = read_raw(path)?;
    let active = active_path(&lines);
    let (a, b) = snap_range(&active, from, to)?;
    Ok(render_range_text(&active, a, b))
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
    from: Uuid,
    to: Uuid,
    mode: RewriteMode,
    summary: Option<&str>,
) -> Result<RewriteOutcome> {
    let lines = read_raw(path)?;
    let active = active_path(&lines);
    let (a, b) = snap_range(&active, from, to)?;

    let messages_before = active.iter().filter(|l| is_message(l)).count();
    let removed_bytes: usize = active[a..=b].iter().map(|l| l.value.to_string().len()).sum();

    // Inherit the source session's cwd so the new session lands in the same
    // project dir. Falls back to "." only for a session with no cwd anywhere
    // (e.g. a metadata-only stub) — harmless, just an unusual target dir.
    let cwd = active
        .iter()
        .find_map(|l| l.value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string());
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
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        let c = "00000000-0000-0000-0000-0000000000c4".parse().unwrap();
        let text = extract(&p, a, c).unwrap();
        assert!(text.contains("[user]"));
        assert!(text.contains("A"));
        assert!(text.contains("▸ Bash"));
        assert!(text.contains("→ result: out"));
        assert!(text.contains("C"));
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
        let b = "00000000-0000-0000-0000-0000000000b2".parse().unwrap();
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
    fn summary_inserts_one_message() {
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-s-{}", std::process::id()));
        let b = "00000000-0000-0000-0000-0000000000b2".parse().unwrap();
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
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
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
        let b = "00000000-0000-0000-0000-0000000000b2".parse().unwrap();
        let out = rewrite(&root, &p, b, b, RewriteMode::Prune, None).unwrap();
        assert!(out.commit.repairs.is_empty(), "clean turn-aligned cut should need no repairs");
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
