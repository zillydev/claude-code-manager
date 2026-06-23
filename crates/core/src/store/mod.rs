pub mod lineage;
pub mod paths;
pub mod watcher;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::session::event::{Block, Event, MessageContent, UserBlock};
use crate::session::parse::{last_activity, parse_file, ParseOutcome};
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
/// Cheap activity probe for the sidebar running/unread poll. Returns `(session
/// id, last real-event time, awaiting_input)` for every session whose file was
/// modified within `recent_secs` — the only ones that could be "running".
/// Tail-reads each such file; everything older is skipped after a single stat.
pub fn scan_activity(root: &Path, recent_secs: u64) -> Vec<(Uuid, SystemTime, bool)> {
    let projects = paths::projects_dir(root);
    let now = SystemTime::now();
    let mut out = Vec::new();
    let read_dir = match fs::read_dir(&projects) {
        Ok(rd) => rd,
        Err(_) => return out,
    };
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
            let mtime = match child.metadata().and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            // Only recently-touched files can be running; skip the rest (stat only).
            if now.duration_since(mtime).map(|d| d.as_secs()).unwrap_or(u64::MAX) > recent_secs {
                continue;
            }
            if let Some((ts, awaiting)) = last_activity(&child_path) {
                out.push((id, SystemTime::from(ts), awaiting));
            }
        }
    }
    out
}

/// Order-independent, change-sensitive hash of a set of `(path, mtime_secs)`.
/// Used to detect when the session set or any file changed (cheap re-discovery
/// trigger for the shared poller).
pub fn signature_of(mut entries: Vec<(String, u64)>) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    entries.sort();
    let mut h = DefaultHasher::new();
    entries.hash(&mut h);
    h.finish()
}

/// Stat-walk `projects/*/*.jsonl` and hash `(path, mtime)` — a cheap probe to
/// detect any add/remove/modify across all sessions without parsing.
pub fn dir_signature(root: &Path) -> u64 {
    let projects = paths::projects_dir(root);
    let mut entries: Vec<(String, u64)> = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&projects) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Ok(inner) = fs::read_dir(&path) {
                for child in inner.flatten() {
                    let cp = child.path();
                    if cp.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let mtime = child
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    entries.push((cp.to_string_lossy().into_owned(), mtime));
                }
            }
        }
    }
    signature_of(entries)
}

/// One-line, ~`max`-char previews for specific message uuids in a single session
/// file. Parses the file once; returns only requested uuids that resolve to a
/// non-empty text turn (a user text message, or an assistant's first text block).
/// Used to label fork/copy branch points in the lineage tree.
pub fn message_snippets(path: &Path, wanted: &HashSet<Uuid>) -> HashMap<Uuid, String> {
    let mut out = HashMap::new();
    if wanted.is_empty() {
        return out;
    }
    let outcome = match parse_file(path) {
        Ok(o) => o,
        Err(_) => return out,
    };
    for ev in &outcome.events {
        let id = match ev.uuid() {
            Some(id) => id,
            None => continue,
        };
        if !wanted.contains(&id) || out.contains_key(&id) {
            continue;
        }
        if let Some(text) = preview_text(ev) {
            let line = one_line_truncated(&text, 60);
            if !line.is_empty() {
                out.insert(id, line);
            }
        }
    }
    out
}

fn preview_text(ev: &Event) -> Option<String> {
    match ev {
        Event::User(u) => match &u.message.content {
            MessageContent::Text(t) => Some(t.clone()),
            MessageContent::Blocks(bs) => bs.iter().find_map(|b| match b {
                UserBlock::Text { text } => Some(text.clone()),
                _ => None,
            }),
        },
        Event::Assistant(a) => a.message.content.iter().find_map(|b| match b {
            Block::Text { text } => Some(text.clone()),
            _ => None,
        }),
        _ => None,
    }
}

fn one_line_truncated(s: &str, max: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let t: String = collapsed.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

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
    fn message_snippets_resolves_text_and_truncates() {
        use std::collections::HashSet;
        let root = std::env::temp_dir().join(format!("cm-snip-{}", std::process::id()));
        let proj = root.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();
        let path = proj.join("s.jsonl");
        let u1 = "00000000-0000-0000-0000-0000000000a1";
        let u2 = "00000000-0000-0000-0000-0000000000a2";
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                format!(r#"{{"type":"user","uuid":"{u1}","message":{{"role":"user","content":"hello fork point here"}}}}"#),
                format!(r#"{{"type":"user","uuid":"{u2}","message":{{"role":"user","content":"{}"}}}}"#, "x ".repeat(80)),
            ),
        )
        .unwrap();

        let wanted: HashSet<Uuid> =
            [u1, u2].iter().map(|s| Uuid::parse_str(s).unwrap()).collect();
        let got = message_snippets(&path, &wanted);

        assert_eq!(
            got.get(&Uuid::parse_str(u1).unwrap()).map(String::as_str),
            Some("hello fork point here")
        );
        let long = got.get(&Uuid::parse_str(u2).unwrap()).unwrap();
        assert!(long.ends_with('…') && long.chars().count() == 60, "got: {long:?}");
        assert!(!got.contains_key(&Uuid::parse_str("00000000-0000-0000-0000-0000000000ff").unwrap()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn signature_of_is_order_independent_and_change_sensitive() {
        let a = signature_of(vec![("/p/x.jsonl".into(), 10), ("/p/y.jsonl".into(), 20)]);
        let b = signature_of(vec![("/p/y.jsonl".into(), 20), ("/p/x.jsonl".into(), 10)]);
        assert_eq!(a, b, "same entries, any order → same signature");

        let c = signature_of(vec![("/p/x.jsonl".into(), 11), ("/p/y.jsonl".into(), 20)]);
        assert_ne!(a, c, "a changed mtime → different signature");

        let d = signature_of(vec![("/p/x.jsonl".into(), 10)]);
        assert_ne!(a, d, "a removed entry → different signature");
    }

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
