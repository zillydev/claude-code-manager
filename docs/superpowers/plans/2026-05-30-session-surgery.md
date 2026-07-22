# Session Surgery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `claude-code-manager surgery` CLI (extract + rewrite) and a `session-surgery` skill that summarize or prune a range of messages in a session's history, writing a NEW resumable session and leaving the original untouched.

**Architecture:** A new `core::session::surgery` module does all structural work — resolve the two message uuids on the active path, snap the range to whole-turn boundaries, render the range as text (for summarization), and splice (prune or insert-summary) then commit via the existing `core::session::edit::commit` (copy-on-edit, flatten, regenerate uuids, repair tool pairing). The TUI binary gains `surgery extract`/`surgery rewrite` clap subcommands that are a thin shell over `surgery`. The `session-surgery` SKILL.md orchestrates: extract → the running Claude writes a handoff-quality summary → rewrite.

**Tech Stack:** Rust (serde_json, uuid, anyhow, clap subcommands), a Markdown SKILL.md. Reuses `raw.rs`, `edit.rs`. Spec: `docs/superpowers/specs/2026-05-30-session-surgery-skill-design.md`.

---

## Existing APIs this plan builds on (do not reimplement)

- `core::session::raw`: `struct RawLine { pub value: serde_json::Value, pub uuid: Option<Uuid>, pub parent: Option<Uuid>, pub kind: String }`, `read_raw(&Path) -> Result<Vec<RawLine>>`, `active_path(&[RawLine]) -> Vec<&RawLine>` (oldest-first). `RawLine::from_value` is currently PRIVATE — Task 1 makes it public.
- `core::session::edit`: `commit(root: &Path, path_lines: &[&RawLine], opts: &CommitOpts) -> Result<CommitResult>`; `CommitOpts { target_cwd: String, git_branch: Option<String>, custom_title: Option<String> }`; `CommitResult { new_session_id: String, new_path: String, repairs: Vec<Violation> }`.
- `core::store::paths`: `projects_dir(&Path) -> PathBuf`, `default_claude_root() -> Option<PathBuf>`.

## File Structure

- Modify `crates/core/src/session/raw.rs` — make `RawLine::from_value` public.
- Create `crates/core/src/session/surgery.rs` — the surgery module (snap, extract, rewrite, resolve).
- Modify `crates/core/src/session/mod.rs` — add `pub mod surgery;`.
- Modify `crates/tui/src/main.rs` — clap subcommands `surgery extract` / `surgery rewrite`.
- Create `skills/session-surgery/SKILL.md` — the skill.

---

## Task 1: Make `RawLine::from_value` public + surgery module with turn-snapping

**Files:**
- Modify: `crates/core/src/session/raw.rs`
- Create: `crates/core/src/session/surgery.rs`
- Modify: `crates/core/src/session/mod.rs`

- [ ] **Step 1: Make `from_value` public**

In `crates/core/src/session/raw.rs`, find `fn from_value(value: Value) -> Self {` (inside `impl RawLine`) and change it to `pub fn from_value(value: Value) -> Self {`.

- [ ] **Step 2: Register the module**

In `crates/core/src/session/mod.rs`, add `pub mod surgery;` to the module list at the top (keep alphabetical-ish; placing after `summary` is fine).

- [ ] **Step 3: Create `crates/core/src/session/surgery.rs` with the snapping logic + test**

```rust
//! Session "surgery": summarize or prune a range of messages on the active path,
//! producing a new flattened session via `edit::commit` (copy-on-edit).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde_json::Value;
use uuid::Uuid;

use super::edit::{commit, CommitOpts, CommitResult};
use super::raw::{active_path, read_raw, RawLine};
use crate::store::paths;

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
/// user message is its own single-line turn; an assistant turn is a maximal run
/// of assistant lines plus the interleaved/following tool_result user lines,
/// until the next real user message.
fn turn_spans(active: &[&RawLine]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let n = active.len();
    let mut i = 0;
    while i < n {
        let start = i;
        if is_real_user(active[i]) {
            i += 1;
        } else {
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
    use super::*;

    fn write_tmp(name: &str, body: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("cm-surgery-{}-{}.jsonl", name, std::process::id()));
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
        // from = assistant B head, to = assistant B head: the turn is B + its
        // tool_result line (indices 1..=2).
        let b = "00000000-0000-0000-0000-0000000000b2".parse().unwrap();
        let (a_start, b_end) = snap_range(&active, b, b).unwrap();
        assert_eq!((a_start, b_end), (1, 2));
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
```

