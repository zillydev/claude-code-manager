# Session Editor Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the safe session-file write pipeline (copy-on-edit, flatten, invariant-preserving), prove a generated session resumes in real Claude Code, then ship the first two user-facing features: edit a message's text, and fork a new session from any message.

**Architecture:** A new `core::session::edit` module is the *only* writer of session `.jsonl` files. It operates on **raw `serde_json::Value` lines** (lossless — preserves every field Claude Code wrote) and rewrites only linkage fields (`uuid`, `parentUuid`, `sessionId`, `cwd`, `gitBranch`) plus message content. The active path is resolved with the existing `tree` logic. Commit always writes a NEW file (new session UUID) into the target project dir; originals are never mutated. Tauri commands expose `open_for_edit`, `commit_session`, and `fork_session`; the React UI adds inline message editing and a "Fork from here" action.

**Tech Stack:** Rust (serde_json, uuid, chrono, anyhow), Tauri 2, React + TypeScript. Scope = spec phases 0, 1, 1.5 from `docs/superpowers/specs/2026-05-30-session-editor-design.md`. Phases 2–5 (structural edits, editor affordances, cross-session, cross-project/lifecycle) get their own plans after this lands.

---

## File Structure

**Core (Rust) — the write path:**
- Create `crates/core/src/session/raw.rs` — read a session file as an ordered `Vec<RawLine>` (each wraps a `serde_json::Value` plus extracted `uuid`/`parent`/`type`); resolve the active path over raw lines; helpers to read/set linkage fields and message text on a `Value`.
- Create `crates/core/src/session/edit.rs` — `validate()`, `commit()`, `fork()`, the `CommitResult`/`Violation`/`Repair`/`CommitOpts` types. The single file writer.
- Modify `crates/core/src/session/mod.rs` — register the two new modules.
- Modify `crates/core/src/store/paths.rs` — add `encode_project_dir(cwd) -> String` (inverse of the existing decode) and `session_file_path(root, cwd, id)`.

**WebUI (Rust) — commands:**
- Modify `crates/webui/src/commands.rs` — add `open_for_edit`, `commit_session`, `fork_session`.
- Modify `crates/webui/src/lib.rs` — register the three handlers.

**Frontend (React) — UI:**
- Modify `crates/webui/frontend/src/types.ts` — add `EditableSession`, `EditableTurn`, `CommitResult` types.
- Create `crates/webui/frontend/src/hooks/useEditor.ts` — load-for-edit + commit/fork invokers, dirty-state.
- Create `crates/webui/frontend/src/components/CommitResultBanner.tsx` — shows the new session id + copyable `claude --resume` command after a commit/fork.
- Modify `crates/webui/frontend/src/components/Transcript.tsx` — per-turn "Edit" and "Fork from here" actions; inline edit textarea; wire to the editor hook.

---

## Invariants the commit pipeline guarantees (from spec §3)

