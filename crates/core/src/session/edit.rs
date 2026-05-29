use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::raw::RawLine;
use crate::store::paths;

/// A problem detected in a candidate event list. `commit` repairs these and
/// returns the list so the UI can show what changed.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Violation {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitResult {
    pub new_session_id: String,
    pub new_path: String,
    pub repairs: Vec<Violation>,
}

#[derive(Debug, Clone)]
pub struct CommitOpts {
    /// Target project cwd (used for dir encoding + the `cwd` field on each line).
    pub target_cwd: String,
    /// Git branch to stamp; pass through from the source if unknown.
    pub git_branch: Option<String>,
    /// Optional custom title written as a trailing `custom-title` line.
    pub custom_title: Option<String>,
}

/// Message-bearing line types we keep in a flattened session.
fn is_message_line(kind: &str) -> bool {
    matches!(kind, "user" | "assistant")
}

/// Extract the `tool_use` ids declared by an assistant line's content blocks.
fn tool_use_ids(value: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
        for b in blocks {
            if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                if let Some(id) = b.get("id").and_then(Value::as_str) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids
}

/// Extract the `tool_use_id`s a user line answers via tool_result blocks.
fn tool_result_ids(value: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
        for b in blocks {
            if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                if let Some(id) = b.get("tool_use_id").and_then(Value::as_str) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids
}

/// Validate tool pairing over an ordered slice of message values. Pure.
pub fn validate(values: &[Value]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut open: Vec<String> = Vec::new(); // tool_use ids awaiting a result
    for v in values {
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "assistant" {
            if !open.is_empty() {
                for id in open.drain(..) {
                    violations.push(Violation {
                        kind: "dangling_tool_use".into(),
                        detail: id,
                    });
                }
            }
            open.extend(tool_use_ids(v));
        } else if kind == "user" {
            for id in tool_result_ids(v) {
                if let Some(pos) = open.iter().position(|o| o == &id) {
                    open.remove(pos);
                } else {
                    violations.push(Violation {
                        kind: "orphan_tool_result".into(),
                        detail: id,
                    });
                }
            }
        }
    }
    for id in open {
        violations.push(Violation { kind: "dangling_tool_use".into(), detail: id });
    }
    violations
}

/// Write the chosen, ordered path lines as a new flattened, self-consistent
/// session file. Never touches any existing file.
pub fn commit(root: &Path, path_lines: &[&RawLine], opts: &CommitOpts) -> Result<CommitResult> {
    // 1. Keep only message-bearing lines.
    let mut values: Vec<Value> = path_lines
        .iter()
        .filter(|l| is_message_line(&l.kind))
        .map(|l| l.value.clone())
        .collect();

    // 2. Tool-pairing repair.
    let repairs = repair_tool_pairing(&mut values);

    // 3. Regenerate uuids + relink parentUuid; stamp identity.
    let new_session = Uuid::new_v4();
    let mut prev: Option<Uuid> = None;
    for v in values.iter_mut() {
        let new_uuid = Uuid::new_v4();
        set_str(v, "uuid", &new_uuid.to_string());
        match prev {
            Some(p) => set_str(v, "parentUuid", &p.to_string()),
            None => {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("parentUuid".into(), Value::Null);
                }
            }
        }
        set_str(v, "sessionId", &new_session.to_string());
        set_str(v, "cwd", &opts.target_cwd);
        if let Some(b) = &opts.git_branch {
            set_str(v, "gitBranch", b);
        }
        prev = Some(new_uuid);
    }

    // 4. Build output lines.
    let mut out_lines: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    if let Some(leaf) = prev {
        let last_prompt_text = last_user_text(&values);
        let lp = json!({
            "type": "last-prompt",
            "leafUuid": leaf.to_string(),
            "lastPrompt": last_prompt_text,
            "sessionId": new_session.to_string(),
        });
        out_lines.push(lp.to_string());
    }
    if let Some(title) = &opts.custom_title {
        let ct = json!({
            "type": "custom-title",
            "customTitle": title,
            "sessionId": new_session.to_string(),
        });
        out_lines.push(ct.to_string());
    }

    // 5. Write.
    let path = paths::session_file_path(root, &opts.target_cwd, new_session);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, out_lines.join("\n") + "\n")?;

    Ok(CommitResult {
        new_session_id: new_session.to_string(),
        new_path: path.to_string_lossy().into_owned(),
        repairs,
    })
}