- [ ] **Step 4: Run the tests**

Run: `source "$HOME/.cargo/env" && cargo test -p claude-code-manager-core surgery:: 2>&1 | tail -20`
Expected: `snap_expands_assistant_turn_to_include_tool_result`, `snap_real_user_is_single_line_turn`, `snap_rejects_reversed_and_offpath` all PASS.

Run: `source "$HOME/.cargo/env" && cargo test -p claude-code-manager-core 2>&1 | tail -3`
Expected: whole crate green.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/session/raw.rs crates/core/src/session/surgery.rs crates/core/src/session/mod.rs
git commit -m "feat(core): surgery module — turn-snapped range resolution; pub RawLine::from_value"
```

---

## Task 2: Range rendering (`render_range_text`) + `extract` + `resolve_session_path`

**Files:**
- Modify: `crates/core/src/session/surgery.rs`

- [ ] **Step 1: Add the rendering + extract + resolve functions**

Insert these into `crates/core/src/session/surgery.rs`, after `snap_range` and before the `#[cfg(test)]` block:

```rust
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
```

- [ ] **Step 2: Add tests for extract + resolve**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `surgery.rs` (after the snap tests):

```rust
    #[test]
    fn extract_renders_range_with_roles() {
        let p = fixture();
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        let c = "00000000-0000-0000-0000-0000000000c4".parse().unwrap();
        let text = extract(&p, a, c).unwrap();
        // Range A..C covers user A, assistant B+tool_result, assistant C.
        assert!(text.contains("[user]"));
        assert!(text.contains("A"));
        assert!(text.contains("▸ Bash"));
        assert!(text.contains("→ result: out"));
        assert!(text.contains("C"));
        // D is after the range — must not appear as its own user turn.
        assert!(!text.contains("\nD\n"));
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
        let root = std::env::temp_dir(); // unused for the direct-file branch
        assert_eq!(resolve_session_path(&root, p.to_str().unwrap()).unwrap(), p);
        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 3: Run tests**

Run: `source "$HOME/.cargo/env" && cargo test -p claude-code-manager-core surgery:: 2>&1 | tail -20`
Expected: all surgery tests pass (6 total now).

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/session/surgery.rs
git commit -m "feat(core): surgery extract — render range text, session-path resolution"
```

---

## Task 3: `rewrite` (prune + summary) via the commit pipeline

**Files:**
- Modify: `crates/core/src/session/surgery.rs`

- [ ] **Step 1: Add the rewrite types + function**

Insert into `crates/core/src/session/surgery.rs`, after `resolve_session_path` and before `#[cfg(test)]`:

```rust
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

    let is_msg = |l: &&RawLine| l.kind == "user" || l.kind == "assistant";
    let messages_before = active.iter().filter(is_msg).count();
    let removed_bytes: usize = active[a..=b].iter().map(|l| l.value.to_string().len()).sum();

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

    if path_refs.iter().filter(|l| is_msg(l)).count() == 0 {
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
```

- [ ] **Step 2: Add rewrite tests**

Add these inside the `#[cfg(test)] mod tests` block in `surgery.rs`. They reuse the `fixture()` and `write_tmp` helpers; add `use super::super::edit::validate;` at the top of the test module if not present (it is not — add it next to `use super::*;`).

Update the test module's `use` lines to:
```rust
    use super::*;
    use crate::session::edit::validate;
```

Then add:
```rust
    #[test]
    fn prune_drops_the_range_and_revalidates_clean() {
        let p = fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-p-{}", std::process::id()));
        let b = "00000000-0000-0000-0000-0000000000b2".parse().unwrap();
        // prune just assistant B's turn (B + its tool_result) → A, C, D remain.
        // Active path has 5 user/assistant lines: A(user), B(assistant),
        // tool_result(user), C(assistant), D(user). Pruning B+tool_result leaves
        // A, C, D = 3 message lines. (We assert only the after-count; the exact
        // before-count is incidental.)
        let out = rewrite(&root, &p, b, b, RewriteMode::Prune, None).unwrap();
        assert_eq!(out.messages_after, 3);
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
        let c = "00000000-0000-0000-0000-0000000000c4".parse().unwrap();
        // summarize B..C (B turn + tool_result + C) → replaced by one summary line.
        let out = rewrite(&root, &p, b, c, RewriteMode::Summary, Some("did the thing")).unwrap();
        let written = read_raw(std::path::Path::new(&out.commit.new_path)).unwrap();
        let msgs: Vec<&RawLine> = written
            .iter()
            .filter(|l| l.kind == "user" || l.kind == "assistant")
            .collect();
        // A(user) + summary(user) + D(user) = 3 message lines
        assert_eq!(msgs.len(), 3);
        // the summary message carries the marker text
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
```

