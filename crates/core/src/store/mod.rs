pub mod lineage;
pub mod paths;
pub mod watcher;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
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
    /// Every tool called in the discovered sessions, most used first.
    pub tool_counts: Vec<ToolCount>,
}

/// A tool and how many times it was called across the discovered sessions.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ToolCount {
    pub name: String,
    pub count: usize,
}

/// Cached per-file parse result, invalidated by mtime. `discover` re-runs every
/// time anything in the tree changes; without this it re-parsed every session
/// (seconds for hundreds of files) just to pick up the one that changed. Holds
/// the BASE summary (before the per-directory copy-lineage stamp) + the lineage
/// uuids, both of which depend only on the file's own contents.
#[derive(Clone)]
struct CachedFile {
    mtime: SystemTime,
    summary: SessionSummary,
    uuids: lineage::SessionUuids,
    tools: HashMap<String, usize>,
}

fn discover_cache() -> &'static Mutex<HashMap<PathBuf, CachedFile>> {
    static C: OnceLock<Mutex<HashMap<PathBuf, CachedFile>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn discover(root: &Path) -> Result<Discovery> {
    let projects = paths::projects_dir(root);
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();
    let mut tool_totals: HashMap<String, usize> = HashMap::new();

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

            // Reuse the cached parse when this file is unchanged (mtime match),
            // so a poll re-discover only re-parses the file(s) that actually grew.
            let cached = discover_cache()
                .lock()
                .unwrap()
                .get(&child_path)
                .filter(|c| c.mtime == mtime)
                .cloned();
            let (summary, su, tools) = match cached {
                Some(c) => (c.summary, c.uuids, c.tools),
                None => match parse_file(&child_path) {
                    Ok(ParseOutcome { events, .. }) => {
                        let project_label = events
                            .iter()
                            .find_map(|e| e.cwd().map(paths::label_from_cwd))
                            .unwrap_or_else(|| fallback_label.clone());
                        let summary =
                            summarize(&events, id, child_path.clone(), project_label, mtime);
                        let su = lineage::SessionUuids {
                            id,
                            head: events.iter().find_map(|e| e.uuid()),
                            uuids: events.iter().filter_map(|e| e.uuid()).collect(),
                            forked: summary.forked_from.is_some(),
                        };
                        // Counted here, from the parse discovery already does, so tool: autocomplete costs no extra reading.
                        let tools = crate::session::summary::tool_call_counts(&events);
                        discover_cache().lock().unwrap().insert(
                            child_path.clone(),
                            CachedFile { mtime, summary: summary.clone(), uuids: su.clone(), tools: tools.clone() },
                        );
                        (summary, su, tools)
                    }
                    Err(e) => {
                        warnings.push(format!("parse {}: {}", child_path.display(), e));
                        continue;
                    }
                },
            };
            for (name, n) in tools {
                *tool_totals.entry(name).or_insert(0) += n;
            }
            dir_summaries.push(summary);
            dir_uuids.push(su);
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
    sessions.sort_by_key(|s| std::cmp::Reverse(s.mtime));

    let mut tool_counts: Vec<ToolCount> =
        tool_totals.into_iter().map(|(name, count)| ToolCount { name, count }).collect();
    tool_counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

    Ok(Discovery { sessions, warnings, tool_counts })
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

/// One message's preview for views that show it without opening the session.
#[derive(Debug, Clone)]
pub struct MessagePreview {
    pub snippet: String,
    pub ts: Option<chrono::DateTime<chrono::Utc>>,
    pub tool: Option<ToolPreview>,
}

/// [`message_snippets`] plus each message's timestamp — for views that show a
/// preview with its time without opening the session (the bookmark browser).
pub fn message_previews(path: &Path, wanted: &HashSet<Uuid>) -> HashMap<Uuid, MessagePreview> {
    let mut out = HashMap::new();
    if wanted.is_empty() {
        return out;
    }
    let outcome = match parse_file(path) {
        Ok(o) => o,
        Err(_) => return out,
    };
    for (i, ev) in outcome.events.iter().enumerate() {
        let Some(id) = ev.uuid() else { continue };
        if !wanted.contains(&id) || out.contains_key(&id) {
            continue;
        }
        if let Some((text, tool)) = preview_for_turn(&outcome.events, i) {
            let line = one_line_truncated(&text, 60);
            if !line.is_empty() {
                out.insert(id, MessagePreview { snippet: line, ts: ev.timestamp(), tool });
            }
        }
    }
    out
}

/// Everything a turn-level consumer needs from the reply anchored at
/// `events[i]`: its texts, tool names, and first thinking snippet. Walks over
/// interleaved tool_result user lines and attachments, stopping at a real
/// user prompt (the shape real files actually have).
struct TurnScan {
    texts: Vec<String>,
    tools: Vec<ToolPreview>,
    thinking: Option<String>,
}

fn turn_scan(events: &[Event], i: usize) -> TurnScan {
    let mut scan = TurnScan { texts: Vec::new(), tools: Vec::new(), thinking: None };
    // A user head is its own single-event turn.
    if let Event::User(_) = &events[i] {
        if let Some(t) = preview_text(&events[i]) {
            scan.texts.push(t);
        }
        return scan;
    }
    if !matches!(events[i], Event::Assistant(_)) {
        return scan;
    }
    for ev in events[i..].iter() {
        match ev {
            Event::Assistant(a) => {
                for b in &a.message.content {
                    match b {
                        Block::Text { text } if !text.trim().is_empty() => {
                            scan.texts.push(text.clone());
                        }
                        Block::ToolUse { name, input, .. } => scan.tools.push(ToolPreview {
                            name: name.clone(),
                            summary: crate::session::display::tool_input_summary(name, input),
                        }),
                        Block::Thinking { thinking: th, .. }
                            if scan.thinking.is_none() && !th.trim().is_empty() =>
                        {
                            scan.thinking = Some(th.trim().to_string());
                        }
                        _ => {}
                    }
                }
            }
            Event::User(u) => {
                let pure_result = matches!(
                    &u.message.content,
                    MessageContent::Blocks(bs)
                        if !bs.is_empty()
                            && bs.iter().all(|b| matches!(b, UserBlock::ToolResult { .. }))
                );
                if !pure_result {
                    break; // a real prompt ends the reply
                }
            }
            _ => {} // attachments/system interleave within a reply
        }
    }
    scan
}

/// Label for a turn with no matched text: first text, else thinking, else a tool call —
/// the one named by `prefer` (a `tool:` filter) when present, else the first.
fn preview_from_scan(scan: &TurnScan, prefer: Option<&str>) -> Option<(String, Option<ToolPreview>)> {
    if let Some(t) = scan.texts.first().or(scan.thinking.as_ref()) {
        return Some((t.clone(), None));
    }
    let call = pick_tool(&scan.tools, prefer)?;
    Some((tool_line(call), Some(call.clone())))
}

fn pick_tool<'a>(calls: &'a [ToolPreview], prefer: Option<&str>) -> Option<&'a ToolPreview> {
    prefer
        .and_then(|want| calls.iter().find(|c| c.name.eq_ignore_ascii_case(want)))
        .or_else(|| calls.first())
}

