//! Dev-only Develop → Rebuild & Relaunch for `make dev` (scripts/dev.sh): build
//! while the current windows stay up, then exit with `RELAUNCH_EXIT_CODE` so the
//! launcher starts the new binary, which restores each window's frame and view.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::menu::{MenuItem, Submenu};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewWindow};

/// Exit code scripts/dev.sh reads as "start me again".
pub const RELAUNCH_EXIT_CODE: i32 = 75;

pub const MENU_ID: &str = "dev_rebuild";

/// A restore file older than this belongs to a relaunch that never happened.
const RESTORE_MAX_AGE_SECS: i64 = 120;

/// Lines of compiler output the failure banner shows.
const EXCERPT_MAX_LINES: usize = 60;

/// One window as it was when the rebuild finished (logical pixels).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowRestore {
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Whatever the window's frontend last reported via `dev_set_view`.
    pub view: Option<Value>,
}

#[derive(Serialize, Deserialize)]
struct RestoreFile {
    saved_at: i64,
    windows: Vec<WindowRestore>,
}

#[derive(Default)]
pub struct DevReloadState {
    building: AtomicBool,
    relaunch: AtomicBool,
    /// Latest view each window reported, by window label.
    views: Mutex<HashMap<String, Value>>,
    /// Views read from the restore file at startup, each handed out once.
    restored: Mutex<HashMap<String, Value>>,
}

impl DevReloadState {
    pub fn relaunch_requested(&self) -> bool {
        self.relaunch.load(Ordering::SeqCst)
    }
}

/// Broadcast as `dev-rebuild` so every window's banner follows the build.
#[derive(Clone, Serialize)]
#[serde(tag = "phase", rename_all = "lowercase")]
enum Progress {
    Building,
    Relaunching,
    Failed { output: String },
}

/// Only a debug build started by scripts/dev.sh: anything else has no launcher
/// to start it again after the relaunch exit.
pub fn available() -> bool {
    cfg!(debug_assertions) && std::env::var_os("CM_DEV_LAUNCHER").is_some()
}

fn restore_path() -> PathBuf {
    crate::config::config_dir().join("dev-restore.json")
}

pub fn menu(handle: &AppHandle) -> tauri::Result<Submenu<tauri::Wry>> {
    let rebuild =
        MenuItem::with_id(handle, MENU_ID, "Rebuild & Relaunch", true, Some("CmdOrCtrl+Shift+R"))?;
    Submenu::with_items(handle, "Develop", true, &[&rebuild])
}

/// Build in the background; on success save the windows and exit for relaunch,
/// on failure report the errors and keep running. A second request mid-build is ignored.
pub fn start_rebuild(app: &AppHandle) {
    if !available() || app.state::<DevReloadState>().building.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = app.emit("dev-rebuild", Progress::Building);
        let state = app.state::<DevReloadState>();
        match cargo_build() {
            Ok(()) => {
                let _ = app.emit("dev-rebuild", Progress::Relaunching);
                if let Err(e) = write_restore(&restore_path(), &capture_windows(&app), now_secs()) {
                    eprintln!("[dev] could not save window state: {e}");
                }
                state.relaunch.store(true, Ordering::SeqCst);
                // Tauri drops a non-zero code here; the Exit handler exits with ours.
                app.exit(0);
            }
            Err(output) => {
                state.building.store(false, Ordering::SeqCst);
                let _ = app.emit("dev-rebuild", Progress::Failed { output });
            }
        }
    });
}

/// `cargo build` of this crate with the inherited environment — scripts/dev.sh's
/// `TAURI_CONFIG` included, so the build matches the launcher's and stays incremental.
fn cargo_build() -> Result<(), String> {
    let mut child = Command::new("cargo")
        .args(["build", "--color", "never"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    let mut lines = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("{line}");
            lines.push(line);
        }
    }
    match child.wait() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(error_excerpt(&lines)),
        Err(e) => Err(format!("cargo did not finish: {e}")),
    }
}