- [ ] **Step 3: Run tests**

Run: `source "$HOME/.cargo/env" && cargo test -p claude-code-manager-core surgery:: 2>&1 | tail -25`
Expected: all surgery tests pass (`prune_drops_the_range_and_revalidates_clean`, `summary_inserts_one_message`, `summary_requires_text` + the earlier ones).

Run: `source "$HOME/.cargo/env" && cargo test -p claude-code-manager-core 2>&1 | tail -3` — whole crate green.

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/session/surgery.rs
git commit -m "feat(core): surgery rewrite — prune/summary splice via commit pipeline"
```

---

## Task 4: CLI subcommands `surgery extract` / `surgery rewrite`

**Files:**
- Modify: `crates/tui/src/main.rs`

- [ ] **Step 1: Read the current main.rs**

Read `crates/tui/src/main.rs` fully. Note: it uses `#[derive(Parser)] struct Cli { root, print, fork_test }`, then `main()` resolves `root` and branches on `cli.fork_test` / `cli.print` before launching the TUI.

- [ ] **Step 2: Add clap subcommand types + a `command` field**

In `crates/tui/src/main.rs`, change the clap import line `use clap::Parser;` to:
```rust
use clap::{Parser, Subcommand};
```
Add `use uuid::Uuid;` to the imports near the top (with the other `use` lines).

Add a `command` field to the `Cli` struct (after `fork_test`):
```rust
    #[command(subcommand)]
    command: Option<Command>,
```

Add these enums after the `Cli` struct:
```rust
#[derive(Subcommand, Debug)]
enum Command {
    /// Rewrite session history to reduce context (summarize or prune a range).
    Surgery {
        #[command(subcommand)]
        op: SurgeryOp,
    },
}

#[derive(Subcommand, Debug)]
enum SurgeryOp {
    /// Print the messages in the range [from..to] (turn-snapped) for summarizing.
    Extract {
        /// Session file path or bare session id.
        #[arg(long)]
        session: String,
        /// Start message id (uuid).
        #[arg(long)]
        from: String,
        /// End message id (uuid).
        #[arg(long)]
        to: String,
    },
    /// Write a new session with the range pruned or replaced by a summary.
    Rewrite {
        #[arg(long)]
        session: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        /// "prune" or "summary".
        #[arg(long)]
        mode: String,
        /// File containing the summary text (required for --mode summary).
        #[arg(long)]
        summary_file: Option<PathBuf>,
    },
}
```

- [ ] **Step 3: Handle the subcommand in `main()`**

In `main()`, immediately after `root` is resolved (after the `.context("could not locate ~/.claude ...")?;` line) and BEFORE the `if let Some(src) = cli.fork_test` block, add:

```rust
    if let Some(Command::Surgery { op }) = &cli.command {
        use claude_code_manager_core::session::surgery::{self, RewriteMode};
        match op {
            SurgeryOp::Extract { session, from, to } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let a = Uuid::parse_str(from).context("invalid --from uuid")?;
                let b = Uuid::parse_str(to).context("invalid --to uuid")?;
                print!("{}", surgery::extract(&path, a, b)?);
            }
            SurgeryOp::Rewrite { session, from, to, mode, summary_file } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let a = Uuid::parse_str(from).context("invalid --from uuid")?;
                let b = Uuid::parse_str(to).context("invalid --to uuid")?;
                let m = match mode.as_str() {
                    "prune" => RewriteMode::Prune,
                    "summary" => RewriteMode::Summary,
                    other => anyhow::bail!("--mode must be 'prune' or 'summary', got '{}'", other),
                };
                let summary_text = match summary_file {
                    Some(f) => Some(std::fs::read_to_string(f).context("could not read --summary-file")?),
                    None => None,
                };
                let out = surgery::rewrite(&root, &path, a, b, m, summary_text.as_deref())?;
                println!("new session: {}", out.commit.new_session_id);
                println!("path: {}", out.commit.new_path);
                println!("resume with: claude --resume {}", out.commit.new_session_id);
                println!(
                    "messages: {} -> {}  (removed ~{} bytes)",
                    out.messages_before, out.messages_after, out.removed_bytes
                );
                if !out.commit.repairs.is_empty() {
                    println!("tool-pairing repairs: {}", out.commit.repairs.len());
                }
            }
        }
        return Ok(());
    }
```