fn repair_tool_pairing(values: &mut Vec<Value>) -> Vec<Violation> {
    let repairs = validate(values);
    if repairs.is_empty() {
        return repairs;
    }
    let orphans: std::collections::HashSet<String> = repairs
        .iter()
        .filter(|v| v.kind == "orphan_tool_result")
        .map(|v| v.detail.clone())
        .collect();
    let dangling: Vec<String> = repairs
        .iter()
        .filter(|v| v.kind == "dangling_tool_use")
        .map(|v| v.detail.clone())
        .collect();

    // Drop orphan tool_result blocks; drop now-empty user lines.
    if !orphans.is_empty() {
        values.retain_mut(|v| {
            if v.get("type").and_then(Value::as_str) != Some("user") {
                return true;
            }
            if let Some(blocks) = v.pointer_mut("/message/content").and_then(Value::as_array_mut) {
                blocks.retain(|b| {
                    if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let id = b.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                        return !orphans.contains(id);
                    }
                    true
                });
                return !blocks.is_empty();
            }
            true
        });
    }

    // For each dangling tool_use, insert a synthesized user tool_result line
    // right after the assistant line that declares it.
    for id in dangling {
        let pos = values.iter().position(|v| {
            v.get("type").and_then(Value::as_str) == Some("assistant")
                && tool_use_ids(v).iter().any(|x| x == &id)
        });
        if let Some(idx) = pos {
            let synth = json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": "[result removed during editing]",
                        "is_error": false
                    }]
                }
            });
            values.insert(idx + 1, synth);
        }
    }
    repairs
}

fn set_str(v: &mut Value, key: &str, val: &str) {
    if let Some(obj) = v.as_object_mut() {
        obj.insert(key.into(), Value::String(val.to_string()));
    }
}