/// Plain-text form of a tool preview, for consumers that only read `snippet`.
fn tool_line(t: &ToolPreview) -> String {
    format!("{}: {}", t.name, t.summary)
}

/// Preview for the REPLY anchored at `events[i]` — see [`turn_scan`].
fn preview_for_turn(events: &[Event], i: usize) -> Option<(String, Option<ToolPreview>)> {
    if let Some(t) = preview_text(&events[i]) {
        return Some((t, None));
    }
    preview_from_scan(&turn_scan(events, i), None)
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

/// Move a session file into the app trash (`<root>/.trash/claude-code-manager/`) and
/// write a `<name>.trashinfo` sidecar recording the original path and deletion
/// time so it can be restored later. Returns the trashed file's new path.
///
/// The trash dir lives under `<root>` but outside `<root>/projects/`, so trashed
/// sessions are not re-discovered by `discover`.
pub fn trash_session(root: &Path, file_path: &Path) -> Result<PathBuf> {
    let trash_dir = root.join(".trash").join("claude-code-manager");
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

/// A tool call as a result card shows it when there is no text to excerpt. The name
/// stays raw; the UI owns how tool names display.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ToolPreview {
    pub name: String,
    pub summary: String,
}

/// A single matching message from a cross-session search.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MessageHit {
    pub session_id: Uuid,
    pub uuid: Option<Uuid>,
    pub role: String,
    /// Epoch seconds, when the line carried a timestamp.
    pub ts: Option<i64>,
    pub snippet: String,
    /// Occurrences of the query across the message's conversation text
    /// (0 when the hit satisfied a query-less filter).
    pub match_count: usize,
    /// Set when the message has no text to excerpt and is previewed by a tool call instead.
    pub tool: Option<ToolPreview>,
}

/// One candidate file for [`search_messages`], with an optional uuid restriction
/// (the `is:bookmarked` path passes the session's bookmark uuids here so every
/// query shape flows through one engine).
#[derive(Debug, Clone)]
pub struct SearchFile {
    pub path: std::path::PathBuf,
    pub session_id: Uuid,
    pub only_uuids: Option<Vec<Uuid>>,
}