- [ ] **Step 4: Build**

Run: `source "$HOME/.cargo/env" && cargo build -p claude-code-manager-tui 2>&1 | tail -10`
Expected: Finished, no errors. (A pre-existing `theme::thinking` dead-code warning may appear — leave it.)

- [ ] **Step 5: Manual smoke — extract + prune on a real session**

```bash
# pick a small real session
SRC=$(ls -S ~/.claude/projects/*/*.jsonl | tail -1)
# grab the first two message uuids on it
IDS=$(python3 -c "
import json,sys
ids=[]
for l in open('$SRC'):
    try: o=json.loads(l)
    except: continue
    if o.get('type') in ('user','assistant') and o.get('uuid'): ids.append(o['uuid'])
print(ids[0]); print(ids[1] if len(ids)>1 else ids[0])
")
A=$(echo "$IDS" | sed -n 1p); B=$(echo "$IDS" | sed -n 2p)
echo "session=$SRC  A=$A  B=$B"
./target/debug/claude-code-manager surgery extract --session "$SRC" --from "$A" --to "$B" | head -20
echo "--- rewrite prune ---"
./target/debug/claude-code-manager surgery rewrite --session "$SRC" --from "$A" --to "$B" --mode prune
```
Expected: `extract` prints the first turn(s) as `[role]` text; `rewrite` prints a new session id, path, `claude --resume <id>`, and a `messages: N -> M` line. Verify the new file is valid:
```bash
NEW=$(./target/debug/claude-code-manager surgery rewrite --session "$SRC" --from "$A" --to "$B" --mode prune | grep '^path:' | sed 's/^path: //')
python3 -c "import json;[json.loads(l) for l in open('$NEW') if l.strip()];print('valid jsonl')"
```

- [ ] **Step 6: Commit**

```bash
git add crates/tui/src/main.rs
git commit -m "feat(cli): surgery extract/rewrite subcommands"
```

---

## Task 5: The `session-surgery` skill

**Files:**
- Create: `skills/session-surgery/SKILL.md`

- [ ] **Step 1: Create the skill file**

Create `skills/session-surgery/SKILL.md` with exactly:

````markdown
---
name: session-surgery
description: Use when the user wants to shrink/compact a Claude Code session's context by summarizing or pruning a range of messages between two message ids (copied from the claude-code-manager webui's per-message "copy id" button). Produces a NEW resumable session via the claude-code-manager binary; never modifies the original. Trigger phrases include "summarize messages between <id> and <id>", "prune/cut this part of the session", "reduce context in session X", "compact this stretch".
---

# Session Surgery

You are rewriting the history of a Claude Code session so it uses less context, then handing the user a `claude --resume <id>` for a new, smaller session that picks up where the original left off.

**Two hard guarantees, both enforced by the tooling — never violate them:**
1. **Copy-on-edit.** Every operation writes a brand-new session file (new id). The original session file is read-only and is never modified. If anything goes wrong, the user loses nothing.
2. **You never hand-edit the jsonl.** All structural work goes through the `claude-code-manager` binary's `surgery` subcommands, which reuse the tested write pipeline (flatten, regenerate uuids, relink the parent chain, repair tool_use/tool_result pairing). Editing the jsonl by hand would corrupt those invariants.

## Background you need (mental model)

A Claude Code session is a `.jsonl` transcript at `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` — one JSON object per line, linked into a tree by `parentUuid`. On `claude --resume <id>`, Claude Code reconstructs the entire conversation **from that file**. This has been verified empirically: whatever is in the transcript becomes the resumed model's memory of the conversation. That is exactly why this works — we produce a new transcript with a denser middle, and the resumed agent treats the summary as real history.

