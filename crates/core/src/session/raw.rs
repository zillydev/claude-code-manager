use std::collections::HashMap;
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
    pub fn from_value(value: Value) -> Self {
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

/// Return the path ending at `leaf`, oldest first, by walking `parent` back to
/// its tree root. Works for ANY node — including a message in an orphaned
/// pre-compaction tree — so a fork can be built from the fork point's own tree
/// rather than only the active path. Lines without a uuid (metadata) are skipped.
pub fn path_to(lines: &[RawLine], leaf: Uuid) -> Vec<&RawLine> {
    let mut by_uuid: HashMap<Uuid, &RawLine> = HashMap::with_capacity(lines.len());
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

/// Return references to the lines on the active path, oldest first, by walking
/// `parent` from the leaf. Lines without a uuid (metadata) are not on the path.
pub fn active_path(lines: &[RawLine]) -> Vec<&RawLine> {
    match resolve_leaf(lines) {
        Some(leaf) => path_to(lines, leaf),
        None => Vec::new(),
    }
}

/// The session's launch cwd: the first `cwd` field in FILE ORDER. This is the
/// directory the session was started in (and thus the project dir it lives in),
/// even if the agent later `cd`'d elsewhere or the active path begins
/// post-compaction in a different directory. Copy-on-edit writers use this so a
/// new session lands in the same project dir as its source and stays resumable
/// (Claude Code resolves `--resume <id>` within the current directory's project).
pub fn first_cwd(lines: &[RawLine]) -> Option<String> {
    lines
        .iter()
        .find_map(|l| l.value.get("cwd").and_then(Value::as_str).map(str::to_string))
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

    #[test]
    fn active_path_follows_appended_branch_after_fork() {
        // a -> b (abandoned), then a -> c (the active branch, appended last).
        // The leaf is the last line with a uuid (c), so the path is a -> c, not a -> b.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let body = format!(
            "{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, Some(a), "assistant"),
        );
        let path = write_tmp("fork", &body);
        let lines = read_raw(&path).unwrap();
        let ids: Vec<String> = active_path(&lines)
            .iter()
            .filter_map(|l| l.uuid.map(|u| u.to_string()))
            .collect();
        assert_eq!(ids, vec![a.to_string(), c.to_string()]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn first_cwd_is_the_file_order_launch_dir() {
        // The launch cwd is the FIRST cwd in file order — not whatever the agent
        // had cd'd into later (here the second line carries an inner dir).
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"cwd":"/launch","message":{"role":"user","content":"x"}}"#,
            "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","cwd":"/launch/inner","message":{"role":"user","content":"y"}}"#,
            "\n",
        );
        let path = write_tmp("firstcwd", body);
        let lines = read_raw(&path).unwrap();
        assert_eq!(first_cwd(&lines).as_deref(), Some("/launch"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn path_to_walks_the_targets_own_tree_not_the_active_one() {
        // Two disconnected trees (mirrors a compaction): tree 1 a->b, tree 2
        // (active) c->d. path_to(b) returns tree 1's path, so a fork from a
        // pre-compaction message yields that message's own history — while
        // active_path still resolves to the active tree.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let d = "00000000-0000-0000-0000-0000000000d4";
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, None, "user"),
            line(d, Some(c), "assistant"),
        );
        let path = write_tmp("pathto", &body);
        let lines = read_raw(&path).unwrap();
        let ids = |ls: Vec<&RawLine>| -> Vec<String> {
            ls.iter().filter_map(|l| l.uuid.map(|u| u.to_string())).collect()
        };
        assert_eq!(ids(path_to(&lines, b.parse().unwrap())), vec![a.to_string(), b.to_string()]);
        assert_eq!(ids(active_path(&lines)), vec![c.to_string(), d.to_string()]);
        std::fs::remove_file(&path).ok();
    }
}
