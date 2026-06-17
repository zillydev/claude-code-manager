pub mod lineage;
pub mod paths;
pub mod watcher;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::session::parse::{parse_file, ParseOutcome};
use crate::session::summary::{summarize, SessionSummary};
use crate::session::{tree, Session};

/// Discover every session jsonl under `<root>/projects/`. Errors on
/// individual entries are swallowed (returned alongside successes).
pub struct Discovery {
    pub sessions: Vec<SessionSummary>,
    pub warnings: Vec<String>,
}

pub fn discover(root: &Path) -> Result<Discovery> {
    let projects = paths::projects_dir(root);
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();

    let read_dir = match fs::read_dir(&projects) {
        Ok(rd) => rd,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "could not read {}: {}",
                projects.display(),
                e
            ));
        }
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("read_dir entry: {}", e));
                continue;
            }
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let fallback_label = paths::fallback_label(&dir_name);

        let inner = match fs::read_dir(&path) {
            Ok(rd) => rd,
            Err(e) => {
                warnings.push(format!("{}: {}", path.display(), e));
                continue;
            }
        };

        // Accumulate this directory's sessions so we can resolve copy-lineage
        // (which needs every session's uuids) before pushing them.
        let mut dir_summaries: Vec<SessionSummary> = Vec::new();
        let mut dir_uuids: Vec<lineage::SessionUuids> = Vec::new();

        for child in inner {
            let child = match child {
                Ok(c) => c,
                Err(_) => continue,
            };
            let child_path = child.path();
            if child_path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = match child_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let id = match Uuid::parse_str(stem) {
                Ok(u) => u,
                Err(_) => continue, // skip non-UUID jsonl files
            };
            let mtime = child
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            match parse_file(&child_path) {
                Ok(ParseOutcome { events, .. }) => {
                    let project_label = events
                        .iter()
                        .find_map(|e| e.cwd().map(paths::label_from_cwd))
                        .unwrap_or_else(|| fallback_label.clone());
                    let summary = summarize(
                        &events,
                        id,
                        child_path.clone(),
                        project_label,
                        mtime,
                    );
                    let su = lineage::SessionUuids {
                        id,
                        head: events.iter().find_map(|e| e.uuid()),
                        uuids: events.iter().filter_map(|e| e.uuid()).collect(),
                        forked: summary.forked_from.is_some(),
                    };
                    dir_summaries.push(summary);
                    dir_uuids.push(su);
                }
                Err(e) => {
                    warnings.push(format!("parse {}: {}", child_path.display(), e));
                }
            }
        }

        // Resolve copy-lineage within this directory and stamp the summaries.
        // `dir_summaries[i]` and `dir_uuids[i]` are the same session (pushed
        // together), so the descendant's head — its branch point in the ancestor
        // — is `dir_uuids[i].head`.
        let links = lineage::copy_lineage(&dir_uuids);
        for (i, summary) in dir_summaries.iter_mut().enumerate() {
            if let Some((ancestor_id, overlap)) = links.get(&summary.id) {
                summary.copied_from = Some(ancestor_id.to_string());
                summary.copied_overlap = Some(*overlap);
                summary.copied_from_message = dir_uuids[i].head.map(|h| h.to_string());
            }
        }
        sessions.extend(dir_summaries);
    }

    // most-recent first
    sessions.sort_by(|a, b| b.mtime.cmp(&a.mtime));

    Ok(Discovery { sessions, warnings })
}