**Context in real sessions is dominated by tool outputs** (file reads, bash/search dumps), not prose. A long exploration phase, a giant accidental dump, or a resolved debugging thread can be hundreds of KB. Surgery lets the user collapse *exactly that span* while keeping everything else verbatim — targeted, manual compaction (unlike Claude Code's untargeted auto-compaction).

## Inputs (parse from the user's request)

- **session** — a session id (e.g. `8cab6894-...`) or a full `.jsonl` path. The user typically copies the id from the webui detail panel's **session** row or from a `claude --resume <id>` line. If they don't give one and it's not obvious, ask.
- **from** and **to** — two **message ids** (uuids), copied with the webui's per-message **copy id** hover button. They bound the range to operate on. The range is **inclusive** and **snaps to whole-turn boundaries** automatically (picking an assistant message's id pulls in its tool calls and their results, so a turn is never half-cut). `from` must come at or before `to` on the conversation.
- **operation** — `summarize` (replace the range with one dense summary message) or `prune` (delete the range entirely). If the user didn't say which, infer from intent and confirm: summarize when the span has durable value worth keeping in condensed form; prune when it's pure noise.

## Locate the binary (once, up front)

```bash
BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-code-manager
[ -x "$BIN" ] || BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/debug/claude-code-manager
[ -x "$BIN" ] || { echo "build it first"; (cd /Users/zillydev/my/Dev/Personal/claude-manager && cargo build --release); BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-code-manager; }
```

## Step 0 — Safety check (ALWAYS do first)

A live Claude Code process that currently has the target session open will re-persist its file from memory — it can recreate the original even after you produce a new session. We never touch the original, but a live process resuming the *old* id is a footgun. Check:

```bash
grep -l "\"sessionId\":\"$SESSION_ID\"" ~/.claude/sessions/*.json 2>/dev/null
```

If there's a match, tell the user: "that session is currently open in a running Claude Code process — finish/close it before resuming the new session, or the live process may keep re-persisting the old transcript." Then proceed (it's still safe to read the file).

## Choosing the operation and confirming the range

Before rewriting, **always `extract` first and look at what you're about to compress.** Don't blindly trust the two ids — confirm the span is the right one.

- **summarize** when the span reached conclusions you want to keep: an exploration that landed on a design, a debugging thread that found the root cause, a long back-and-forth whose *outcome* matters but whose *verbosity* doesn't.
- **prune** when the span has no lasting value: an abandoned dead-end approach, an accidental huge file/dir dump, redundant repetition. Nothing in it is needed going forward.
- **Keep the tail verbatim.** Avoid including the most recent messages in the range — recent context is usually what the agent most needs intact. If `to` is at/near the end of the conversation, double-check with the user.

## Operation: SUMMARIZE

### 1. Extract the range
```bash
"$BIN" surgery extract --session "$SESSION" --from "$FROM" --to "$TO"
```
This prints the messages in the (turn-snapped, inclusive) range as readable text. Tool inputs/results are capped to keep your context bounded — that's fine, you're summarizing, not transcribing. Read it.

### 2. Write a HANDOFF-QUALITY summary
This is the part that determines whether the resumed agent keeps performing well. **A thin/vague summary degrades the agent — that is the main failure mode of this skill.** Write the summary like a briefing to a competent teammate who will continue this exact work with no other context. It must let them resume seamlessly.

Cover, concretely:
- **Goal** — what this span was trying to accomplish.
- **Decisions made, and WHY** — the durable conclusions and their rationale. The "why" is what prevents the resumed agent from re-litigating settled questions.
- **Current state** — where the work actually stands now (done / in progress / blocked).
- **Concrete artifacts — preserve these verbatim, never paraphrase:** file paths, function/type/variable names, commands run and their outcomes, ids (session/PR/issue/commit/message), config keys + values, URLs, error strings, version numbers.
- **Open threads** — what's unresolved, known bugs, deferred items.
- **Immediate next step** — the very next action to take.

Style: dense and factual. Drop pleasantries, restated questions, and the verbose tool outputs (those are the bytes you're reclaiming). Keep every identifier and decision. Aim for the shortest text that loses nothing actionable — a 50-message stretch often compresses to ~15–40 lines. When unsure whether a detail matters, keep it (it's far cheaper than a confused resumed agent).

**Anti-patterns (do not do):** "We explored several options and picked one." (Which? Why?) · dropping file paths or ids · summarizing away the error message that explains the fix · pasting raw tool output back in (defeats the purpose) · editorializing.

Save it to a temp file:
```bash
SUMMARY_FILE=$(mktemp)
cat > "$SUMMARY_FILE" <<'EOF'
<your dense handoff summary here>
EOF
```

