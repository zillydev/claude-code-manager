//! Verbatim copy-on-edit ("fork v2").
//!
//! [`edit::commit`](super::edit::commit) builds a fork by reconstructing the
//! fork point's ancestor chain, flattening it, regenerating every uuid, and
//! trimming to claude's live context segment. That produces a small, clean file
//! — at the cost of discarding everything off the chain (abandoned rewind
//! branches, pre-compaction segments) and of trusting a heuristic to decide
//! what claude still holds.
//!
//! This module takes the opposite bet: copy the source file's lines UP TO the
//! fork point in FILE order, byte-for-byte, and change only what session
//! identity requires. Nothing is flattened, dropped, trimmed, or re-uuid'd, so
//! the copy carries exactly the history the source had at that point.
//!
//! Deviations from a literal byte copy, and why each is unavoidable:
//!
//! 1. **`sessionId`** is rewritten on every line that has one. It is
//!    denormalized onto each line, and two files claiming one id would make
//!    `claude --resume <id>` ambiguous. This is what forces a parse-and-rewrite
//!    pass at all.
//! 2. **`forkedFrom`** is stamped on the cut line only — enough for
//!    [`summary`](super::summary) to resolve both the source session (first
//!    occurrence) and the branch point (last occurrence). It is needed because
//!    uuid-overlap lineage inference explicitly rejects a candidate sharing the
//!    descendant's head, and a prefix copy always shares its source's head.
//! 3. A fresh **`last-prompt`** trailer (and an optional `custom-title`) is
//!    appended so the copy's leaf and title resolve against itself.
//! 4. A **synthesized `tool_result`** is appended ONLY when the copy would
//!    otherwise end on an unanswered `tool_use`. Mid-conversation the source
//!    already tolerates such a call, but as the FINAL message the API rejects
//!    it, so the copy would not resume at all.
//!
//! Timestamps are deliberately NOT backfilled: nothing here synthesizes a
//! message line out of thin air, so a null timestamp in the copy means the
//! source had one — in which case the source is equally undiscoverable.

use std::path::Path;