/// Message-level filters. `text` empty = no content requirement (some other
/// filter must narrow instead — callers enforce that).
#[derive(Debug, Clone, Default)]
pub struct MessageFilters {
    pub text: String,
    pub role: Option<String>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub tool: Option<String>,
    pub has_link: bool,
    pub limit: usize,
    /// Matches to skip before collecting — the "Load more" continuation.
    /// Stateless by design: a re-scan with the raw-line prefilter is cheap, and
    /// unlike a cursor it cannot go stale when files grow between pages.
    pub skip: usize,
}

/// The global-find engine: walk the (already prefiltered) candidate files and
/// return every conversation message passing the filters, capped at `limit`
/// (second return = truncated). Text matches are confirmed in conversation
/// TEXT blocks — tool payloads and machinery never match. Raw-line prefilters
/// keep JSON parsing to candidate lines: the query text for text searches, any
/// wanted uuid for restricted files.
pub fn search_messages(files: &[SearchFile], f: &MessageFilters) -> (Vec<MessageHit>, bool) {
    use std::io::{BufRead, BufReader};
    let query_lower = f.text.to_lowercase();
    let mut out = Vec::new();
    let mut skipped = 0usize;

    'files: for sf in files {
        // Restricted files (is:bookmarked) get a TURN-aware event walk: a
        // bookmark stores the merged turn's head uuid, often a textless
        // thinking/tool line — the cheap line walk below would silently drop
        // it. Bookmarked sessions are few and their uuid sets tiny, so the
        // full parse is affordable exactly where it's needed.
        if let Some(us) = &sf.only_uuids {
            let Ok(outcome) = parse_file(&sf.path) else { continue };
            for (i, ev) in outcome.events.iter().enumerate() {
                let Some(id) = ev.uuid() else { continue };
                if ev.hidden_from_transcript() {
                    continue; // nothing to open — the transcript never shows it
                }
                if !us.contains(&id) {
                    continue;
                }
                let role = match ev {
                    Event::User(_) => "user",
                    Event::Assistant(_) => "assistant",
                    _ => continue,
                };
                if let Some(want) = &f.role {
                    if role != want {
                        continue;
                    }
                }
                let ts = ev.timestamp().map(|d| d.timestamp());
                if let Some(after) = f.after {
                    if ts.is_none_or(|x| x < after) {
                        continue;
                    }
                }
                if let Some(before) = f.before {
                    if ts.is_none_or(|x| x >= before) {
                        continue;
                    }
                }
                let scan = turn_scan(&outcome.events, i);
                if let Some(tool) = &f.tool {
                    if !scan.tools.iter().any(|t| t.name.eq_ignore_ascii_case(tool)) {
                        continue;
                    }
                }
                if f.has_link
                    && !scan.texts.iter().any(|t| t.contains("http://") || t.contains("https://"))
                {
                    continue;
                }
                let matched = if query_lower.is_empty() {
                    None
                } else if scan.texts.iter().any(|t| t.to_lowercase().contains(&query_lower)) {
                    // Join every text block: the excerpts and the ×N count
                    // cover the whole turn, not just the first matching block.
                    Some(scan.texts.join("\n"))
                } else {
                    continue; // text query unsatisfied in this turn
                };
                let ((snippet, match_count), tool) = match matched {
                    Some(t) => (snippet_excerpts(&t, &query_lower), None),
                    None => {
                        let (s, tool) = preview_from_scan(&scan, f.tool.as_deref())
                            .unwrap_or_else(|| ("(no text)".to_string(), None));
                        ((s, 0), tool)
                    }
                };
                if skipped < f.skip {
                    skipped += 1;
                    continue;
                }
                out.push(MessageHit {
                    session_id: sf.session_id,
                    uuid: Some(id),
                    role: role.to_string(),
                    ts,
                    snippet,
                    match_count,
                    tool,
                });
                if out.len() >= f.limit {
                    break 'files;
                }
            }
            continue;
        }
        let Ok(file) = fs::File::open(&sf.path) else { continue };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else { continue };
            if !query_lower.is_empty() && !line.to_lowercase().contains(&query_lower) {
                continue;
            }
            let Some(hit) = message_hit(&line, sf.session_id, &query_lower, f) else { continue };
            if skipped < f.skip {
                skipped += 1;
                continue;
            }
            out.push(hit);
            if out.len() >= f.limit {
                break 'files;
            }
        }
    }
    let truncated = out.len() >= f.limit;
    (out, truncated)
}

