use std::sync::mpsc::channel;
use std::sync::Mutex;

use claude_manager_core::session::parse::parse_tail;
use claude_manager_core::session::render::{render_session, RenderedSession, RenderedTurn};
use claude_manager_core::session::summary::SessionSummary;
use claude_manager_core::session::tree;
use claude_manager_core::store;
use claude_manager_core::store::paths;
use claude_manager_core::store::watcher;
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
