use std::sync::mpsc::channel;
use std::sync::Mutex;

use claude_manager_core::session::edit::{commit, CommitOpts, CommitResult};
use claude_manager_core::session::parse::parse_tail;
use claude_manager_core::session::raw::{active_path, read_raw, RawLine};
use claude_manager_core::session::render::{render_session, RenderedSession, RenderedTurn};
use claude_manager_core::session::summary::SessionSummary;
use claude_manager_core::session::tree;
use claude_manager_core::store;
use claude_manager_core::store::paths;
use claude_manager_core::store::watcher;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Mutable state shared between tauri commands — tracks the open session for live tail.
pub struct WatcherState {
    pub watcher: Option<watcher::FileWatcher>,
}

impl Default for WatcherState {
    fn default() -> Self {
        Self { watcher: None }
    }
}

#[tauri::command]
pub fn list_sessions() -> Result<Vec<SessionSummary>, String> {
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let disc = store::discover(&root).map_err(|e| e.to_string())?;
    Ok(disc.sessions)
}

#[tauri::command]
pub fn search_transcripts(query: String) -> Result<Vec<String>, String> {
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let ids = store::search_sessions(&root, &query).map_err(|e| e.to_string())?;
    Ok(ids.into_iter().map(|u| u.to_string()).collect())
}

#[tauri::command]
pub fn open_session(
    file_path: String,
    project_label: String,
    app: AppHandle,
) -> Result<RenderedSession, String> {
    let session =
        store::open_session(file_path.clone().into(), project_label).map_err(|e| e.to_string())?;
    let rendered = render_session(&session);

    // Start file watcher for live tail.
    let state = app.state::<Mutex<WatcherState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;

    // Drop any previous watcher.
    guard.watcher = None;

    let (tx, rx) = channel::<()>();
    let bytes_read = session.bytes_read;
    let fp = file_path.clone();

    match watcher::watch_file(std::path::Path::new(&file_path), tx) {
        Ok(w) => {
            guard.watcher = Some(w);

            // Spawn a thread that reads from rx, debounces, tails the file,
            // renders new events, and emits them to the frontend.
            let app_clone = app.clone();
            std::thread::spawn(move || {
                tail_loop(rx, &fp, bytes_read, app_clone);
            });
        }
        Err(_) => {
            // Watcher failure is non-fatal; live tail just won't work.
        }
    }

    Ok(rendered)
}

#[tauri::command]
pub fn stop_watching(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Mutex<WatcherState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.watcher = None;
    Ok(())
}

fn tail_loop(
    rx: std::sync::mpsc::Receiver<()>,
    file_path: &str,
    initial_offset: u64,
    app: AppHandle,
) {
    let debounce = std::time::Duration::from_millis(80);
    let mut offset = initial_offset;
    let path = std::path::PathBuf::from(file_path);

    loop {
        // Block until a file-change notification arrives. If the channel
        // disconnects (watcher dropped), we exit the thread.
        if rx.recv().is_err() {
            break;
        }
        // Debounce: drain any queued notifications.
        std::thread::sleep(debounce);
        while rx.try_recv().is_ok() {}

        // Parse new content.
        let Ok(outcome) = parse_tail(&path, offset) else {
            continue;
        };
        if outcome.events.is_empty() {
            offset = outcome.bytes_read;
            continue;
        }

        // Render the new events into turns. We build a mini Session from
        // just the new events to get rendered turns — this avoids re-parsing
        // the entire file. The tree resolution is trivial for append-only
        // new events (they're all on the current path).
        let leaf = tree::resolve_leaf(&outcome.events);
        let mini = claude_manager_core::session::Session {
            project_label: String::new(),
            id: uuid::Uuid::nil(),
            file_path: path.clone(),
            events: outcome.events,
            leaf,
            mtime: std::time::SystemTime::now(),
            bytes_read: outcome.bytes_read,
        };
        let rendered = render_session(&mini);
        let new_turns: Vec<RenderedTurn> = rendered.turns;

        if !new_turns.is_empty() {
            let _ = app.emit("session-updated", &new_turns);
        }
        offset = outcome.bytes_read;
    }
}

/// One editable turn surfaced to the UI. `index` is the position on the active
/// path among message turns (used as the fork point and the edit target).
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
        .filter(|l| l.kind == "user" || l.kind == "assistant")
        .enumerate()
        .map(|(index, l)| EditableTurn {
            index,
            uuid: l.uuid.map(|u| u.to_string()).unwrap_or_default(),
            role: l.kind.clone(),
            text: line_text(l),
        })
        .collect();
    Ok(EditableSession { source_path: file_path, cwd, git_branch, turns })
}

/// Commit an edit: optionally set the text of the message-turn at `edit_index`,
/// then write the full active path as a new flattened session (copy-on-edit).
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

    // Snapshot the active path's uuids (order) BEFORE any edit.
    let active_uuids: Vec<uuid::Uuid> = active_path(&lines).iter().filter_map(|l| l.uuid).collect();

    // Optionally edit the text of the message-turn at edit_index.
    if let (Some(idx), Some(text)) = (edit_index, new_text.as_ref()) {
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

    // Rebuild active-path refs (post-edit) by uuid, preserving original order.
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

/// Fork: write the prefix up to and including the message-turn at `fork_index`
/// as a new session (copy-on-edit).
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