1. Linear `parentUuid` chain: each event's parent = previous event's new uuid; first = null.
2. Unique regenerated event UUIDs on every commit.
3. Tool pairing: every assistant `tool_use` has a following user `tool_result` with matching `tool_use_id`; orphans repaired.
4. Tool-result adjacency preserved (the `tool_result` user line immediately follows its `tool_use` assistant line).
5. Consistent identity: every line gets the new `sessionId`, target `cwd`, target `gitBranch`.
6. A trailing `last-prompt` line points at the new leaf.
7. Roles/shapes unchanged from source (we don't alter message structure except the edited text).

---

## Task 1: Raw line model + active-path resolution

**Files:**
- Create: `crates/core/src/session/raw.rs`
- Modify: `crates/core/src/session/mod.rs` (add `pub mod raw;`)
- Test: inline `#[cfg(test)]` in `raw.rs`

- [ ] **Step 1: Register the module**

In `crates/core/src/session/mod.rs`, add to the module list at the top:

```rust
pub mod edit;
pub mod event;
pub mod parse;
pub mod raw;
pub mod render;
pub mod summary;
pub mod tree;
```

(`edit` is created in Task 2; adding it now is fine because Rust resolves modules at compile time and Task 2 lands before the first `cargo build`. If building between tasks, temporarily omit `pub mod edit;` until Task 2.)

- [ ] **Step 2: Write the failing test**

Create `crates/core/src/session/raw.rs`:

```rust
use std::path::Path;

use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

/// One physical line of a session jsonl, kept as raw JSON so no fields are lost
/// on a read→write round-trip.
#[derive(Debug, Clone)]
pub struct RawLine {
    pub value: Value,
    pub uuid: Option<Uuid>,
    pub parent: Option<Uuid>,
    pub kind: String,
}

impl RawLine {
    fn from_value(value: Value) -> Self {
        let uuid = value
            .get("uuid")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok());
        let parent = value
            .get("parentUuid")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok());
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Self { value, uuid, parent, kind }
    }
}

/// Read every line of a session file as raw JSON. Blank/unparseable lines are skipped.
pub fn read_raw(path: &Path) -> Result<Vec<RawLine>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            out.push(RawLine::from_value(v));
        }
    }
    Ok(out)
}

/// Resolve the active leaf: the last line in file order that has a uuid.
/// (Mirrors `tree::resolve_leaf` — the append-only tip is the true active node.)
pub fn resolve_leaf(lines: &[RawLine]) -> Option<Uuid> {
    lines.iter().rev().find_map(|l| l.uuid)
}

/// Return references to the lines on the active path, oldest first, by walking
/// `parent` from the leaf. Lines without a uuid (metadata) are not on the path.
pub fn active_path(lines: &[RawLine]) -> Vec<&RawLine> {
    let Some(leaf) = resolve_leaf(lines) else {
        return Vec::new();
    };
    let mut by_uuid = std::collections::HashMap::new();
    for l in lines {
        if let Some(u) = l.uuid {
            by_uuid.insert(u, l);
        }
    }
    let mut path = Vec::new();
    let mut cursor = Some(leaf);
    let mut guard = 0u32;
    while let Some(u) = cursor {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        let Some(l) = by_uuid.get(&u) else { break };
        path.push(*l);
        cursor = l.parent;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(uuid: &str, parent: Option<&str>, kind: &str) -> String {
        let p = parent
            .map(|p| format!("\"{}\"", p))
            .unwrap_or_else(|| "null".to_string());
        format!(
            r#"{{"type":"{}","uuid":"{}","parentUuid":{},"message":{{"role":"user","content":"x"}}}}"#,
            kind, uuid, p
        )
    }

    fn write_tmp(name: &str, body: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("cm-raw-{}-{}.jsonl", name, std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn active_path_walks_parents_from_last_uuid() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        // a -> b -> c, plus a trailing metadata line with no uuid
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, Some(b), "user"),
            r#"{"type":"last-prompt","leafUuid":"00000000-0000-0000-0000-0000000000c3","lastPrompt":"x"}"#
        );
        let path = write_tmp("walk", &body);
        let lines = read_raw(&path).unwrap();
        let active = active_path(&lines);
        let ids: Vec<String> = active.iter().filter_map(|l| l.uuid.map(|u| u.to_string())).collect();
        assert_eq!(ids, vec![a.to_string(), b.to_string(), c.to_string()]);
        std::fs::remove_file(&path).ok();
    }
}
```

- [ ] **Step 3: Run the test to verify it fails to compile then passes once written**

Run: `cargo test -p claude-manager-core raw:: 2>&1 | tail -20`
Expected: `test session::raw::tests::active_path_walks_parents_from_last_uuid ... ok`

(If `pub mod edit;` was added in Step 1 before Task 2 exists, the crate won't compile — temporarily comment it out, run this test, then restore in Task 2.)

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/session/raw.rs crates/core/src/session/mod.rs
git commit -m "feat(core): raw-line session model + active-path resolution"
```

---

## Task 2: Path encoding helpers

**Files:**
- Modify: `crates/core/src/store/paths.rs`
- Test: inline `#[cfg(test)]` in `paths.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/core/src/store/paths.rs`:

```rust
    #[test]
    fn encode_then_session_path() {
        let dir = encode_project_dir("/Users/zillydev/my/Dev/Personal/claude-manager");
        assert_eq!(dir, "-Users-zillydev-my-Dev-Personal-claude-manager");

        let root = std::path::Path::new("/tmp/.claude");
        let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap();
        let p = session_file_path(root, "/Users/x/proj", id);
        assert_eq!(
            p,
            std::path::PathBuf::from("/tmp/.claude/projects/-Users-x-proj/00000000-0000-0000-0000-0000000000aa.jsonl")
        );
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p claude-manager-core paths::tests::encode_then_session_path 2>&1 | tail -15`
Expected: FAIL — `cannot find function encode_project_dir`.

- [ ] **Step 3: Implement the helpers**

Add to `crates/core/src/store/paths.rs` (before the `#[cfg(test)]` block):

```rust
use uuid::Uuid;

/// Encode a real cwd into Claude Code's project directory name (replace `/` with `-`).
/// This is the inverse Claude Code applies; it is lossy for paths containing real
/// `-`, but matches CC's own encoding so the resulting dir is the one CC reads.
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.replace('/', "-")
}

/// Full path to a session file for `cwd`/`id` under `<root>/projects/<encoded>/<id>.jsonl`.
pub fn session_file_path(root: &Path, cwd: &str, id: Uuid) -> PathBuf {
    projects_dir(root)
        .join(encode_project_dir(cwd))
        .join(format!("{}.jsonl", id))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p claude-manager-core paths:: 2>&1 | tail -15`
Expected: all `paths::tests::*` PASS including `encode_then_session_path`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/store/paths.rs
git commit -m "feat(core): project-dir encode + session-file-path helpers"
```

---

## Task 3: Commit pipeline — validate + write a flattened session

**Files:**
- Create: `crates/core/src/session/edit.rs`
- Test: inline `#[cfg(test)]` in `edit.rs`

- [ ] **Step 1: Write the module with the core types and a failing test**

Create `crates/core/src/session/edit.rs`:

```rust
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::raw::RawLine;
use crate::store::paths;

/// A problem detected in a candidate event list. `auto_repaired` ones are fixed
/// by `commit`; the list is returned so the UI can show what changed.
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

/// Message-bearing line types we keep in a flattened session. Everything else
/// (file-history-snapshot, system turn_duration, ai-title, permission-mode, …)
/// is dropped; we synthesize the minimal envelope ourselves.
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

/// Validate tool pairing over an ordered slice of message values. Pure; used by
/// the UI pre-commit and by `commit` to drive repairs.
pub fn validate(values: &[Value]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut open: Vec<String> = Vec::new(); // tool_use ids awaiting a result
    for v in values {
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "assistant" {
            // Any still-open tool_use that wasn't answered before the next
            // assistant turn is dangling.
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

/// Write `path_values` (the chosen, ordered message lines as raw JSON) as a new
/// flattened, self-consistent session file. Never touches any existing file.
pub fn commit(root: &Path, path_lines: &[&RawLine], opts: &CommitOpts) -> Result<CommitResult> {
    // 1. Keep only message-bearing lines (drop metadata noise).
    let mut values: Vec<Value> = path_lines
        .iter()
        .filter(|l| is_message_line(&l.kind))
        .map(|l| l.value.clone())
        .collect();

    // 2. Tool-pairing repair: synthesize a placeholder tool_result for any
    //    dangling tool_use; drop orphan tool_results.
    let repairs = repair_tool_pairing(&mut values);

    // 3. Regenerate uuids + relink parentUuid into a linear chain; stamp identity.
    let new_session = Uuid::new_v4();
    let mut prev: Option<Uuid> = None;
    for v in values.iter_mut() {
        let new_uuid = Uuid::new_v4();
        set_str(v, "uuid", &new_uuid.to_string());
        match prev {
            Some(p) => set_str(v, "parentUuid", &p.to_string()),
            None => {
                v.as_object_mut().unwrap().insert("parentUuid".into(), Value::Null);
            }
        }
        set_str(v, "sessionId", &new_session.to_string());
        set_str(v, "cwd", &opts.target_cwd);
        if let Some(b) = &opts.git_branch {
            set_str(v, "gitBranch", b);
        }
        prev = Some(new_uuid);
    }

    // 4. Build the output: message lines, a trailing last-prompt bookmark, and
    //    an optional custom-title.
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

    // 5. Write to <root>/projects/<encoded cwd>/<new_session>.jsonl.
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

/// Set the plain-text content of a user line (or the first text block of an
/// assistant line) identified by original uuid. Used by edit-text.
pub fn set_message_text(line: &mut RawLine, text: &str) {
    let v = &mut line.value;
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    if kind == "user" {
        // user content is a string OR array of blocks
        match v.pointer("/message/content") {
            Some(Value::String(_)) => {
                if let Some(obj) = v.pointer_mut("/message").and_then(Value::as_object_mut) {
                    obj.insert("content".into(), Value::String(text.to_string()));
                }
            }
            _ => {
                if let Some(obj) = v.pointer_mut("/message").and_then(Value::as_object_mut) {
                    obj.insert("content".into(), Value::String(text.to_string()));
                }
            }
        }
    } else if kind == "assistant" {
        if let Some(blocks) = v.pointer_mut("/message/content").and_then(Value::as_array_mut) {
            for b in blocks.iter_mut() {
                if b.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(o) = b.as_object_mut() {
                        o.insert("text".into(), Value::String(text.to_string()));
                    }
                    return;
                }
            }
            // no text block: prepend one
            blocks.insert(0, json!({"type":"text","text":text}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::raw::{read_raw, active_path};

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
        // no matching tool_result for T1 → dangling; a stray tool_result T9 → orphan
        let user = json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"T9","content":"x"}]}});
        let v = validate(&[assistant, user]);
        assert!(v.iter().any(|x| x.kind == "dangling_tool_use" && x.detail == "T1"));
        assert!(v.iter().any(|x| x.kind == "orphan_tool_result" && x.detail == "T9"));
    }

    #[test]
    fn commit_produces_linear_resolvable_session() {
        // a(user) -> b(assistant) -> c(user)
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

        // Reload the written file and verify the chain.
        let written = read_raw(std::path::Path::new(&res.new_path)).unwrap();
        let msgs: Vec<&super::RawLine> =
            written.iter().filter(|l| l.kind == "user" || l.kind == "assistant").collect();
        assert_eq!(msgs.len(), 3);
        // first parent null, each subsequent parent = previous uuid
        assert!(msgs[0].parent.is_none());
        assert_eq!(msgs[1].parent, msgs[0].uuid);
        assert_eq!(msgs[2].parent, msgs[1].uuid);
        // sessionId stamped + lossless field preserved
        assert_eq!(msgs[0].value.get("sessionId").and_then(|v| v.as_str()), Some(res.new_session_id.as_str()));
        assert_eq!(msgs[0].value.get("version").and_then(|v| v.as_str()), Some("keep-me"));
        assert_eq!(msgs[0].value.get("cwd").and_then(|v| v.as_str()), Some("/tmp/proj"));
        // trailing last-prompt resolves to the final message
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
        // written file must now pass validation (no dangling)
        let written = read_raw(std::path::Path::new(&res.new_path)).unwrap();
        let vals: Vec<serde_json::Value> = written.iter()
            .filter(|l| l.kind=="user"||l.kind=="assistant").map(|l| l.value.clone()).collect();
        assert!(validate(&vals).is_empty());
        std::fs::remove_file(&src).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p claude-manager-core edit:: 2>&1 | tail -25`
Expected: `validate_flags_dangling_and_orphan`, `commit_produces_linear_resolvable_session`, `commit_repairs_dangling_tool_use` all PASS.

- [ ] **Step 3: Confirm the whole core crate still builds + tests green**

Run: `cargo test -p claude-manager-core 2>&1 | tail -5`
Expected: all tests pass (existing 20 + the 3 new ones).

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/session/edit.rs crates/core/src/session/mod.rs
git commit -m "feat(core): commit pipeline — flatten, relink, tool-pairing repair, lossless rewrite"
```

---

## Task 4: Phase 0 gate — verify a generated session resumes in real Claude Code

**Files:**
- Modify: `crates/tui/src/main.rs` (add a hidden `--fork-test` dev flag)

This is the **gate**: until a fully regenerated file resumes correctly, do not build the UI on top. We add a tiny CLI path to generate a forked copy of a real session so it can be resumed manually.

- [ ] **Step 1: Add a dev flag to the TUI CLI**

In `crates/tui/src/main.rs`, add to the `Cli` struct:

```rust
    /// Dev: fork an existing session file to a new flattened session and print the new id. Then exit.
    #[arg(long)]
    fork_test: Option<std::path::PathBuf>,
```

And near the top of `main()` after `root` is resolved, before the `--print` block:

```rust
    if let Some(src) = cli.fork_test.clone() {
        use claude_manager_core::session::raw::{read_raw, active_path};
        use claude_manager_core::session::edit::{commit, CommitOpts};
        let lines = read_raw(&src)?;
        let path = active_path(&lines);
        // derive target cwd from the source events
        let cwd = lines
            .iter()
            .find_map(|l| l.value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| ".".to_string());
        let git_branch = lines
            .iter()
            .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
        let opts = CommitOpts { target_cwd: cwd, git_branch, custom_title: Some("fork-test".into()) };
        let res = commit(&root, &path, &opts)?;
        println!("new session: {}", res.new_session_id);
        println!("path: {}", res.new_path);
        println!("repairs: {}", res.repairs.len());
        println!("resume with: claude --resume {}", res.new_session_id);
        return Ok(());
    }
```

- [ ] **Step 2: Build and generate a fork of a real session**

```bash
cargo build -p claude-manager-tui 2>&1 | tail -3
# pick a small real session to fork (adjust path):
SRC=$(ls -S ~/.claude/projects/-Users-zillydev-my-Dev-Personal-claude-manager/*.jsonl | tail -1)
./target/debug/claude-manager --fork-test "$SRC"
```

Expected: prints `new session: <uuid>`, a `path:` under the same project dir, and a `claude --resume <uuid>` line.

- [ ] **Step 3: MANUAL — verify resume reconstructs context**

In a terminal in `/Users/zillydev/my/Dev/Personal/claude-manager`:

```bash
claude --resume <the-new-uuid>
```

Then ask: `what was the last thing we discussed?`

Expected: Claude answers from the forked transcript's content (proves the regenerated file is accepted and context is reconstructed). If it errors or shows an empty/blank conversation, **stop and record what failed** — the minimal-envelope assumption is wrong and Tasks 5+ must wait until the envelope is corrected (compare a working CC-written file field-by-field against the generated one).

- [ ] **Step 4: Record the outcome in the spec**

Append a short "Phase 0 result" note to `docs/superpowers/specs/2026-05-30-session-editor-design.md` (PASS, plus any envelope fields that turned out to be required). Commit:

```bash
git add crates/tui/src/main.rs docs/superpowers/specs/2026-05-30-session-editor-design.md
git commit -m "chore(tui): --fork-test dev flag; record Phase 0 resume-acceptance result"
```

---

## Task 5: WebUI commands — open_for_edit, commit_session, fork_session

**Files:**
- Modify: `crates/webui/src/commands.rs`
- Modify: `crates/webui/src/lib.rs`

- [ ] **Step 1: Add an editable-session DTO + commands**

Append to `crates/webui/src/commands.rs`:

```rust
use claude_manager_core::session::edit::{commit, CommitOpts, CommitResult};
use claude_manager_core::session::raw::{active_path, read_raw, RawLine};
use serde::Serialize;
use std::path::PathBuf;

/// One editable turn surfaced to the UI. `index` is the position on the active
/// path (used as the fork point and the edit target). `text` is the primary
/// editable text (user message text, or the assistant's first text block).
#[derive(Serialize)]
pub struct EditableTurn {
    pub index: usize,
    pub uuid: String,
    pub role: String,
    pub text: String,
}

#[derive(Serialize)]
pub struct EditableSession {
    pub source_path: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub turns: Vec<EditableTurn>,
}

fn line_role(l: &RawLine) -> &str {
    &l.kind
}

fn line_text(l: &RawLine) -> String {
    let content = l.value.pointer("/message/content");
    if let Some(s) = content.and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(arr) = content.and_then(|v| v.as_array()) {
        for b in arr {
            if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                    return t.to_string();
                }
            }
        }
    }
    String::new()
}

#[tauri::command]
pub fn open_for_edit(file_path: String) -> Result<EditableSession, String> {
    let path = PathBuf::from(&file_path);
    let lines = read_raw(&path).map_err(|e| e.to_string())?;
    let active = active_path(&lines);
    let cwd = active
        .iter()
        .find_map(|l| l.value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    let git_branch = active
        .iter()
        .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let turns = active
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == "user" || l.kind == "assistant")
        .map(|(index, l)| EditableTurn {
            index,
            uuid: l.uuid.map(|u| u.to_string()).unwrap_or_default(),
            role: line_role(l).to_string(),
            text: line_text(l),
        })
        .collect();
    Ok(EditableSession { source_path: file_path, cwd, git_branch, turns })
}

/// Commit an edit: set the text of the turn at `edit_index` (if provided), then
/// write the full active path as a new flattened session.
#[tauri::command]
pub fn commit_session(
    file_path: String,
    edit_index: Option<usize>,
    new_text: Option<String>,
    custom_title: Option<String>,
) -> Result<CommitResult, String> {
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let path = PathBuf::from(&file_path);
    let mut lines = read_raw(&path).map_err(|e| e.to_string())?;

    // Resolve active path indices, then optionally edit one line's text.
    let active_uuids: Vec<uuid::Uuid> = active_path(&lines).iter().filter_map(|l| l.uuid).collect();
    if let (Some(idx), Some(text)) = (edit_index, new_text.as_ref()) {
        // map active index → message-turn → underlying line by uuid
        let msg_uuids: Vec<uuid::Uuid> = active_path(&lines)
            .iter()
            .filter(|l| l.kind == "user" || l.kind == "assistant")
            .filter_map(|l| l.uuid)
            .collect();
        if let Some(target) = msg_uuids.get(idx).copied() {
            if let Some(line) = lines.iter_mut().find(|l| l.uuid == Some(target)) {
                claude_manager_core::session::edit::set_message_text(line, text);
            }
        }
    }

    // Rebuild active path refs after the in-place text edit.
    let by_uuid: std::collections::HashMap<uuid::Uuid, &RawLine> =
        lines.iter().filter_map(|l| l.uuid.map(|u| (u, l))).collect();
    let path_refs: Vec<&RawLine> = active_uuids.iter().filter_map(|u| by_uuid.get(u).copied()).collect();

    let cwd = path_refs
        .iter()
        .find_map(|l| l.value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string());
    let git_branch = path_refs
        .iter()
        .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let opts = CommitOpts { target_cwd: cwd, git_branch, custom_title };
    commit(&root, &path_refs, &opts).map_err(|e| e.to_string())
}

/// Fork: write the prefix `root..=fork_index` (by active message-turn index) as
/// a new session.
#[tauri::command]
pub fn fork_session(
    file_path: String,
    fork_index: usize,
    custom_title: Option<String>,
) -> Result<CommitResult, String> {
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let path = PathBuf::from(&file_path);
    let lines = read_raw(&path).map_err(|e| e.to_string())?;
    let active = active_path(&lines);

    // Find the underlying-path cutoff: include every line up to and including
    // the line whose message-turn index == fork_index.
    let mut msg_seen = 0usize;
    let mut cutoff = active.len();
    for (i, l) in active.iter().enumerate() {
        if l.kind == "user" || l.kind == "assistant" {
            if msg_seen == fork_index {
                cutoff = i + 1;
                break;
            }
            msg_seen += 1;
        }
    }
    let prefix: Vec<&RawLine> = active.into_iter().take(cutoff).collect();
    let cwd = prefix
        .iter()
        .find_map(|l| l.value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string());
    let git_branch = prefix
        .iter()
        .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let opts = CommitOpts { target_cwd: cwd, git_branch, custom_title };
    commit(&root, &prefix, &opts).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Register the handlers**

In `crates/webui/src/lib.rs`, extend the `generate_handler!` list:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::open_session,
            commands::stop_watching,
            commands::search_transcripts,
            commands::open_for_edit,
            commands::commit_session,
            commands::fork_session,
        ])
```

- [ ] **Step 3: Build the webui backend**

Run: `cargo build -p claude-manager-webui 2>&1 | tail -5`
Expected: `Finished` with no errors.

- [ ] **Step 4: Commit**

```bash
git add crates/webui/src/commands.rs crates/webui/src/lib.rs
git commit -m "feat(webui): open_for_edit, commit_session, fork_session commands"
```

---

## Task 6: Frontend — types + editor hook

**Files:**
- Modify: `crates/webui/frontend/src/types.ts`
- Create: `crates/webui/frontend/src/hooks/useEditor.ts`

- [ ] **Step 1: Add types**

Append to `crates/webui/frontend/src/types.ts`:

```typescript
export interface EditableTurn {
  index: number;
  uuid: string;
  role: string;
  text: string;
}

export interface EditableSession {
  source_path: string;
  cwd: string;
  git_branch: string | null;
  turns: EditableTurn[];
}

export interface Violation {
  kind: string;
  detail: string;
}

export interface CommitResult {
  new_session_id: string;
  new_path: string;
  repairs: Violation[];
}
```

- [ ] **Step 2: Create the editor hook**

Create `crates/webui/frontend/src/hooks/useEditor.ts`:

```typescript
import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CommitResult } from "../types";

export function useEditor() {
  const [result, setResult] = useState<CommitResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const commitEdit = useCallback(
    async (filePath: string, editIndex: number, newText: string) => {
      setBusy(true);
      setError(null);
      try {
        const res = await invoke<CommitResult>("commit_session", {
          filePath,
          editIndex,
          newText,
          customTitle: null,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    []
  );

  const fork = useCallback(
    async (filePath: string, forkIndex: number) => {
      setBusy(true);
      setError(null);
      try {
        const res = await invoke<CommitResult>("fork_session", {
          filePath,
          forkIndex,
          customTitle: null,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    []
  );

  const dismiss = useCallback(() => setResult(null), []);

  return { result, busy, error, commitEdit, fork, dismiss };
}
```

- [ ] **Step 3: Type-check**

Run: `npm --prefix crates/webui/frontend run build 2>&1 | tail -5`
Expected: builds with no TypeScript errors.

- [ ] **Step 4: Commit**

```bash
git add crates/webui/frontend/src/types.ts crates/webui/frontend/src/hooks/useEditor.ts
git commit -m "feat(frontend): editor types + useEditor hook"
```

---

## Task 7: Frontend — CommitResultBanner

**Files:**
- Create: `crates/webui/frontend/src/components/CommitResultBanner.tsx`

- [ ] **Step 1: Create the banner**

Create `crates/webui/frontend/src/components/CommitResultBanner.tsx`:

```tsx
import type { CommitResult } from "../types";

export default function CommitResultBanner({
  result,
  onDismiss,
}: {
  result: CommitResult;
  onDismiss: () => void;
}) {
  const cmd = `claude --resume ${result.new_session_id}`;
  return (
    <div className="px-4 py-3 border-b border-[var(--color-accent)] bg-[var(--color-surface)] text-sm">
      <div className="flex items-center justify-between">
        <span className="text-[var(--color-assistant)]">
          ✓ New session created
        </span>
        <button
          onClick={onDismiss}
          className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] text-xs"
        >
          dismiss
        </button>
      </div>
      <div className="mt-1 flex items-center gap-2">
        <code className="text-xs bg-[var(--color-bg)] px-2 py-1 rounded border border-[var(--color-border)] flex-1 overflow-x-auto">
          {cmd}
        </code>
        <button
          onClick={() => navigator.clipboard.writeText(cmd)}
          className="text-xs px-2 py-1 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)]"
        >
          copy
        </button>
      </div>
      {result.repairs.length > 0 && (
        <div className="mt-1 text-xs text-[var(--color-system)]">
          {result.repairs.length} auto-repair(s):{" "}
          {result.repairs.map((r) => r.kind).join(", ")}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `npm --prefix crates/webui/frontend run build 2>&1 | tail -5`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/components/CommitResultBanner.tsx
git commit -m "feat(frontend): commit-result banner with copyable resume command"
```

---

## Task 8: Frontend — wire Edit + Fork into the transcript

**Files:**
- Modify: `crates/webui/frontend/src/components/Transcript.tsx`

The transcript currently renders `RenderedTurn`s. We add, per user/assistant turn: an "Edit" affordance (inline textarea + Save → `commitEdit`) and a "Fork from here" button (→ `fork`). We map a rendered turn to its **active-path message index** by counting user/assistant turns in order (this matches the backend's message-turn indexing, since both skip system/summary turns).

- [ ] **Step 1: Import the hook + banner and track edit state**

At the top of `crates/webui/frontend/src/components/Transcript.tsx`, add imports:

```tsx
import { useEditor } from "../hooks/useEditor";
import CommitResultBanner from "./CommitResultBanner";
```

In the `Transcript` component body (alongside the existing `useState` calls), add:

```tsx
  const editor = useEditor();
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
```

- [ ] **Step 2: Render the banner above the turns**

Immediately inside the turns scroll container (before the `.map`), add:

```tsx
        {editor.result && (
          <CommitResultBanner result={editor.result} onDismiss={editor.dismiss} />
        )}
        {editor.error && (
          <div className="text-[var(--color-error)] text-xs mb-2">{editor.error}</div>
        )}
```

- [ ] **Step 3: Compute message index + pass actions to Turn**

Replace the existing turns `.map(...)` call with one that threads a running message index and the action callbacks. Find:

```tsx
        {visibleTurns.map((turn, i) => (
          <Turn key={i} turn={turn} />
        ))}
```

Replace with:

```tsx
        {(() => {
          let msgIdx = -1;
          return visibleTurns.map((turn, i) => {
            const isMsg = turn.type === "user" || turn.type === "assistant";
            if (isMsg) msgIdx += 1;
            const thisMsgIdx = msgIdx;
            return (
              <Turn
                key={i}
                turn={turn}
                msgIndex={isMsg ? thisMsgIdx : null}
                editing={editingIndex === thisMsgIdx && isMsg}
                draft={draft}
                onStartEdit={(text) => {
                  setEditingIndex(thisMsgIdx);
                  setDraft(text);
                }}
                onChangeDraft={setDraft}
                onCancelEdit={() => setEditingIndex(null)}
                onSaveEdit={async () => {
                  await editor.commitEdit(session.id ? sourcePath : sourcePath, thisMsgIdx, draft);
                  setEditingIndex(null);
                }}
                onFork={() => editor.fork(sourcePath, thisMsgIdx)}
              />
            );
          });
        })()}
```

- [ ] **Step 4: Thread `sourcePath` into Transcript**

The commit/fork commands need the session's file path. The `RenderedSession` does not currently carry it. Add `source_path` to the render DTO:

In `crates/core/src/session/render.rs`, find the `RenderedSession` struct and add a field:

```rust
    pub source_path: String,
```

and in `render_session(session: &Session) -> RenderedSession`, set it where the struct is constructed:

```rust
        source_path: session.file_path.to_string_lossy().into_owned(),
```

In `crates/webui/frontend/src/types.ts`, add to `RenderedSession`:

```typescript
  source_path: string;
```

Then in `Transcript.tsx`, derive it near the top of the component:

```tsx
  const sourcePath = session.source_path;
```

- [ ] **Step 5: Extend the `Turn` component with edit/fork UI**

Update the `Turn` function signature and the user/assistant branches. Replace the `Turn` function's signature:

```tsx
function Turn({ turn }: { turn: RenderedTurn }) {
```

with:

```tsx
function Turn({
  turn,
  msgIndex,
  editing,
  draft,
  onStartEdit,
  onChangeDraft,
  onCancelEdit,
  onSaveEdit,
  onFork,
}: {
  turn: RenderedTurn;
  msgIndex: number | null;
  editing: boolean;
  draft: string;
  onStartEdit: (text: string) => void;
  onChangeDraft: (s: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onFork: () => void;
}) {
```

In the `user` case, replace the content `<div>` with an editable version + action row:

```tsx
    case "user":
      return (
        <div className="group">
          <div className="flex items-baseline gap-2 mb-1">
            <span className="text-xs text-[var(--color-text-dim)]">{ts}</span>
            <span className="font-medium text-[var(--color-user)]">user</span>
            {msgIndex !== null && (
              <TurnActions
                onEdit={() => onStartEdit(turn.text)}
                onFork={onFork}
              />
            )}
          </div>
          {editing ? (
            <EditBox
              draft={draft}
              onChange={onChangeDraft}
              onSave={onSaveEdit}
              onCancel={onCancelEdit}
            />
          ) : (
            <div className="pl-4 text-sm whitespace-pre-wrap break-words">{turn.text}</div>
          )}
        </div>
      );
```

In the `assistant` case, add the actions to the header row and an edit box for the first text block. Find the assistant header `<div className="flex items-baseline gap-2 mb-1">…</div>` and add, after the model span:

```tsx
            {msgIndex !== null && (
              <TurnActions
                onEdit={() => {
                  const firstText = turn.blocks.find((b) => b.type === "text");
                  onStartEdit(firstText && firstText.type === "text" ? firstText.text : "");
                }}
                onFork={onFork}
              />
            )}
```

And wrap the assistant block list so that when `editing`, an `EditBox` shows instead of the first text block. The minimal version: above the `<div className="pl-4">` blocks container, add:

```tsx
          {editing && (
            <EditBox
              draft={draft}
              onChange={onChangeDraft}
              onSave={onSaveEdit}
              onCancel={onCancelEdit}
            />
          )}
```

- [ ] **Step 6: Add the `TurnActions` and `EditBox` helpers**

Add near the other helper components at the bottom of `Transcript.tsx`:

```tsx
function TurnActions({ onEdit, onFork }: { onEdit: () => void; onFork: () => void }) {
  return (
    <span className="opacity-0 group-hover:opacity-100 transition-opacity flex gap-2 ml-2">
      <button
        onClick={onEdit}
        className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
      >
        edit
      </button>
      <button
        onClick={onFork}
        className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
        title="Create a new session ending at this message"
      >
        fork from here
      </button>
    </span>
  );
}

function EditBox({
  draft,
  onChange,
  onSave,
  onCancel,
}: {
  draft: string;
  onChange: (s: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="pl-4">
      <textarea
        value={draft}
        onChange={(e) => onChange(e.target.value)}
        className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-sm p-2 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)] font-mono"
        rows={Math.min(20, Math.max(3, draft.split("\n").length + 1))}
      />
      <div className="flex gap-2 mt-1">
        <button
          onClick={onSave}
          className="text-xs px-2 py-1 rounded border border-[var(--color-accent)] text-[var(--color-accent)]"
        >
          Save as new session
        </button>
        <button
          onClick={onCancel}
          className="text-xs px-2 py-1 rounded border border-[var(--color-border)] text-[var(--color-text-dim)]"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 7: Build the frontend + backend**

Run:
```bash
cargo build -p claude-manager-webui 2>&1 | tail -3
npm --prefix crates/webui/frontend run build 2>&1 | tail -5
```
Expected: both build clean (no TS errors, no Rust errors).

- [ ] **Step 8: MANUAL — end-to-end smoke**

```bash
make webui
```
- Open a session. Hover a user message → click **edit** → change text → **Save as new session**. Banner shows a new `claude --resume <id>` command.
- Hover a message → **fork from here** → banner shows a new session id.
- Confirm both new sessions appear in the sidebar (hit refresh ↻).
- Resume one (`claude --resume <id>`) and confirm context matches the edited/forked history.

- [ ] **Step 9: Commit**

```bash
git add crates/webui/frontend/src/components/Transcript.tsx crates/core/src/session/render.rs crates/webui/frontend/src/types.ts
git commit -m "feat(frontend): inline message edit + fork-from-here, copy-on-edit to new sessions"
```

---

## Self-Review notes (coverage vs spec)

- **Spec §3 invariants 1–6** → Task 3 `commit` (relink, regenerate uuids, tool-pairing repair, identity stamp, trailing last-prompt). Invariant 7 (shapes unchanged) holds because we edit only text/linkage on raw values.
- **Spec §2 copy-on-edit / flatten / guard-railed** → Task 3 writes a new file from the active path only; UI never writes jsonl.
- **Spec §4.1 edit message text** → Tasks 5–8.
- **Spec §4.8 fork (from latest / mid-session)** → `fork_session` (Task 5) + "fork from here" (Task 8). Fork-before-N, fork-and-edit, multi-fork, provenance → deferred to a Phase 1.5 follow-up (noted; not in this plan's tasks to keep it shippable).
- **Spec §6 Phase 0 gate** → Task 4 (the hard dependency for everything after).
- **Resume-command surfacing** → Task 7 banner.
- **Diff preview / dirty-state / validation panel** (spec §4.3, §4.7) → deferred to Phase 2 plan; not blocking the headline edit+fork features.

**Deferred to later plans (out of scope here):** structural edits (delete/insert/reorder/split/merge), undo/redo, cut/copy/paste, find&replace, cross-session moves, cross-project moves, trash/restore, export/import, block-level + raw-JSON editing, fork provenance UI.