use anyhow::{bail, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use super::edit::{last_user_text, CommitResult};
use super::raw::RawLine;
use crate::store::paths;

#[derive(Debug, Clone)]
pub struct CloneOpts {
    /// Target project cwd — decides which project dir the copy lands in.
    pub target_cwd: String,
    /// Optional custom title, appended as a trailing `custom-title` line.
    pub custom_title: Option<String>,
}

/// Whether `l` is a user line carrying nothing but `tool_result` blocks — the
/// machinery line claude appends when a tool finishes.
fn is_pure_tool_result(l: &RawLine) -> bool {
    if l.kind != "user" {
        return false;
    }
    match l.value.pointer("/message/content").and_then(Value::as_array) {
        Some(blocks) if !blocks.is_empty() => blocks
            .iter()
            .all(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")),
        _ => false,
    }
}

fn tool_use_ids(v: &Value) -> Vec<String> {
    v.pointer("/message/content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                .filter_map(|b| b.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The `tool_use` ids still awaiting a result at the END of `values`.
///
/// Mirrors [`edit::validate`](super::edit::validate)'s state machine, including
/// its `message.id` grouping: claude writes each content block of ONE assistant
/// message as its own line, so a line sharing the previous line's id continues
/// that message rather than abandoning its calls. Only the trailing open set
/// matters here — an unanswered call in the middle is something the source
/// already carries, and copying it changes nothing.
fn trailing_unanswered(values: &[&Value]) -> Vec<String> {
    let mut open: Vec<String> = Vec::new();
    let mut open_msg: Option<String> = None;
    for v in values {
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "assistant" => {
                let msg_id = v.pointer("/message/id").and_then(Value::as_str).map(str::to_string);
                if !(msg_id.is_some() && msg_id == open_msg) {
                    open.clear();
                }
                open.extend(tool_use_ids(v));
                open_msg = msg_id;
            }
            "user" => {
                if let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                            if let Some(id) = b.get("tool_use_id").and_then(Value::as_str) {
                                open.retain(|o| o != id);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    open
}

/// Position of the last line to copy: `cut`'s own index in FILE order, extended
/// forward over the `tool_result` lines that answer it. The extension keeps a
/// tool call together with its result without reordering or rewriting anything
/// — the copy simply ends a line or two later than the clicked message.
fn cut_index(lines: &[RawLine], cut: Uuid) -> Option<usize> {
    let mut idx = lines.iter().position(|l| l.uuid == Some(cut))?;
    while idx + 1 < lines.len() && is_pure_tool_result(&lines[idx + 1]) {
        idx += 1;
    }
    Some(idx)
}

/// Copy `lines[..=cut]` (file order) as a new session. See the module docs for
/// exactly what is and is not preserved. Never touches any existing file.
pub fn clone_prefix(
    root: &Path,
    lines: &[RawLine],
    cut: Uuid,
    opts: &CloneOpts,
) -> Result<CommitResult> {
    let Some(idx) = cut_index(lines, cut) else {
        bail!("fork point {cut} not found in session");
    };
    let slice = &lines[..=idx];
    let new_session = Uuid::new_v4();
    let source_session = slice
        .iter()
        .find_map(|l| l.value.get("sessionId").and_then(Value::as_str))
        .map(str::to_string);

    // The deepest copied MESSAGE line is the branch point `forkedFrom` records.
    let branch_at = slice
        .iter()
        .rposition(|l| l.kind == "user" || l.kind == "assistant");

    let mut values: Vec<Value> = slice.iter().map(|l| l.value.clone()).collect();
    for (i, v) in values.iter_mut().enumerate() {
        let Some(obj) = v.as_object_mut() else { continue };
        if obj.contains_key("sessionId") {
            obj.insert("sessionId".into(), Value::String(new_session.to_string()));
        }
        if Some(i) == branch_at {
            if let Some(src) = &source_session {
                let mut ff = serde_json::Map::new();
                ff.insert("sessionId".into(), Value::String(src.clone()));
                if let Some(mu) = slice[i].uuid {
                    ff.insert("messageUuid".into(), Value::String(mu.to_string()));
                }
                obj.insert("forkedFrom".into(), Value::Object(ff));
            }
        }
    }

    let mut out_lines: Vec<String> = values.iter().map(Value::to_string).collect();

    // Close a trailing tool call so the copy is resumable (see module docs).
    let msgs: Vec<&Value> = values
        .iter()
        .filter(|v| matches!(v.get("type").and_then(Value::as_str), Some("user") | Some("assistant")))
        .collect();
    let unanswered = trailing_unanswered(&msgs);
    let mut leaf = slice.iter().rev().find_map(|l| l.uuid);
    if !unanswered.is_empty() {
        let blocks: Vec<Value> = unanswered
            .iter()
            .map(|id| {
                json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": "[tool did not finish before this fork point]",
                    "is_error": false
                })
            })
            .collect();
        let synth_uuid = Uuid::new_v4();
        let last = slice.last();
        let mut synth = json!({
            "type": "user",
            "uuid": synth_uuid.to_string(),
            "parentUuid": leaf.map(|u| Value::String(u.to_string())).unwrap_or(Value::Null),
            "sessionId": new_session.to_string(),
            "message": { "role": "user", "content": blocks },
        });
        // Carry the neighbouring line's timestamp/cwd so the synthesized line
        // can't be the thing that makes the file undiscoverable.
        for key in ["timestamp", "cwd", "gitBranch"] {
            if let Some(val) = last.and_then(|l| l.value.get(key)).cloned() {
                synth.as_object_mut().unwrap().insert(key.into(), val);
            }
        }
        out_lines.push(synth.to_string());
        leaf = Some(synth_uuid);
    }

    if let Some(leaf) = leaf {
        out_lines.push(
            json!({
                "type": "last-prompt",
                "leafUuid": leaf.to_string(),
                "lastPrompt": last_user_text(&values),
                "sessionId": new_session.to_string(),
            })
            .to_string(),
        );
    }
    if let Some(title) = &opts.custom_title {
        out_lines.push(
            json!({
                "type": "custom-title",
                "customTitle": title,
                "sessionId": new_session.to_string(),
            })
            .to_string(),
        );
    }

    let path = paths::session_file_path(root, &opts.target_cwd, new_session);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, out_lines.join("\n") + "\n")?;

    Ok(CommitResult {
        new_session_id: new_session.to_string(),
        new_path: path.to_string_lossy().into_owned(),
        // A verbatim copy repairs nothing; the synthesized closer (if any) is
        // reported so the UI can say why the file is not a literal copy.
        repairs: unanswered
            .into_iter()
            .map(|detail| super::edit::Violation { kind: "closed_trailing_tool_use".into(), detail })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::raw::read_raw;

    fn write_tmp(name: &str, body: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("cm-clone-{}-{}.jsonl", name, std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    fn tmp_root(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("cm-clone-root-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    const A: &str = "00000000-0000-0000-0000-0000000000a1";
    const B: &str = "00000000-0000-0000-0000-0000000000b2";
    const C: &str = "00000000-0000-0000-0000-0000000000c3";
    const D: &str = "00000000-0000-0000-0000-0000000000d4";

    fn opts() -> CloneOpts {
        CloneOpts { target_cwd: "/tmp/proj".into(), custom_title: None }
    }

    /// Read back the lines a clone wrote.
    fn clone_to_lines(body: &str, cut: &str, tag: &str) -> Vec<RawLine> {
        let src = write_tmp(tag, body);
        let root = tmp_root(tag);
        let lines = read_raw(&src).unwrap();
        let res = clone_prefix(&root, &lines, cut.parse().unwrap(), &opts()).unwrap();
        let out = read_raw(std::path::Path::new(&res.new_path)).unwrap();
        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
        out
    }

    #[test]
    fn clone_keeps_uuids_parents_and_off_path_lines() {
        // b2 is an ABANDONED branch (c3 also parents to a1). commit() would drop
        // it; a verbatim copy keeps it, exactly as the source had it, and keeps
        // every uuid/parentUuid untouched.
        let body = format!(
            concat!(
                r#"{{"type":"user","uuid":"{a}","parentUuid":null,"sessionId":"OLD","cwd":"/tmp/proj","timestamp":"2026-01-01T00:00:00.000Z","message":{{"role":"user","content":"hi"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"{b}","parentUuid":"{a}","sessionId":"OLD","timestamp":"2026-01-01T00:00:01.000Z","message":{{"id":"m1","role":"assistant","content":[{{"type":"text","text":"abandoned"}}]}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"{c}","parentUuid":"{a}","sessionId":"OLD","timestamp":"2026-01-01T00:00:02.000Z","message":{{"id":"m2","role":"assistant","content":[{{"type":"text","text":"kept"}}]}}}}"#, "\n",
            ),
            a = A, b = B, c = C
        );
        let out = clone_to_lines(&body, C, "keep");
        let uuids: Vec<String> =
            out.iter().filter_map(|l| l.uuid.map(|u| u.to_string())).collect();
        assert!(uuids.contains(&B.to_string()), "abandoned branch is preserved");
        assert_eq!(uuids[0], A, "uuids are NOT regenerated");
        let c_line = out.iter().find(|l| l.uuid == Some(C.parse().unwrap())).unwrap();
        assert_eq!(c_line.parent, Some(A.parse().unwrap()), "parent links untouched");
    }

    #[test]
    fn clone_rewrites_session_id_on_every_line() {
        let body = format!(
            concat!(
                r#"{{"type":"user","uuid":"{a}","parentUuid":null,"sessionId":"OLD","timestamp":"2026-01-01T00:00:00.000Z","message":{{"role":"user","content":"hi"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"{b}","parentUuid":"{a}","sessionId":"OLD","timestamp":"2026-01-01T00:00:01.000Z","message":{{"id":"m1","role":"assistant","content":[{{"type":"text","text":"yo"}}]}}}}"#, "\n",
            ),
            a = A, b = B
        );
        let out = clone_to_lines(&body, B, "sid");
        let ids: std::collections::HashSet<&str> =
            out.iter().filter_map(|l| l.value.get("sessionId").and_then(Value::as_str)).collect();
        assert_eq!(ids.len(), 1, "one session id across the file");
        assert!(!ids.contains("OLD"), "the source's id must not survive");
    }

    #[test]
    fn clone_stops_at_the_cut_and_drops_nothing_before_it() {
        let body = format!(
            concat!(
                r#"{{"type":"user","uuid":"{a}","parentUuid":null,"sessionId":"OLD","timestamp":"2026-01-01T00:00:00.000Z","message":{{"role":"user","content":"hi"}}}}"#, "\n",
                r#"{{"type":"attachment","uuid":"{b}","parentUuid":"{a}","sessionId":"OLD"}}"#, "\n",
                r#"{{"type":"assistant","uuid":"{c}","parentUuid":"{b}","sessionId":"OLD","timestamp":"2026-01-01T00:00:02.000Z","message":{{"id":"m1","role":"assistant","content":[{{"type":"text","text":"cut here"}}]}}}}"#, "\n",
                r#"{{"type":"user","uuid":"{d}","parentUuid":"{c}","sessionId":"OLD","timestamp":"2026-01-01T00:00:03.000Z","message":{{"role":"user","content":"after"}}}}"#, "\n",
            ),
            a = A, b = B, c = C, d = D
        );
        let out = clone_to_lines(&body, C, "cut");
        let uuids: Vec<String> =
            out.iter().filter_map(|l| l.uuid.map(|u| u.to_string())).collect();
        assert!(uuids.contains(&B.to_string()), "attachment lines survive (commit drops them)");
        assert!(uuids.contains(&C.to_string()));
        assert!(!uuids.contains(&D.to_string()), "nothing after the cut");
    }

    #[test]
    fn clone_extends_the_cut_over_the_answering_tool_result() {
        // Cutting at the tool_use alone would end the copy on an unanswered
        // call; the result line immediately after it comes along.
        let body = format!(
            concat!(
                r#"{{"type":"user","uuid":"{a}","parentUuid":null,"sessionId":"OLD","timestamp":"2026-01-01T00:00:00.000Z","message":{{"role":"user","content":"run"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"{b}","parentUuid":"{a}","sessionId":"OLD","timestamp":"2026-01-01T00:00:01.000Z","message":{{"id":"m1","role":"assistant","content":[{{"type":"tool_use","id":"T1","name":"Bash","input":{{}}}}]}}}}"#, "\n",
                r#"{{"type":"user","uuid":"{c}","parentUuid":"{b}","sessionId":"OLD","timestamp":"2026-01-01T00:00:02.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"T1","content":"ok"}}]}}}}"#, "\n",
                r#"{{"type":"user","uuid":"{d}","parentUuid":"{c}","sessionId":"OLD","timestamp":"2026-01-01T00:00:03.000Z","message":{{"role":"user","content":"next"}}}}"#, "\n",
            ),
            a = A, b = B, c = C, d = D
        );
        let out = clone_to_lines(&body, B, "pair");
        let uuids: Vec<String> =
            out.iter().filter_map(|l| l.uuid.map(|u| u.to_string())).collect();
        assert!(uuids.contains(&C.to_string()), "the answering tool_result is included");
        assert!(!uuids.contains(&D.to_string()), "but nothing beyond it");
    }

    #[test]
    fn clone_closes_a_tool_call_that_never_got_a_result() {
        // An interrupted tool: no result exists anywhere. Mid-conversation the
        // source tolerates it, but as the copy's FINAL message the API would
        // reject it — so one closer is synthesized and reported.
        let body = format!(
            concat!(
                r#"{{"type":"user","uuid":"{a}","parentUuid":null,"sessionId":"OLD","timestamp":"2026-01-01T00:00:00.000Z","message":{{"role":"user","content":"run"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"{b}","parentUuid":"{a}","sessionId":"OLD","timestamp":"2026-01-01T00:00:01.000Z","message":{{"id":"m1","role":"assistant","content":[{{"type":"tool_use","id":"T1","name":"Bash","input":{{}}}}]}}}}"#, "\n",
            ),
            a = A, b = B
        );
        let src = write_tmp("orphan", &body);
        let root = tmp_root("orphan");
        let lines = read_raw(&src).unwrap();
        let res = clone_prefix(&root, &lines, B.parse().unwrap(), &opts()).unwrap();
        assert_eq!(res.repairs.len(), 1);
        assert_eq!(res.repairs[0].detail, "T1");
        let out = read_raw(std::path::Path::new(&res.new_path)).unwrap();
        let msgs: Vec<&Value> = out
            .iter()
            .filter(|l| l.kind == "user" || l.kind == "assistant")
            .map(|l| &l.value)
            .collect();
        assert!(trailing_unanswered(&msgs).is_empty(), "copy ends fully paired");
        let last = msgs.last().unwrap();
        assert!(last.get("timestamp").is_some(), "synthesized line carries a timestamp");
        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clone_records_provenance_on_the_branch_point() {
        let body = format!(
            concat!(
                r#"{{"type":"user","uuid":"{a}","parentUuid":null,"sessionId":"OLD","timestamp":"2026-01-01T00:00:00.000Z","message":{{"role":"user","content":"hi"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"{b}","parentUuid":"{a}","sessionId":"OLD","timestamp":"2026-01-01T00:00:01.000Z","message":{{"id":"m1","role":"assistant","content":[{{"type":"text","text":"yo"}}]}}}}"#, "\n",
            ),
            a = A, b = B
        );
        let out = clone_to_lines(&body, B, "prov");
        let tagged: Vec<&RawLine> =
            out.iter().filter(|l| l.value.get("forkedFrom").is_some()).collect();
        assert_eq!(tagged.len(), 1, "provenance goes on the branch point only");
        assert_eq!(tagged[0].uuid, Some(B.parse().unwrap()));
        let ff = tagged[0].value.get("forkedFrom").unwrap();
        assert_eq!(ff.get("sessionId").and_then(Value::as_str), Some("OLD"));
        assert_eq!(ff.get("messageUuid").and_then(Value::as_str), Some(B));
    }

    #[test]
    fn clone_rejects_an_unknown_cut() {
        let body = format!(
            r#"{{"type":"user","uuid":"{a}","parentUuid":null,"sessionId":"OLD","message":{{"role":"user","content":"hi"}}}}"#,
            a = A
        );
        let src = write_tmp("unknown", &format!("{body}\n"));
        let root = tmp_root("unknown");
        let lines = read_raw(&src).unwrap();
        assert!(clone_prefix(&root, &lines, D.parse().unwrap(), &opts()).is_err());
        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