/// Best-effort "last user message text" for the last-prompt bookmark.
fn last_user_text(values: &[Value]) -> String {
    for v in values.iter().rev() {
        if v.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let content = v.pointer("/message/content");
        if let Some(s) = content.and_then(Value::as_str) {
            return s.to_string();
        }
        if let Some(arr) = content.and_then(Value::as_array) {
            for b in arr {
                if b.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        return t.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

/// Set the primary editable text of a message line. If the content is a block
/// array, the first `text` block is updated (one is inserted if none exists);
/// otherwise the content is set to a plain string. Used by edit-text.
pub fn set_message_text(line: &mut RawLine, text: &str) {
    let v = &mut line.value;
    // Block-array content: update the first text block (insert if absent).
    if let Some(blocks) = v.pointer_mut("/message/content").and_then(Value::as_array_mut) {
        for b in blocks.iter_mut() {
            if b.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(o) = b.as_object_mut() {
                    o.insert("text".into(), Value::String(text.to_string()));
                }
                return;
            }
        }
        blocks.insert(0, json!({ "type": "text", "text": text }));
        return;
    }
    // String (or scalar) content: set a plain string.
    if let Some(obj) = v.pointer_mut("/message").and_then(Value::as_object_mut) {
        obj.insert("content".into(), Value::String(text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::raw::{active_path, read_raw};
    use std::path::PathBuf;

    fn write_tmp(name: &str, body: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("cm-edit-{}-{}.jsonl", name, std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn validate_flags_dangling_and_orphan() {
        let assistant = json!({"type":"assistant","message":{"content":[
            {"type":"tool_use","id":"T1","name":"Bash","input":{}}]}});
        let user = json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"T9","content":"x"}]}});
        let v = validate(&[assistant, user]);
        assert!(v.iter().any(|x| x.kind == "dangling_tool_use" && x.detail == "T1"));
        assert!(v.iter().any(|x| x.kind == "orphan_tool_result" && x.detail == "T9"));
    }

    #[test]
    fn commit_produces_linear_resolvable_session() {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"hello"},"version":"keep-me"}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":"00000000-0000-0000-0000-0000000000b2","message":{"role":"user","content":"bye"}}"#, "\n"
        );
        let src = write_tmp("commit-src", body);
        let lines = read_raw(&src).unwrap();
        let path = active_path(&lines);

        let root = std::env::temp_dir().join(format!("cm-root-{}", std::process::id()));
        let opts = CommitOpts {
            target_cwd: "/tmp/proj".into(),
            git_branch: Some("main".into()),
            custom_title: None,
        };
        let res = commit(&root, &path, &opts).unwrap();

        let written = read_raw(std::path::Path::new(&res.new_path)).unwrap();
        let msgs: Vec<&RawLine> =
            written.iter().filter(|l| l.kind == "user" || l.kind == "assistant").collect();
        assert_eq!(msgs.len(), 3);
        assert!(msgs[0].parent.is_none());
        assert_eq!(msgs[1].parent, msgs[0].uuid);
        assert_eq!(msgs[2].parent, msgs[1].uuid);
        assert_eq!(msgs[0].value.get("sessionId").and_then(|v| v.as_str()), Some(res.new_session_id.as_str()));
        assert_eq!(msgs[0].value.get("version").and_then(|v| v.as_str()), Some("keep-me"));
        assert_eq!(msgs[0].value.get("cwd").and_then(|v| v.as_str()), Some("/tmp/proj"));
        let leaf = written.iter().rev().find(|l| l.kind == "user" || l.kind == "assistant").unwrap().uuid;
        let lp = written.iter().find(|l| l.kind == "last-prompt").unwrap();
        assert_eq!(lp.value.get("leafUuid").and_then(|v| v.as_str()).map(|s| s.to_string()),
                   leaf.map(|u| u.to_string()));

        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn commit_repairs_dangling_tool_use() {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"assistant","content":[{"type":"tool_use","id":"T1","name":"Bash","input":{}}]}}"#, "\n"
        );
        let src = write_tmp("repair", body);
        let lines = read_raw(&src).unwrap();
        let path = active_path(&lines);
        let root = std::env::temp_dir().join(format!("cm-root2-{}", std::process::id()));
        let opts = CommitOpts { target_cwd: "/tmp/p".into(), git_branch: None, custom_title: None };
        let res = commit(&root, &path, &opts).unwrap();
        assert!(res.repairs.iter().any(|r| r.kind == "dangling_tool_use"));
        let written = read_raw(std::path::Path::new(&res.new_path)).unwrap();
        let vals: Vec<serde_json::Value> = written.iter()
            .filter(|l| l.kind=="user"||l.kind=="assistant").map(|l| l.value.clone()).collect();
        assert!(validate(&vals).is_empty());
        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn commit_repairs_orphan_tool_results() {
        // A: user "go"
        // B: assistant tool_use T1
        // C: user [tool_result T1 (real), tool_result T9 (orphan)]  -> drop T9, keep C
        // D: assistant text "ok"
        // E: user [tool_result T8 (orphan only)]                    -> dropped entirely
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"assistant","content":[{"type":"tool_use","id":"T1","name":"Bash","input":{}}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":"00000000-0000-0000-0000-0000000000b2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"T1","content":"ok"},{"type":"tool_result","tool_use_id":"T9","content":"orphan"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000d4","parentUuid":"00000000-0000-0000-0000-0000000000c3","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000e5","parentUuid":"00000000-0000-0000-0000-0000000000d4","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"T8","content":"orphan2"}]}}"#, "\n"
        );
        let src = write_tmp("orphan", body);
        let lines = read_raw(&src).unwrap();
        let path = active_path(&lines);
        let root = std::env::temp_dir().join(format!("cm-root3-{}", std::process::id()));
        let opts = CommitOpts { target_cwd: "/tmp/p".into(), git_branch: None, custom_title: None };
        let res = commit(&root, &path, &opts).unwrap();

        // Two orphan violations reported.
        assert_eq!(res.repairs.iter().filter(|r| r.kind == "orphan_tool_result").count(), 2);

        let written = read_raw(std::path::Path::new(&res.new_path)).unwrap();
        let msgs: Vec<&RawLine> = written.iter().filter(|l| l.kind == "user" || l.kind == "assistant").collect();
        // E (pure-orphan) dropped → A, B, C, D remain.
        assert_eq!(msgs.len(), 4);
        // C kept exactly the real T1 result (orphan T9 dropped).
        let c = &msgs[2];
        let blocks = c.value.pointer("/message/content").and_then(|v| v.as_array()).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].get("tool_use_id").and_then(|v| v.as_str()), Some("T1"));
        // Output re-validates clean.
        let vals: Vec<serde_json::Value> = msgs.iter().map(|l| l.value.clone()).collect();
        assert!(validate(&vals).is_empty());

        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