fn capture_windows(app: &AppHandle) -> Vec<WindowRestore> {
    let views = app.state::<DevReloadState>().views.lock().unwrap().clone();
    let mut out: Vec<WindowRestore> = app
        .webview_windows()
        .into_iter()
        .filter_map(|(label, w)| {
            let scale = w.scale_factor().ok()?;
            let pos = w.outer_position().ok()?.to_logical::<f64>(scale);
            let size = w.inner_size().ok()?.to_logical::<f64>(scale);
            Some(WindowRestore {
                view: views.get(&label).cloned(),
                label,
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
            })
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

/// Recreate the windows a relaunch saved. Runs in setup, after Tauri has
/// already opened the configured `main` window.
pub fn restore_windows(app: &AppHandle) {
    if !available() {
        return;
    }
    let Some(windows) = take_restore(&restore_path(), now_secs()) else { return };
    app.state::<DevReloadState>().restored.lock().unwrap().extend(
        windows.iter().filter_map(|w| Some((w.label.clone(), w.view.clone()?))),
    );
    for w in &windows {
        if w.label == "main" {
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.set_position(LogicalPosition::new(w.x, w.y));
                let _ = main.set_size(LogicalSize::new(w.width, w.height));
            }
        } else {
            crate::window::build_window(app, &w.label, (w.x, w.y), (w.width, w.height));
        }
    }
    // `main` had been closed before the rebuild; drop it only once others exist.
    if !windows.is_empty() && !windows.iter().any(|w| w.label == "main") {
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.destroy();
        }
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Serialize)]
pub struct DevView {
    available: bool,
    view: Option<Value>,
}

#[tauri::command]
pub fn dev_rebuild(app: AppHandle) {
    start_rebuild(&app);
}

/// Called once per window on mount: whether dev relaunch is on, and the view a
/// relaunch saved for this window's label.
#[tauri::command]
pub fn dev_take_view(window: WebviewWindow, state: State<'_, DevReloadState>) -> DevView {
    let view = if available() { state.restored.lock().unwrap().remove(window.label()) } else { None };
    DevView { available: available(), view }
}

#[tauri::command]
pub fn dev_set_view(window: WebviewWindow, view: Value, state: State<'_, DevReloadState>) {
    if available() {
        state.views.lock().unwrap().insert(window.label().to_string(), view);
    }
}

fn write_restore(path: &Path, windows: &[WindowRestore], now: i64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = RestoreFile { saved_at: now, windows: windows.to_vec() };
    std::fs::write(path, serde_json::to_vec(&file)?)
}

/// Read and delete the restore file: `None` when absent, malformed, or stale.
fn take_restore(path: &Path, now: i64) -> Option<Vec<WindowRestore>> {
    let bytes = std::fs::read(path).ok()?;
    let _ = std::fs::remove_file(path);
    let file: RestoreFile = serde_json::from_slice(&bytes).ok()?;
    (now - file.saved_at <= RESTORE_MAX_AGE_SECS).then_some(file.windows)
}

/// The part of a failed `cargo build`'s stderr worth showing: from the first
/// error on, capped; the `make dev` terminal keeps the full log.
fn error_excerpt(lines: &[String]) -> String {
    // No error line (e.g. a build script's own failure message): show the tail.
    let start = lines
        .iter()
        .position(|l| l.starts_with("error"))
        .unwrap_or(lines.len().saturating_sub(EXCERPT_MAX_LINES));
    let rest = &lines[start..];
    let mut out: Vec<&str> = rest.iter().take(EXCERPT_MAX_LINES).map(String::as_str).collect();
    if rest.len() > EXCERPT_MAX_LINES {
        out.push("… (full output in the make dev terminal)");
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ccm-dev-reload-{}-{name}.json", std::process::id()))
    }

    fn win(label: &str) -> WindowRestore {
        WindowRestore {
            label: label.into(),
            x: 10.0,
            y: 20.0,
            width: 1200.0,
            height: 800.0,
            view: Some(serde_json::json!({ "sidebarTab": "find" })),
        }
    }

    #[test]
    fn excerpt_starts_at_the_first_error_skipping_warnings() {
        let out = error_excerpt(&lines(&[
            "   Compiling claude-code-manager-app v0.1.2",
            "warning: unused variable: `x`",
            "error[E0308]: mismatched types",
            " --> src/lib.rs:3:5",
            "error: could not compile `claude-code-manager-app`",
        ]));
        assert!(out.starts_with("error[E0308]: mismatched types"), "{out}");
        assert!(!out.contains("warning"));
        assert!(out.ends_with("could not compile `claude-code-manager-app`"));
    }

    #[test]
    fn excerpt_caps_long_output_and_points_at_the_terminal() {
        let mut v = vec!["error: boom".to_string()];
        v.extend((0..200).map(|i| format!("line {i}")));
        let out = error_excerpt(&v);
        assert_eq!(out.lines().count(), EXCERPT_MAX_LINES + 1);
        assert!(out.lines().last().unwrap().contains("make dev"));
    }

    #[test]
    fn excerpt_without_an_error_line_keeps_the_tail() {
        // e.g. a build script that failed with its own message
        let v: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
        let out = error_excerpt(&v);
        assert!(out.ends_with("line 99"));
        assert_eq!(out.lines().count(), EXCERPT_MAX_LINES);
    }

    #[test]
    fn restore_round_trips_and_is_consumed() {
        let path = temp_path("roundtrip");
        let windows = vec![win("main"), win("win-1")];
        write_restore(&path, &windows, 1_000).unwrap();
        assert_eq!(take_restore(&path, 1_005), Some(windows));
        assert!(!path.exists(), "taking the file must delete it");
        assert_eq!(take_restore(&path, 1_005), None);
    }

    #[test]
    fn stale_restore_is_ignored_but_still_removed() {
        let path = temp_path("stale");
        write_restore(&path, &[win("main")], 1_000).unwrap();
        assert_eq!(take_restore(&path, 1_000 + RESTORE_MAX_AGE_SECS + 1), None);
        assert!(!path.exists());
    }

    #[test]
    fn malformed_restore_is_ignored() {
        let path = temp_path("malformed");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(take_restore(&path, 1_000), None);
        assert!(!path.exists());
    }
}
