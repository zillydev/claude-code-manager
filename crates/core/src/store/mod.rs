pub mod paths;
pub mod watcher;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
                    sessions.push(summary);
                }
                Err(e) => {
                    warnings.push(format!("parse {}: {}", child_path.display(), e));
                }
            }
        }
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