/// Fully read a single session file into memory.
pub fn open_session(file_path: PathBuf, project_label: String) -> Result<Session> {
    let id = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .context("file stem is not a UUID")?;
    let mtime = fs::metadata(&file_path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let outcome = parse_file(&file_path)?;
    let mut s = Session {
        project_label,
        id,
        file_path,
        events: outcome.events,
        leaf: None,
        mtime,
        bytes_read: outcome.bytes_read,
    };
    s.leaf = tree::resolve_leaf(&s.events);
    Ok(s)
}

/// Move a session file into the app trash (`<root>/.trash/claude-manager/`) and
/// write a `<name>.trashinfo` sidecar recording the original path and deletion
/// time so it can be restored later. Returns the trashed file's new path.
///
/// The trash dir lives under `<root>` but outside `<root>/projects/`, so trashed
/// sessions are not re-discovered by `discover`.
pub fn trash_session(root: &Path, file_path: &Path) -> Result<PathBuf> {
    let trash_dir = root.join(".trash").join("claude-manager");
    fs::create_dir_all(&trash_dir).with_context(|| {
        format!("could not create trash dir {}", trash_dir.display())
    })?;

    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    // Uniquify with a unix-second suffix so re-deleting a restored file (or two
    // files with the same stem) never clobbers an existing trash entry.
    let ts = Utc::now().timestamp();
    let dest = trash_dir.join(format!("{}.{}.jsonl", stem, ts));

    // Same filesystem (both under <root>), so rename is atomic and cheap.
    fs::rename(file_path, &dest).with_context(|| {
        format!("could not move {} to trash", file_path.display())
    })?;

    let info = serde_json::json!({
        "original_path": file_path.to_string_lossy(),
        "deleted_at": Utc::now().to_rfc3339(),
    });
    let info_path = dest.with_extension("jsonl.trashinfo");
    fs::write(&info_path, info.to_string()).with_context(|| {
        format!("could not write trashinfo {}", info_path.display())
    })?;

    Ok(dest)
}

/// Search for a query string across all session jsonl files. Returns the UUIDs
/// of sessions that contain at least one line matching `query` (case-insensitive).
/// This is O(total data on disk) — callers should debounce.
pub fn search_sessions(root: &Path, query: &str) -> Result<Vec<Uuid>> {
    use std::io::{BufRead, BufReader};

    let projects = paths::projects_dir(root);
    let query_lower = query.to_lowercase();
    let mut matches = Vec::new();

    let read_dir = fs::read_dir(&projects)?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let inner = match fs::read_dir(&path) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for child in inner.flatten() {
            let child_path = child.path();
            if child_path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = match child_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let id = match Uuid::parse_str(stem) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let file = match fs::File::open(&child_path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let reader = BufReader::new(file);
            let mut found = false;
            for line in reader.lines() {
                let Ok(line) = line else { continue };
                if line.to_lowercase().contains(&query_lower) {
                    found = true;
                    break;
                }
            }
            if found {
                matches.push(id);
            }
        }
    }

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_moves_file_and_writes_info() {
        // Build a fake root with a project file.
        let root = std::env::temp_dir().join(format!("cm-trash-{}", std::process::id()));
        let proj = root.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();
        let id = "00000000-0000-0000-0000-0000000000aa";
        let src = proj.join(format!("{}.jsonl", id));
        fs::write(&src, "{\"type\":\"user\"}\n").unwrap();

        let dest = trash_session(&root, &src).unwrap();

        // Original gone, trashed file present under .trash/claude-manager.
        assert!(!src.exists(), "source should be moved");
        assert!(dest.exists(), "trashed file should exist");
        assert!(dest.starts_with(root.join(".trash").join("claude-manager")));

        // trashinfo sidecar records the original path.
        let info_path = dest.with_extension("jsonl.trashinfo");
        assert!(info_path.exists(), "trashinfo should exist");
        let info: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&info_path).unwrap()).unwrap();
        assert_eq!(
            info.get("original_path").and_then(|v| v.as_str()),
            Some(src.to_string_lossy().as_ref())
        );

        fs::remove_dir_all(&root).ok();
    }

    fn write_session(dir: &std::path::Path, session_id: &str, lines: &[String]) {
        use std::io::Write;
        let p = dir.join(format!("{session_id}.jsonl"));
        let mut f = fs::File::create(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn discover_sets_copied_from_via_shared_uuid() {
        let tmp = std::env::temp_dir().join(format!("cm-lineage-test-{}", std::process::id()));
        let proj = tmp.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();

        // SHARED message uuid present in BOTH sessions.
        let shared = "55555555-0000-0000-0000-000000000005";
        // A (origin): own head A1, then the shared message.
        let a_id = "11111111-1111-1111-1111-111111111111";
        let a1 = "aaaaaaaa-0000-0000-0000-000000000001";
        write_session(
            &proj,
            a_id,
            &[
                format!(r#"{{"type":"user","uuid":"{a1}","parentUuid":null,"isSidechain":false,"message":{{"role":"user","content":"a-first"}},"timestamp":"2026-06-17T00:00:00Z"}}"#),
                format!(r#"{{"type":"user","uuid":"{shared}","parentUuid":"{a1}","isSidechain":false,"message":{{"role":"user","content":"shared"}},"timestamp":"2026-06-17T00:01:00Z"}}"#),
            ],
        );
        // B (copy): head = the shared message (its fork point), then a new message.
        let b_id = "22222222-2222-2222-2222-222222222222";
        let b2 = "bbbbbbbb-0000-0000-0000-000000000002";
        write_session(
            &proj,
            b_id,
            &[
                format!(r#"{{"type":"user","uuid":"{shared}","parentUuid":null,"isSidechain":false,"message":{{"role":"user","content":"shared"}},"timestamp":"2026-06-17T00:02:00Z"}}"#),
                format!(r#"{{"type":"user","uuid":"{b2}","parentUuid":"{shared}","isSidechain":false,"message":{{"role":"user","content":"b-new"}},"timestamp":"2026-06-17T00:03:00Z"}}"#),
            ],
        );

        let disc = discover(&tmp).unwrap();
        let a = disc.sessions.iter().find(|s| s.id.to_string() == a_id).unwrap();
        let b = disc.sessions.iter().find(|s| s.id.to_string() == b_id).unwrap();

        assert_eq!(b.copied_from.as_deref(), Some(a_id), "B should be copied from A");
        assert_eq!(b.copied_overlap, Some(1), "one shared message");
        assert_eq!(a.copied_from, None, "A is the origin");

        // B's branch point in A = B's head (its first message = the shared root).
        assert_eq!(b.copied_from_message.as_deref(), Some(shared), "B's branch point is its head");
        assert_eq!(a.copied_from_message, None);

        fs::remove_dir_all(&tmp).ok();
    }
}
