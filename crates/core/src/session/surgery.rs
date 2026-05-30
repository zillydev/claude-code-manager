//! Session "surgery": summarize or prune a range of messages on the active path,
//! producing a new flattened session via `edit::commit` (copy-on-edit).

use anyhow::{anyhow, Result};
use serde_json::Value;
use uuid::Uuid;

use super::raw::RawLine;

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
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
}
