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
