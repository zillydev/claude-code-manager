use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::sync::Mutex;

use crate::terminal::{self, TerminalState};

use claude_code_manager_core::session::clone::{clone_prefix, CloneOpts};
use claude_code_manager_core::session::edit::CommitResult;
use claude_code_manager_core::session::parse::parse_tail;
use claude_code_manager_core::session::raw::{first_cwd, read_raw};
use claude_code_manager_core::session::render::{render_session, RenderedSession, RenderedTurn};
use claude_code_manager_core::session::summary::SessionSummary;
use claude_code_manager_core::session::tree;
use claude_code_manager_core::store;
use claude_code_manager_core::store::paths;
use claude_code_manager_core::store::watcher;

use crate::shared::Shared;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Mutable state shared between tauri commands — one live-tail watcher per
/// window, keyed by the window's label, so multiple windows tail different
/// sessions without clobbering each other.
#[derive(Default)]
pub struct WatcherState {
    pub watchers: HashMap<String, watcher::FileWatcher>,
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

#[derive(serde::Deserialize)]
pub struct SearchFileDto {
    pub file_path: String,
    pub session_id: String,
    pub only_uuids: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct MessageHitDto {
    pub session_id: String,
    pub uuid: Option<String>,
    pub role: String,
    pub ts: Option<i64>,
    pub snippet: String,
    pub match_count: usize,
}

#[derive(Serialize)]
pub struct SearchResultDto {
    pub hits: Vec<MessageHitDto>,
    pub truncated: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFiltersDto {
    pub text: String,
    pub role: Option<String>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub tool: Option<String>,
    pub has_link: bool,
    #[serde(default)]
    pub skip: usize,
}

/// The global-find engine: candidate files come prefiltered from the frontend
/// (project/tag/session/mtime narrowing happens there, where the metadata is).
#[tauri::command]
pub async fn search_messages(
    files: Vec<SearchFileDto>,
    filters: SearchFiltersDto,
) -> Result<SearchResultDto, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<SearchResultDto, String> {
        let files: Vec<store::SearchFile> = files
            .into_iter()
            .filter_map(|f| {
                Some(store::SearchFile {
                    path: PathBuf::from(f.file_path),
                    session_id: uuid::Uuid::parse_str(&f.session_id).ok()?,
                    only_uuids: f.only_uuids.map(|us| {
                        us.iter().filter_map(|u| uuid::Uuid::parse_str(u).ok()).collect()
                    }),
                })
            })
            .collect();
        let filters = store::MessageFilters {
            text: filters.text,
            role: filters.role,
            after: filters.after,
            before: filters.before,
            tool: filters.tool,
            has_link: filters.has_link,
            limit: 200,
            skip: filters.skip,
        };
        let (hits, truncated) = store::search_messages(&files, &filters);
        Ok(SearchResultDto {
            hits: hits
                .into_iter()
                .map(|h| MessageHitDto {
                    session_id: h.session_id.to_string(),
                    uuid: h.uuid.map(|u| u.to_string()),
                    role: h.role,
                    ts: h.ts,
                    snippet: h.snippet,
                    match_count: h.match_count,
                })
                .collect(),
            truncated,
        })
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
        let mini = claude_code_manager_core::session::Session {
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

        // Back-fill results for tool calls rendered in an EARLIER batch: a slow
        // tool's result arrives minutes after its call, in its own tail read, so
        // the mini-render above can't pair it (its tool_use isn't in this batch)
        // — the transcript's already-appended call would stay "(no result)"
        // forever. Same-batch pairs are already embedded in the turns; the
        // frontend only patches calls still missing a result. Emitted even when
        // no turns rendered — a pure tool-result message is suppressed
        // machinery, producing exactly the turnless batch that needs this.
        let updates = claude_code_manager_core::session::render::tool_result_updates(&mini.events);
        if !updates.is_empty() {
            let _ = app.emit_to(label.as_str(), "session-tool-results", &updates);
        }

        if !new_turns.is_empty() {
            let _ = app.emit_to(label.as_str(), "session-updated", &new_turns);
        }
        offset = outcome.bytes_read;
    }
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

/// Fork: copy the source file's lines up to `fork_uuid` in FILE order as a new
/// session, changing only what session identity requires (see
/// [`session::clone`](claude_code_manager_core::session::clone)). Nothing is
/// flattened, trimmed, or re-uuid'd, so abandoned branches, pre-compaction
/// segments, attachments and system lines all come along and the copy carries
/// exactly the history the source had at that point.
///
/// Resolving by uuid against the current file (not a positional index) is
/// required: the live-tail renders streamed turns with batch-local indices, so
/// an index can truncate the fork; the uuid is stable and found at its true
/// position here.
#[tauri::command]
pub async fn fork_session(
    file_path: String,
    fork_uuid: String,
    custom_title: Option<String>,
) -> Result<CommitResult, String> {
    // read_raw + write on a potentially-large file — off the main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<CommitResult, String> {
        let root = paths::default_claude_root().ok_or("could not locate ~/.claude")?;
        let path = PathBuf::from(&file_path);
        let lines = read_raw(&path).map_err(|e| e.to_string())?;
        let cut = uuid::Uuid::parse_str(&fork_uuid)
            .map_err(|e| format!("invalid fork uuid: {e}"))?;
        // Launch cwd (first cwd in file order) so the copy lands in the same
        // project dir as the source and stays resumable.
        let cwd = first_cwd(&lines).unwrap_or_else(|| ".".to_string());
        let opts = CloneOpts { target_cwd: cwd, custom_title };
        clone_prefix(&root, &lines, cut, &opts).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct MessagePreview {
    pub uuid: String,
    pub snippet: String,
    /// Epoch seconds of the message, when known.
    pub timestamp: Option<i64>,
}

/// One-line previews (+ time) for specific message uuids in one session file —
/// the bookmark browser labels bookmarks of sessions that aren't open. Uuids
/// that don't resolve to a text turn are simply omitted; the frontend renders
/// its own fallback for those.
#[tauri::command]
pub async fn message_previews(
    file_path: String,
    uuids: Vec<String>,
) -> Result<Vec<MessagePreview>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<MessagePreview>, String> {
        let wanted: std::collections::HashSet<uuid::Uuid> =
            uuids.iter().filter_map(|u| uuid::Uuid::parse_str(u).ok()).collect();
        let found = store::message_previews(std::path::Path::new(&file_path), &wanted);
        Ok(found
            .into_iter()
            .map(|(u, (snippet, ts))| MessagePreview {
                uuid: u.to_string(),
                snippet,
                timestamp: ts.map(|t| t.timestamp()),
            })
            .collect())
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

/// Load the user config (creating it with defaults on first run).
#[tauri::command]
pub fn get_config() -> crate::config::Config {
    crate::config::load()
}

/// Persist the user config and broadcast it to every window so they re-apply
/// the new theme/typography/behavior live.
#[tauri::command]
pub fn set_config(config: crate::config::Config, app: AppHandle) -> Result<(), String> {
    crate::config::save(&config).map_err(|e| e.to_string())?;
    let _ = app.emit("config-changed", &config);
    Ok(())
}

/// Load the session-organization metadata (creating the file on first run).
#[tauri::command]
pub fn get_session_meta() -> crate::session_meta::SessionMeta {
    crate::session_meta::load()
}

/// Persist session-organization metadata and broadcast it to every window.
#[tauri::command]
pub fn set_session_meta(
    meta: crate::session_meta::SessionMeta,
    app: AppHandle,
) -> Result<(), String> {
    crate::session_meta::save(&meta).map_err(|e| e.to_string())?;
    let _ = app.emit("session-meta-changed", &meta);
    Ok(())
}

/// The frontend's per-session last-viewed map (localStorage `cm:lastViewed`),
/// pushed so the backend can split "unread" from "already seen" in the
/// published status file. Merged per session by maximum across windows.
#[tauri::command]
pub fn set_last_viewed(
    map: std::collections::BTreeMap<String, i64>,
    shared: tauri::State<'_, crate::shared::Shared>,
) {
    shared.merge_last_viewed(map);
}

/// Install / remove / probe the Claude Code hook relay (Settings toggle).
/// State lives in `~/.claude/settings.json` itself — no ccm config duplicates it.
#[tauri::command]
pub fn hooks_install() -> Result<(), String> {
    crate::hooks::install()
}

#[tauri::command]
pub fn hooks_uninstall() -> Result<(), String> {
    crate::hooks::uninstall()
}

#[tauri::command]
pub fn hooks_installed() -> bool {
    crate::hooks::installed()
}

#[derive(Serialize, Clone)]
pub struct SessionActivity {
    pub id: String,
    /// Last real-event time, epoch seconds.
    pub last_event_at: i64,
    /// Whether the session's last turn is a completed assistant turn (claude is
    /// done). Distinguishes a thinking pause (still running) from "unread".
    pub awaiting_input: bool,
    /// ccm is running this session's `claude` process (a live PTY in a terminal
    /// tab). Then liveness is known rather than inferred, so consumers skip the
    /// staleness backstop — a 20-minute tool call is still "running".
    #[serde(default)]
    pub hosted: bool,
    /// Hosted AND claude authored real output recently — not just alive, and
    /// not merely repainting in answer to the user's own keys.
    pub busy: bool,
    /// Claude's own account of its state, from the opt-in hook relay: the one
    /// source that can see a question the session file hasn't recorded yet.
    pub hook: Option<crate::hooks::HookPhase>,
    /// Epoch seconds this session's terminal was last visibly working. Only
    /// meaningful while `hosted` (0 otherwise).
    ///
    /// Once it goes quiet, this is the moment claude started waiting on you —
    /// which the session file cannot tell anyone, because it records a question
    /// only after it is answered, leaving your own prompt as the last event.
    #[serde(default)]
    pub busy_at: i64,
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

#[derive(serde::Deserialize)]
pub struct BranchRef {
    pub file_path: String,
    pub message_uuid: String,
}

#[derive(Serialize)]
pub struct BranchSnippet {
    pub message_uuid: String,
    pub snippet: String,
    /// Whether this message is actually rendered in the parent's transcript.
    /// A branch point on an edited-away branch resolves a snippet (we scan the
    /// whole file) but isn't shown, so the UI can't scroll to it.
    pub on_path: bool,
}

/// Resolve short previews for fork/copy branch-point messages. Each branch
/// message lives in a parent session's file; groups by file so each parent is
/// parsed once. Unresolvable uuids are simply omitted. Off the UI thread.
#[tauri::command]
pub async fn resolve_branch_messages(refs: Vec<BranchRef>) -> Result<Vec<BranchSnippet>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<BranchSnippet>, String> {
        use std::collections::{HashMap, HashSet};
        let mut by_file: HashMap<String, HashSet<uuid::Uuid>> = HashMap::new();
        for r in &refs {
            if let Ok(id) = uuid::Uuid::parse_str(&r.message_uuid) {
                by_file.entry(r.file_path.clone()).or_default().insert(id);
            }
        }
        let mut snippets: HashMap<uuid::Uuid, String> = HashMap::new();
        let mut on_path: HashSet<uuid::Uuid> = HashSet::new();
        for (file, wanted) in &by_file {
            let path = std::path::Path::new(file);
            snippets.extend(store::message_snippets(path, wanted));
            // A branch point is reachable only if the transcript actually renders
            // it. Render the parent (same path the UI uses) and collect every
            // shown turn's uuids; a message on an edited-away branch won't appear.
            if let Ok(session) = store::open_session(path.to_path_buf(), String::new()) {
                for t in &render_session(&session).turns {
                    if let RenderedTurn::User { uuids, .. } | RenderedTurn::Assistant { uuids, .. } =
                        t
                    {
                        for u in uuids {
                            if let Ok(id) = uuid::Uuid::parse_str(u) {
                                if wanted.contains(&id) {
                                    on_path.insert(id);
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for r in &refs {
            if let Ok(id) = uuid::Uuid::parse_str(&r.message_uuid) {
                if seen.insert(id) {
                    if let Some(s) = snippets.get(&id) {
                        out.push(BranchSnippet {
                            message_uuid: r.message_uuid.clone(),
                            snippet: s.clone(),
                            on_path: on_path.contains(&id),
                        });
                    }
                }
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
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