/// Apply the per-message filters to one jsonl line.
fn message_hit(
    line: &str,
    session_id: Uuid,
    query_lower: &str,
    f: &MessageFilters,
) -> Option<MessageHit> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let t = v.get("type")?.as_str()?;
    if t != "user" && t != "assistant" {
        return None;
    }
    // Injected machinery reads as a user message but is never rendered, so a
    // hit on one cannot be opened: clicking it did nothing at all.
    if crate::session::event::raw_line_hidden_from_transcript(&v) {
        return None;
    }
    if let Some(role) = &f.role {
        if t != role {
            return None;
        }
    }
    let ts = v
        .get("timestamp")
        .and_then(|x| x.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp());
    if let Some(after) = f.after {
        if ts.is_none_or(|x| x < after) {
            return None;
        }
    }
    if let Some(before) = f.before {
        if ts.is_none_or(|x| x >= before) {
            return None;
        }
    }
    let content = v.pointer("/message/content")?;
    let mut texts: Vec<&str> = Vec::new();
    let mut calls: Vec<ToolPreview> = Vec::new();
    match content {
        serde_json::Value::String(s) => texts.push(s),
        serde_json::Value::Array(blocks) => {
            for b in blocks {
                match b.get("type").and_then(|x| x.as_str()) {
                    Some("text") => {
                        if let Some(tx) = b.get("text").and_then(|x| x.as_str()) {
                            texts.push(tx);
                        }
                    }
                    Some("tool_use") => {
                        if let Some(n) = b.get("name").and_then(|x| x.as_str()) {
                            let input = b.get("input").cloned().unwrap_or_default();
                            calls.push(ToolPreview {
                                name: n.to_string(),
                                summary: crate::session::display::tool_input_summary(n, &input),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => return None,
    }
    if let Some(tool) = &f.tool {
        if !calls.iter().any(|c| c.name.eq_ignore_ascii_case(tool)) {
            return None;
        }
    }
    if f.has_link && !texts.iter().any(|tx| tx.contains("http://") || tx.contains("https://")) {
        return None;
    }
    // Join every text block: the excerpts and the ×N count cover the whole
    // message, not just the first matching block.
    let matched = if query_lower.is_empty() {
        texts.first().map(|tx| (*tx).to_string())
    } else if texts.iter().any(|tx| tx.to_lowercase().contains(query_lower)) {
        Some(texts.join("\n"))
    } else {
        None
    };
    let ((snippet, match_count), tool) = match matched {
        Some(tx) => (snippet_excerpts(&tx, query_lower), None),
        None if query_lower.is_empty() => {
            // `?`: nothing displayable and nothing required it.
            let call = pick_tool(&calls, f.tool.as_deref())?.clone();
            ((tool_line(&call), 0), Some(call))
        }
        None => return None, // text query not satisfied in conversation text
    };
    let uuid = v.get("uuid").and_then(|u| u.as_str()).and_then(|s| Uuid::parse_str(s).ok());
    Some(MessageHit { session_id, uuid, role: t.to_string(), ts, snippet, match_count, tool })
}

/// Discord-style multi-excerpt snippet: a window (~40 chars before / ~70
/// after) around each of the first [`MAX_WINDOWS`] occurrences, overlapping
/// windows merged, non-contiguous ones joined by " … ", whitespace collapsed.
/// Returns the snippet plus the TOTAL occurrence count (the card's ×N badge —
/// it may exceed the excerpts shown). An empty/absent query yields the text's
/// head and count 0. Char-based so multi-byte text can't split a codepoint.
fn snippet_excerpts(text: &str, query_lower: &str) -> (String, usize) {
    const BEFORE: usize = 40;
    const AFTER: usize = 70;
    const MAX_WINDOWS: usize = 3;

    let chars: Vec<char> = text.chars().collect();
    if query_lower.is_empty() {
        let end = 180.min(chars.len());
        let mut out = chars[..end].iter().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
        if end < chars.len() {
            out.push('…');
        }
        return (out, 0);
    }

    // Every occurrence, as char offsets (byte offsets would split multi-byte).
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let needle: Vec<char> = query_lower.chars().collect();
    let mut positions = Vec::new();
    if !needle.is_empty() && lower.len() >= needle.len() {
        let mut i = 0;
        while i + needle.len() <= lower.len() {
            if lower[i..i + needle.len()] == needle[..] {
                positions.push(i);
                i += needle.len(); // non-overlapping, like the find bar
            } else {
                i += 1;
            }
        }
    }
    if positions.is_empty() {
        // Defensive: caller checked containment, but lowercasing can shift
        // char counts in exotic scripts — fall back to the text's head.
        let (head, _) = snippet_excerpts(text, "");
        return (head, 0);
    }

    // Windows around the first MAX_WINDOWS occurrences, merged when they touch.
    let mut windows: Vec<(usize, usize)> = Vec::new();
    for &p in positions.iter().take(MAX_WINDOWS) {
        let start = p.saturating_sub(BEFORE);
        let end = (p + needle.len() + AFTER).min(chars.len());
        match windows.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => windows.push((start, end)),
        }
    }

    let mut parts = Vec::new();
    for &(start, end) in &windows {
        parts.push(chars[start..end].iter().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let mut out = String::new();
    if windows[0].0 > 0 {
        out.push('…');
    }
    out.push_str(&parts.join(" … "));
    if windows.last().unwrap().1 < chars.len() {
        out.push('…');
    }
    (out, positions.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_excerpts_single_match_windows_and_counts() {
        let text = format!("{} needle {}", "a".repeat(100), "b".repeat(200));
        let (snip, n) = snippet_excerpts(&text, "needle");
        assert_eq!(n, 1);
        assert!(snip.starts_with('…') && snip.ends_with('…'));
        assert!(snip.contains("needle"));
        // window is ~40 before + ~70 after, so nowhere near the full 300 chars
        assert!(snip.chars().count() < 160, "len={}", snip.chars().count());
    }

    #[test]
    fn snippet_excerpts_spread_matches_get_separate_windows() {
        let text = format!(
            "first needle here {} second needle there {} third needle end {} fourth needle tail",
            "x".repeat(300),
            "y".repeat(300),
            "z".repeat(300),
        );
        let (snip, n) = snippet_excerpts(&text, "needle");
        assert_eq!(n, 4, "counts ALL occurrences");
        // …but only the first 3 windows are excerpted
        assert_eq!(snip.matches("needle").count(), 3);
        assert_eq!(snip.matches(" … ").count(), 2, "non-contiguous windows joined by ellipsis: {snip}");
        assert!(snip.contains("first needle") && snip.contains("second needle") && snip.contains("third needle"));
        assert!(!snip.contains("fourth"));
    }

    #[test]
    fn snippet_excerpts_close_matches_merge_into_one_window() {
        let (snip, n) = snippet_excerpts("the needle and the needle again", "needle");
        assert_eq!(n, 2);
        assert!(!snip.contains(" … "), "adjacent windows merge: {snip}");
        assert_eq!(snip, "the needle and the needle again");
    }

    #[test]
    fn snippet_excerpts_case_insensitive_and_multibyte_safe() {
        let (snip, n) = snippet_excerpts("Needle — ünïcödé NEEDLE", "needle");
        assert_eq!(n, 2);
        assert!(snip.contains("Needle") && snip.contains("NEEDLE"));
    }

    #[test]
    fn snippet_excerpts_empty_query_heads_text() {
        let (snip, n) = snippet_excerpts(&"w".repeat(400), "");
        assert_eq!(n, 0);
        assert!(snip.ends_with('…') && snip.chars().count() <= 181);
    }

    #[test]
    fn search_never_returns_a_message_the_transcript_hides() {
        // Claude Code writes injected context — skill bodies, command echoes,
        // caveats — as user lines flagged isMeta, and the renderer drops them.
        // They used to come back as results that scrolled nowhere when clicked.
        let root = std::env::temp_dir().join(format!("cm-meta-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let sid = Uuid::parse_str("00000000-0000-0000-0000-00000000cccc").unwrap();
        let path = root.join(format!("{sid}.jsonl"));
        let meta_uuid = "00000000-0000-0000-0000-0000000000e1";
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                format_args!(
                    r#"{{"type":"user","uuid":"{meta_uuid}","isMeta":true,"timestamp":"2026-08-01T10:00:00Z","message":{{"role":"user","content":"needle inside a skill body"}}}}"#
                ),
                r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000e2","timestamp":"2026-08-01T10:01:00Z","message":{"role":"user","content":"needle you actually typed"}}"#,
            ),
        )
        .unwrap();
        let files = [SearchFile { path: path.clone(), session_id: sid, only_uuids: None }];

        let (hits, _) = search_messages(
            &files,
            &MessageFilters { text: "needle".into(), limit: 10, ..Default::default() },
        );
        assert_eq!(hits.len(), 1, "the isMeta line must not be offered");
        assert!(hits[0].snippet.contains("actually typed"));

        // Same rule on the uuid-restricted path (is:bookmarked), which parses
        // events instead of walking lines.
        let files = [SearchFile {
            path,
            session_id: sid,
            only_uuids: Some(vec![Uuid::parse_str(meta_uuid).unwrap()]),
        }];
        let (hits, _) = search_messages(&files, &MessageFilters { limit: 10, ..Default::default() });
        assert!(hits.is_empty(), "a bookmarked isMeta line is just as unopenable");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_never_returns_slash_command_machinery() {
        // A slash command's own stdout is a user line WITHOUT isMeta, so the
        // isMeta rule above never caught it — yet the renderer drops it, and on
        // a real corpus it was 104 of 355 unreachable user results.
        let root = std::env::temp_dir().join(format!("cm-localcmd-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let sid = Uuid::parse_str("00000000-0000-0000-0000-00000000dddd").unwrap();
        let path = root.join(format!("{sid}.jsonl"));
        let stdout_uuid = "00000000-0000-0000-0000-0000000000f1";
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                format_args!(
                    r#"{{"type":"user","uuid":"{stdout_uuid}","timestamp":"2026-08-01T10:00:00Z","message":{{"role":"user","content":"<local-command-stdout>Set model to needle</local-command-stdout>"}}}}"#
                ),
                r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000f2","timestamp":"2026-08-01T10:01:00Z","message":{"role":"user","content":"needle you actually typed"}}"#,
            ),
        )
        .unwrap();
        let files = [SearchFile { path: path.clone(), session_id: sid, only_uuids: None }];

        let (hits, _) = search_messages(
            &files,
            &MessageFilters { text: "needle".into(), limit: 10, ..Default::default() },
        );
        assert_eq!(hits.len(), 1, "the <local-command-stdout> line must not be offered");
        assert!(hits[0].snippet.contains("actually typed"));

        // And on the uuid-restricted path, which walks events instead of lines.
        let files = [SearchFile {
            path,
            session_id: sid,
            only_uuids: Some(vec![Uuid::parse_str(stdout_uuid).unwrap()]),
        }];
        let (hits, _) = search_messages(&files, &MessageFilters { limit: 10, ..Default::default() });
        assert!(hits.is_empty(), "nor on the bookmarked path");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_messages_filters_and_caps() {
        let root = std::env::temp_dir().join(format!("cm-gf-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let sid = Uuid::parse_str("00000000-0000-0000-0000-00000000bbbb").unwrap();
        let path = root.join(format!("{sid}.jsonl"));
        fs::write(
            &path,
            concat!(
                r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000d1","timestamp":"2026-08-01T10:00:00Z","message":{"role":"user","content":"the needle early"}}"#,
                "
",
                r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000d2","timestamp":"2026-08-03T10:00:00Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"ran the needle command"}]}}"#,
                "
",
                // needle only in a tool_result payload: must never match a text query
                r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000d3","timestamp":"2026-08-04T10:00:00Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"needle in payload"}]}}"#,
                "
",
            ),
        )
        .unwrap();
        let files = [SearchFile { path: path.clone(), session_id: sid, only_uuids: None }];

        // text + role + date window
        let (hits, trunc) = search_messages(
            &files,
            &MessageFilters {
                text: "needle".into(),
                role: Some("assistant".into()),
                after: Some(1_785_000_000), // < 2026-08-03
                ..MessageFilters { limit: 10, ..Default::default() }
            },
        );
        assert!(!trunc);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].role, "assistant");
        assert!(hits[0].snippet.contains("needle"));

        // tool filter without text; cap at 1 reports truncated
        let (hits, trunc) = search_messages(
            &files,
            &MessageFilters { tool: Some("bash".into()), limit: 1, ..Default::default() },
        );
        assert!(trunc);
        assert_eq!(hits.len(), 1);

        // skip: the Load-more continuation pages through matches statelessly
        let (page2, _) = search_messages(
            &files,
            &MessageFilters { text: "needle".into(), limit: 1, skip: 1, ..Default::default() },
        );
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].role, "assistant", "skip=1 lands on the second match");

        // uuid restriction (the is:bookmarked path)
        let only = vec![Uuid::parse_str("00000000-0000-0000-0000-0000000000d1").unwrap()];
        let files_b = [SearchFile { path, session_id: sid, only_uuids: Some(only) }];
        let (hits, _) = search_messages(&files_b, &MessageFilters { limit: 10, ..Default::default() });
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("early"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bookmarked_search_resolves_a_textless_turn_head() {
        // is:bookmarked flows through only_uuids; a bookmark on a turn whose
        // head line is a textless thinking/tool line must still appear in
        // results (the line walk silently dropped it — that was the gap that
        // made "query bookmarks from the find tab" a false claim).
        let root = std::env::temp_dir().join(format!("cm-bmsearch-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let sid = Uuid::parse_str("00000000-0000-0000-0000-00000000cccc").unwrap();
        let path = root.join(format!("{sid}.jsonl"));
        let body = [
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000f1","timestamp":"2026-08-15T10:00:00Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":""}]}}"#,
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000f2","timestamp":"2026-08-15T10:00:01Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000f3","timestamp":"2026-08-15T10:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000f4","timestamp":"2026-08-15T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"bookmark landed here"}]}}"#,
        ]
        .join("\n");
        fs::write(&path, body + "\n").unwrap();
        let head = Uuid::parse_str("00000000-0000-0000-0000-0000000000f1").unwrap();
        let files =
            [SearchFile { path, session_id: sid, only_uuids: Some(vec![head]) }];

        // No text query: the bookmark lists with the turn's text as snippet.
        let (hits, _) =
            search_messages(&files, &MessageFilters { limit: 10, ..Default::default() });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "bookmark landed here");

        // Text query matches the TURN's text, not just the head line.
        let (hits, _) = search_messages(
            &files,
            &MessageFilters { text: "landed".into(), limit: 10, ..Default::default() },
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("landed"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn preview_resolves_a_textless_turn_head() {
        // A merged assistant turn's head is often a thinking/tool_use line; the
        // bookmark stores THAT uuid. The preview must come from the run's later
        // text line — "(message not found)" for a real message is the bug.
        let root = std::env::temp_dir().join(format!("cm-prevhead-{}", std::process::id()));
        let proj = root.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();
        let path = proj.join("h.jsonl");
        let head = "00000000-0000-0000-0000-0000000000e1";
        // The real shape that broke: empty redacted thinking head, then
        // tool_use / tool_result / attachment interleaved, text arriving late.
        let body = [
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000e1","timestamp":"2026-08-15T10:00:00Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":""}]}}"#,
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000e2","timestamp":"2026-08-15T10:00:01Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000e3","timestamp":"2026-08-15T10:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"attachment","uuid":"00000000-0000-0000-0000-0000000000e4","timestamp":"2026-08-15T10:00:02Z"}"#,
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000e5","timestamp":"2026-08-15T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"the actual reply text"}]}}"#,
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000e6","timestamp":"2026-08-15T10:01:00Z","message":{"role":"user","content":"next question"}}"#,
        ]
        .join("\n");
        fs::write(&path, body + "\n").unwrap();
        let wanted: std::collections::HashSet<Uuid> =
            [Uuid::parse_str(head).unwrap()].into_iter().collect();
        let got = message_previews(&path, &wanted);
        let p = got.get(&Uuid::parse_str(head).unwrap()).expect("resolves");
        assert_eq!(p.snippet, "the actual reply text");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_counts_tool_calls_across_sessions_but_not_subagents() {
        let root = std::env::temp_dir().join(format!("cm-toolcounts-{}", std::process::id()));
        let proj = root.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();
        let session = |uuid_tail: &str, tools: &[&str]| {
            let calls: Vec<String> = tools
                .iter()
                .enumerate()
                .map(|(i, t)| format!(r#"{{"type":"tool_use","id":"t{i}","name":"{t}","input":{{}}}}"#))
                .collect();
            [
                format!(r#"{{"type":"user","uuid":"00000000-0000-0000-0000-0000000001{uuid_tail}","timestamp":"2026-09-10T05:00:00Z","cwd":"/tmp/proj","message":{{"role":"user","content":"go"}}}}"#),
                format!(r#"{{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000002{uuid_tail}","parentUuid":"00000000-0000-0000-0000-0000000001{uuid_tail}","timestamp":"2026-09-10T05:00:01Z","message":{{"role":"assistant","content":[{}]}}}}"#, calls.join(",")),
            ]
            .join("\n")
                + "\n"
        };
        fs::write(proj.join("00000000-0000-0000-0000-00000000aa01.jsonl"), session("01", &["Bash", "Bash", "Read"])).unwrap();
        fs::write(proj.join("00000000-0000-0000-0000-00000000aa02.jsonl"), session("02", &["Bash", "mcp__x__y", "mcp__x__y", "mcp__x__y"])).unwrap();
        // A subagent transcript is not a session global find searches, so its calls don't count.
        fs::write(proj.join("agent-abc123.jsonl"), session("03", &["Bash", "Bash", "Bash", "Bash", "Bash"])).unwrap();

        let expected = vec![
            ToolCount { name: "Bash".into(), count: 3 },
            ToolCount { name: "mcp__x__y".into(), count: 3 },
            ToolCount { name: "Read".into(), count: 1 },
        ];
        assert_eq!(discover(&root).unwrap().tool_counts, expected, "most used first, ties by name");
        assert_eq!(discover(&root).unwrap().tool_counts, expected, "the same when served from the parse cache");
        fs::remove_dir_all(&root).ok();
    }

    /// A session whose only assistant line is two tool calls and no text.
    fn tool_only_session(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, Uuid, Uuid) {
        let root = std::env::temp_dir().join(format!("cm-tool-{tag}-{}", std::process::id()));
        let proj = root.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();
        let sid = Uuid::parse_str("00000000-0000-0000-0000-00000000eeee").unwrap();
        let call = "00000000-0000-0000-0000-0000000000c1";
        let path = proj.join(format!("{sid}.jsonl"));
        let body = [
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c0","timestamp":"2026-09-10T05:16:00Z","message":{"role":"user","content":"check the repo"}}"#,
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000c1","timestamp":"2026-09-10T05:18:00Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"src/a.ts"}},{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"git status","description":"Show status"}}]}}"#,
        ]
        .join("\n");
        fs::write(&path, body + "\n").unwrap();
        (root, path, sid, Uuid::parse_str(call).unwrap())
    }

    #[test]
    fn a_tool_filtered_hit_previews_the_call_it_matched() {
        let (root, path, sid, _) = tool_only_session("filter");
        let files = [SearchFile { path, session_id: sid, only_uuids: None }];
        let (hits, _) = search_messages(
            &files,
            &MessageFilters { tool: Some("bash".into()), limit: 10, ..Default::default() },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].tool,
            Some(ToolPreview { name: "Bash".into(), summary: "git status".into() }),
            "the Bash call the filter asked for, not the Read before it"
        );
        assert_eq!(hits[0].snippet, "Bash: git status");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_bookmarked_tool_turn_previews_its_first_call() {
        let (root, path, sid, call) = tool_only_session("bm");
        let files = [SearchFile { path: path.clone(), session_id: sid, only_uuids: Some(vec![call]) }];
        let (hits, _) = search_messages(&files, &MessageFilters { limit: 10, ..Default::default() });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tool, Some(ToolPreview { name: "Read".into(), summary: "src/a.ts".into() }));

        // The bookmark browser's preview carries the same.
        let wanted: std::collections::HashSet<Uuid> = [call].into_iter().collect();
        let p = message_previews(&path, &wanted).remove(&call).expect("resolves");
        assert_eq!(p.tool, Some(ToolPreview { name: "Read".into(), summary: "src/a.ts".into() }));
        assert_eq!(p.snippet, "Read: src/a.ts");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_text_hit_carries_no_tool_preview() {
        let (root, path, sid, _) = tool_only_session("text");
        let files = [SearchFile { path, session_id: sid, only_uuids: None }];
        let (hits, _) = search_messages(
            &files,
            &MessageFilters { text: "repo".into(), limit: 10, ..Default::default() },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tool, None);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn message_previews_carry_the_timestamp() {
        use std::collections::HashSet;
        let root = std::env::temp_dir().join(format!("cm-prev-{}", std::process::id()));
        let proj = root.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();
        let path = proj.join("p.jsonl");
        let u1 = "00000000-0000-0000-0000-0000000000b1";
        fs::write(
            &path,
            format!(
                concat!(
                    r#"{{"type":"user","uuid":"{u}","timestamp":"2026-08-05T10:00:00Z","message":{{"role":"user","content":"bookmark me"}}}}"#,
                    "\n",
                ),
                u = u1,
            ),
        )
        .unwrap();

        let wanted: HashSet<Uuid> = [Uuid::parse_str(u1).unwrap()].into_iter().collect();
        let got = message_previews(&path, &wanted);
        let p = got.get(&Uuid::parse_str(u1).unwrap()).unwrap();
        assert_eq!(p.snippet, "bookmark me");
        assert_eq!(p.ts.unwrap().timestamp(), 1_785_924_000);
        std::fs::remove_dir_all(&root).ok();
    }

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
            {
                let long = "x ".repeat(80);
                format!(
                    concat!(
                        r#"{{"type":"user","uuid":"{u1}","message":{{"role":"user","content":"hello fork point here"}}}}"#,
                        "\n",
                        r#"{{"type":"user","uuid":"{u2}","message":{{"role":"user","content":"{long}"}}}}"#,
                        "\n",
                    ),
                    u1 = u1,
                    u2 = u2,
                    long = long,
                )
            },
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

        // Original gone, trashed file present under .trash/claude-code-manager.
        assert!(!src.exists(), "source should be moved");
        assert!(dest.exists(), "trashed file should exist");
        assert!(dest.starts_with(root.join(".trash").join("claude-code-manager")));

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
