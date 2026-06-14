use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::sync::Mutex;

use claude_manager_core::session::edit::{commit, CommitOpts, CommitResult};
use claude_manager_core::session::parse::parse_tail;
use claude_manager_core::session::raw::{active_path, first_cwd, read_raw, RawLine};
use claude_manager_core::session::render::{render_session, RenderedSession, RenderedTurn};
use claude_manager_core::session::summary::SessionSummary;
use claude_manager_core::session::tree;
use claude_manager_core::store;
use claude_manager_core::store::paths;
use claude_manager_core::store::watcher;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Mutable state shared between tauri commands — one live-tail watcher per
/// window, keyed by the window's label, so multiple windows tail different
/// sessions without clobbering each other.
pub struct WatcherState {
    pub watchers: HashMap<String, watcher::FileWatcher>,
}

impl Default for WatcherState {
    fn default() -> Self {
        Self { watchers: HashMap::new() }
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
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<RenderedSession, String> {
    let session =
        store::open_session(file_path.clone().into(), project_label).map_err(|e| e.to_string())?;
    let rendered = render_session(&session);

    let label = window.label().to_string();

    // Start file watcher for live tail, scoped to THIS window.
    let state = app.state::<Mutex<WatcherState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;

    // Drop any previous watcher for this window only.
    guard.watchers.remove(&label);

    let (tx, rx) = channel::<()>();
    let bytes_read = session.bytes_read;
    let fp = file_path.clone();

    match watcher::watch_file(std::path::Path::new(&file_path), tx) {
        Ok(w) => {
            guard.watchers.insert(label.clone(), w);

            // Spawn a thread that reads from rx, debounces, tails the file,
            // renders new events, and emits them to the originating window.
            let app_clone = app.clone();
            std::thread::spawn(move || {
                tail_loop(rx, &fp, bytes_read, app_clone, label);
            });
        }
        Err(_) => {
            // Watcher failure is non-fatal; live tail just won't work.
        }
    }

    Ok(rendered)
}

#[tauri::command]
pub fn stop_watching(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    let state = app.state::<Mutex<WatcherState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.watchers.remove(window.label());
    Ok(())
}

fn tail_loop(
    rx: std::sync::mpsc::Receiver<()>,
    file_path: &str,
    initial_offset: u64,
    app: AppHandle,
    label: String,
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
            let _ = app.emit_to(label.as_str(), "session-updated", &new_turns);
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

/// Commit an edit: optionally set the text of the message-turn identified by
/// `edit_uuid`, then write the full active path as a new flattened session
/// (copy-on-edit). Resolving by uuid (not a positional index) is required: the
/// live-tail renders streamed turns with batch-local indices, so an index can
/// point at the wrong message; the uuid is stable.
#[tauri::command]
pub fn commit_session(
    file_path: String,
    edit_uuid: Option<String>,
    new_text: Option<String>,
    custom_title: Option<String>,
) -> Result<CommitResult, String> {
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let path = PathBuf::from(&file_path);
    let mut lines = read_raw(&path).map_err(|e| e.to_string())?;

    // Snapshot the active path's uuids (order) BEFORE any edit.
    let active_uuids: Vec<uuid::Uuid> = active_path(&lines).iter().filter_map(|l| l.uuid).collect();

    // Optionally edit the text of the message line identified by edit_uuid.
    if let (Some(uuid_str), Some(text)) = (edit_uuid, new_text.as_ref()) {
        let target = uuid::Uuid::parse_str(&uuid_str)
            .map_err(|e| format!("invalid edit uuid: {e}"))?;
        match lines.iter_mut().find(|l| l.uuid == Some(target)) {
            Some(line) => claude_manager_core::session::edit::set_message_text(line, text),
            None => return Err(format!("edit target {uuid_str} not found in session")),
        }
    }

    // Rebuild active-path refs (post-edit) by uuid, preserving original order.
    let by_uuid: std::collections::HashMap<uuid::Uuid, &RawLine> =
        lines.iter().filter_map(|l| l.uuid.map(|u| (u, l))).collect();
    let path_refs: Vec<&RawLine> = active_uuids.iter().filter_map(|u| by_uuid.get(u).copied()).collect();

    // Launch cwd (first cwd in file order) so the new session lands in the same
    // project dir as the source and stays resumable — not the active path's cwd,
    // which for a compacted session begins post-compaction in a cd'd directory.
    let cwd = first_cwd(&lines).unwrap_or_else(|| ".".to_string());
    let git_branch = path_refs
        .iter()
        .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let opts = CommitOpts { target_cwd: cwd, git_branch, custom_title };
    commit(&root, &path_refs, &opts).map_err(|e| e.to_string())
}

/// Rename a session in place by appending a `custom-title` event (the most-recent
/// custom-title wins). This is the one place we mutate the original file — a
/// custom-title is metadata and a rename must keep the same session id.
#[tauri::command]
pub fn rename_session(file_path: String, new_title: String) -> Result<(), String> {
    let path = PathBuf::from(&file_path);
    claude_manager_core::session::edit::append_custom_title(&path, &new_title)
        .map_err(|e| e.to_string())
}

/// Move a session file to the app trash (recoverable). Returns Ok(()) on success.
#[tauri::command]
pub fn delete_session(file_path: String) -> Result<(), String> {
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    store::trash_session(&root, std::path::Path::new(&file_path)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Fork: write the prefix up to and including the event identified by `fork_uuid`
/// as a new session (copy-on-edit). Resolving by uuid against the current file
/// (not a positional `fork_index`) is required: the live-tail renders streamed
/// turns with batch-local indices, so an index can truncate the fork; the uuid
/// is stable and found at its true position here.
#[tauri::command]
pub fn fork_session(
    file_path: String,
    fork_uuid: String,
    custom_title: Option<String>,
) -> Result<CommitResult, String> {
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let path = PathBuf::from(&file_path);
    let lines = read_raw(&path).map_err(|e| e.to_string())?;

    // Walk from the fork point back to its own tree's root. This works for any
    // message — including one in an orphaned pre-compaction tree — so forking a
    // pre-compaction message yields the full raw history up to that point.
    let fork_target = uuid::Uuid::parse_str(&fork_uuid)
        .map_err(|e| format!("invalid fork uuid: {e}"))?;
    let prefix = claude_manager_core::session::raw::path_to(&lines, fork_target);
    if prefix.is_empty() {
        return Err(format!("fork point {fork_uuid} not found in session"));
    }
    // Launch cwd (first cwd in file order) so the fork lands in the same project
    // dir as the source and stays resumable — not the active-path prefix's cwd,
    // which for a compacted session begins post-compaction in a cd'd directory.
    let cwd = first_cwd(&lines).unwrap_or_else(|| ".".to_string());
    let git_branch = prefix
        .iter()
        .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let opts = CommitOpts { target_cwd: cwd, git_branch, custom_title };
    commit(&root, &prefix, &opts).map_err(|e| e.to_string())
}

/// Delete a single message (turn) by its uuid, writing a new flattened session
/// (copy-on-edit). Reuses the surgery prune path: passing the turn's uuid as
/// both ends of the range turn-snaps to the whole turn (an assistant turn pulls
/// in its tool calls + results), repairs tool-pairing across the cut, and
/// refuses a delete that would leave a leading assistant message.
#[tauri::command]
pub fn delete_message(file_path: String, message_uuid: String) -> Result<CommitResult, String> {
    use claude_manager_core::session::surgery::{rewrite, RewriteMode};
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let path = PathBuf::from(&file_path);
    let id = uuid::Uuid::parse_str(&message_uuid)
        .map_err(|e| format!("invalid message uuid: {e}"))?;
    let out = rewrite(&root, &path, id, id, RewriteMode::Prune, None).map_err(|e| e.to_string())?;
    Ok(out.commit)
}