### 3. Rewrite (writes the new session)
```bash
"$BIN" surgery rewrite --session "$SESSION" --from "$FROM" --to "$TO" --mode summary --summary-file "$SUMMARY_FILE"
```
The binary inserts your summary as a marked context message in place of the range, repairs any tool-pairing across the cut, and writes a new flattened session. It prints the new id, path, resume command, and a `messages: N -> M (removed ~X bytes)` line.

### 4. Report back to the user
Give them: the new session's `claude --resume <id>` command (copyable), the savings (`messages N→M`, ~X bytes), and a one-line note of what you summarized. Remind them the original is untouched.

## Operation: PRUNE

1. Safety check (Step 0).
2. Optionally `extract` to confirm the span is genuinely disposable.
3. Rewrite:
   ```bash
   "$BIN" surgery rewrite --session "$SESSION" --from "$FROM" --to "$TO" --mode prune
   ```
4. Report the new `claude --resume <id>`, the savings line, and what you removed. Original untouched.

## Handling errors from the binary

- **"from/to id ... is not on the active path"** — the uuid isn't a message on the current conversation path (maybe it's from an abandoned branch, or a metadata line, or a typo). Ask the user to re-copy the id from the webui, or list the path's message ids to help them pick.
- **"from id occurs after to id"** — the two ids are reversed; swap `--from` and `--to`.
- **"refusing: the result would contain no messages"** — the range covers the whole conversation; narrow it (you can't summarize-away everything).
- **"session '...' not found"** — the session id/path is wrong; confirm with the user (or pass the full `.jsonl` path).

## After surgery — suggest verifying

Encourage the user to resume the new session and sanity-check: `claude --resume <new-id>`, then ask the agent something that depends on the compressed span (e.g. "what's our current state and the next step?"). If it answers coherently, the summary did its job. If it's lost, the summary was too thin — re-run with a richer summary (the original is still there).
````

- [ ] **Step 2: Commit**

```bash
git add skills/session-surgery/SKILL.md
git commit -m "feat(skill): session-surgery — orchestrate summarize/prune via the CLI"
```

---

## Task 6: End-to-end manual verification

**Files:** none (verification only)

- [ ] **Step 1: Build release**

Run: `source "$HOME/.cargo/env" && cargo build --release 2>&1 | tail -3`

- [ ] **Step 2: MANUAL — summarize a real session and resume it**

1. In the webui (`make webui`), open a session with a verbose middle stretch. Use the per-message **copy id** buttons to copy two ids bounding that stretch.
2. Run the skill (or the CLI directly): summarize between the two ids.
3. Resume the printed new session: `claude --resume <new-id>`.
4. Ask the resumed agent something that depends on the summarized span (e.g. "what's our current state and next step?").

Expected: the agent answers coherently from the summary — it should know the decisions/state the summary preserved. The original session file is unchanged (confirm its size/mtime).

- [ ] **Step 3: Symlink the skill into ~/.claude/skills (optional, user choice)**

If the user wants the skill globally available:
```bash
ln -s /Users/zillydev/my/Dev/Personal/claude-manager/skills/session-surgery ~/.claude/skills/session-surgery
```

---

## Self-Review notes (coverage vs spec)

- **Spec: summarize + prune operations** → Task 3 `rewrite` (RewriteMode), Task 5 skill flows.
- **Spec: CLI subcommands reusing core::edit** → Task 3 (`commit`), Task 4 (`surgery extract/rewrite`).
- **Spec: turn-snapping** → Task 1 `snap_range`/`turn_spans`.
- **Spec: summary = marked user-role handoff note** → Task 3 (marker text), Task 5 (handoff-quality guidance).
- **Spec: copy-on-edit, original read-only** → `rewrite` only writes via `commit` to a new path; verified by Task 6 Step 2.
- **Spec: content-size delta reporting** → Task 3 `RewriteOutcome` + Task 4 print.
- **Spec: session id or path resolution** → Task 2 `resolve_session_path`.
- **Spec: extract caps tool text ~4000 chars** → Task 2 `TOOL_TEXT_CAP`/`cap`.
- **Spec: live-session safety check** → Task 5 skill safety section (informational; we never write the original).
- **Spec: refuse empty result / off-path / reversed** → Task 1 + Task 3 guards.
- **Spec: skill at skills/session-surgery/SKILL.md, symlinkable** → Task 5 + Task 6 Step 3.
- **Spec out-of-scope** (tool-output stripping, keep-only, batch) → not implemented; CLI structured to extend.
