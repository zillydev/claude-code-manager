use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::sync::Mutex;

use crate::terminal::{self, TerminalState};

use claude_manager_core::session::edit::{commit, CommitOpts, CommitResult};
use claude_manager_core::session::parse::parse_tail;
use claude_manager_core::session::raw::{active_path, first_cwd, read_raw, RawLine};
use claude_manager_core::session::render::{render_session, RenderedSession, RenderedTurn};
use claude_manager_core::session::summary::SessionSummary;
use claude_manager_core::session::tree;
use claude_manager_core::store;
use claude_manager_core::store::paths;
use claude_manager_core::store::watcher;

use crate::shared::Shared;
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

/// The sessions list, served from the app-global `Shared` cache (computed once,
/// single-flight, refreshed by the background poller). discover() fully parses
/// every session file (~6s debug), so it runs at most once across all windows;
/// the blocking pool keeps it off the Tauri main thread on a cold cache.
#[tauri::command]
pub async fn list_sessions(app: AppHandle) -> Result<Vec<SessionSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<SessionSummary>, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        app.state::<Shared>().sessions(&root)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Force a re-discovery (manual ↻) and broadcast so every window re-pulls.
#[tauri::command]
pub async fn refresh_sessions(app: AppHandle) -> Result<Vec<SessionSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<SessionSummary>, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        let sessions = app.state::<Shared>().refresh_sessions(&root)?;
        let _ = app.emit("sessions-changed", ());
        Ok(sessions)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn search_transcripts(query: String) -> Result<Vec<String>, String> {
    // Searches every transcript on disk — keep it off the main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<String>, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        let ids = store::search_sessions(&root, &query).map_err(|e| e.to_string())?;
        Ok(ids.into_iter().map(|u| u.to_string()).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_session(
    file_path: String,
    project_label: String,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<RenderedSession, String> {
    // Parsing+rendering the file can be large (tens of MB) — do it off the main
    // thread so opening a big session doesn't freeze the window. The watcher
    // setup below stays on the command thread (it touches Tauri state).
    let fp_parse = file_path.clone();
    let (rendered, bytes_read) =
        tauri::async_runtime::spawn_blocking(move || -> Result<(RenderedSession, u64), String> {
            let session = store::open_session(fp_parse.into(), project_label)
                .map_err(|e| e.to_string())?;
            let rendered = render_session(&session);
            Ok((rendered, session.bytes_read))
        })
        .await
        .map_err(|e| e.to_string())??;

    let label = window.label().to_string();

    // Start file watcher for live tail, scoped to THIS window.
    let state = app.state::<Mutex<WatcherState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;

    // Drop any previous watcher for this window only.
    guard.watchers.remove(&label);

    let (tx, rx) = channel::<()>();
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
pub async fn open_for_edit(file_path: String) -> Result<EditableSession, String> {
    // read_raw parses the whole file — off the main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<EditableSession, String> {
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
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Commit an edit: optionally set the text of the message-turn identified by
/// `edit_uuid`, then write the full active path as a new flattened session
/// (copy-on-edit). Resolving by uuid (not a positional index) is required: the
/// live-tail renders streamed turns with batch-local indices, so an index can
/// point at the wrong message; the uuid is stable.
#[tauri::command]
pub async fn commit_session(
    file_path: String,
    edit_uuid: Option<String>,
    new_text: Option<String>,
    custom_title: Option<String>,
) -> Result<CommitResult, String> {
    // read_raw + commit (write) on a potentially-large file — off the main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<CommitResult, String> {
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
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Rename a session in place by appending a `custom-title` event (the most-recent
/// custom-title wins). This is the one place we mutate the original file — a
/// custom-title is metadata and a rename must keep the same session id.
#[tauri::command]
pub async fn rename_session(file_path: String, new_title: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let path = PathBuf::from(&file_path);
        claude_manager_core::session::edit::append_custom_title(&path, &new_title)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Move a session file to the app trash (recoverable). Returns Ok(()) on success.
#[tauri::command]
pub async fn delete_session(file_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        store::trash_session(&root, std::path::Path::new(&file_path)).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Fork: write the prefix up to and including the event identified by `fork_uuid`
/// as a new session (copy-on-edit). Resolving by uuid against the current file
/// (not a positional `fork_index`) is required: the live-tail renders streamed
/// turns with batch-local indices, so an index can truncate the fork; the uuid
/// is stable and found at its true position here.
#[tauri::command]
pub async fn fork_session(
    file_path: String,
    fork_uuid: String,
    custom_title: Option<String>,
) -> Result<CommitResult, String> {
    // read_raw + commit (write) on a potentially-large file — off the main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<CommitResult, String> {
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
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Delete a single message (turn) by its uuid, writing a new flattened session
/// (copy-on-edit). Reuses the surgery prune path: passing the turn's uuid as
/// both ends of the range turn-snaps to the whole turn (an assistant turn pulls
/// in its tool calls + results), repairs tool-pairing across the cut, and
/// refuses a delete that would leave a leading assistant message.
#[tauri::command]
pub async fn delete_message(file_path: String, message_uuid: String) -> Result<CommitResult, String> {
    use claude_manager_core::session::surgery::{rewrite, RewriteMode};
    // rewrite reads + rewrites a potentially-large file — off the main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<CommitResult, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        let path = PathBuf::from(&file_path);
        let out = rewrite(&root, &path, &message_uuid, &message_uuid, RewriteMode::Prune, None)
            .map_err(|e| e.to_string())?;
        Ok(out.commit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct TerminalOpenResult {
    /// "started" | "reused" | "busy".
    pub status: String,
    /// Owner window label when `busy`, so the frontend can offer "switch to it".
    pub owner: Option<String>,
}

/// Open the terminal for a session in this window. One `claude` PTY per session
/// app-wide: reuse if this window already owns it, refuse ("busy") if another
/// window does, else spawn. Blocking a cross-window open prevents two
/// `claude --resume` processes from corrupting the same `.jsonl`.
#[tauri::command]
pub fn terminal_open(
    session_id: String,
    file_path: String,
    rows: u16,
    cols: u16,
    new_cwd: Option<String>,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<TerminalOpenResult, String> {
    let label = window.label().to_string();
    // Resolve cwd/fresh BEFORE locking (file read, no pool access).
    // `new_cwd` Some → a brand-new session: spawn fresh (`--session-id`) in that
    // dir. None → resume an existing session, cwd from its first cwd event.
    let (cwd, fresh) = match new_cwd {
        Some(c) => (c, true),
        None => {
            let lines = read_raw(std::path::Path::new(&file_path)).map_err(|e| e.to_string())?;
            let cwd = first_cwd(&lines)
                .ok_or("could not determine the session's working directory")?;
            (cwd, false)
        }
    };

    let state = app.state::<Mutex<TerminalState>>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    match terminal::open_decision(guard.owner(&session_id), &label) {
        terminal::OpenDecision::Reuse => {
            Ok(TerminalOpenResult { status: "reused".into(), owner: None })
        }
        terminal::OpenDecision::Busy => {
            let owner = guard.owner(&session_id).map(str::to_string);
            Ok(TerminalOpenResult { status: "busy".into(), owner })
        }
        terminal::OpenDecision::Spawn => {
            // Spawn under the lock so two windows racing the same fresh session
            // can't both spawn (the owner is recorded before the lock releases).
            let handle = terminal::spawn_terminal(
                app.clone(),
                label.clone(),
                session_id.clone(),
                cwd,
                rows,
                cols,
                fresh,
            )
            .map_err(|e| format!("failed to start terminal: {e}"))?;
            guard.insert(&session_id, handle);
            Ok(TerminalOpenResult { status: "started".into(), owner: None })
        }
    }
}

/// Cheap existence check — used to poll for a new session's file appearing
/// (claude writes it on the first turn).
#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).is_file()
}

#[derive(Serialize, Clone)]
pub struct SessionActivity {
    pub id: String,
    /// Last real-event time, epoch seconds.
    pub last_event_at: i64,
    /// Whether the session's last turn is a completed assistant turn (claude is
    /// done). Distinguishes a thinking pause (still running) from "unread".
    pub awaiting_input: bool,
}

/// The sidebar "running"/"unread" signal, served from the `Shared` cache that
/// the background poller refreshes. A cheap clone — the actual scan happens once
/// app-wide in the poller, not per window. Windows get live updates via the
/// "activity-updated" broadcast; this is just the initial pull on mount.
#[tauri::command]
pub fn session_activity(app: AppHandle) -> Result<Vec<SessionActivity>, String> {
    Ok(app.state::<Shared>().activity())
}

#[derive(Serialize)]
pub struct NewSessionResult {
    pub session_id: String,
    pub file_path: String,
}

/// Allocate a new session id + its file path under a chosen working directory.
/// Does NOT spawn — the terminal owns the spawn (`terminal_open` with `new_cwd`),
/// so no output is lost to a reconnect. `claude --session-id` creates the file.
#[tauri::command]
pub fn new_session(cwd: String) -> Result<NewSessionResult, String> {
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(format!("not a directory: {cwd}"));
    }
    let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
    let id = uuid::Uuid::new_v4();
    let file_path = paths::session_file_path(&root, &cwd, id).to_string_lossy().to_string();
    Ok(NewSessionResult { session_id: id.to_string(), file_path })
}

#[tauri::command]
pub fn terminal_input(session_id: String, data: String, app: AppHandle) -> Result<(), String> {
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .write(&session_id, data.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn terminal_resize(
    session_id: String,
    rows: u16,
    cols: u16,
    app: AppHandle,
) -> Result<(), String> {
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .resize(&session_id, rows, cols);
    Ok(())
}

#[tauri::command]
pub fn terminal_close(session_id: String, app: AppHandle) -> Result<(), String> {
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .close(&session_id);
    Ok(())
}

/// Focus a window by label — used by the "switch to it" button when a session
/// is already open in another window. On macOS this jumps to that window's Space.
#[tauri::command]
pub fn focus_window(label: String, app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(&label) {
        w.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}
