//! Closing a window or quitting kills the claude processes it hosts. When any are hosted, the
//! frontend is asked first — it knows which of those sessions Claude is still working in.

use std::sync::Mutex;

use tauri::{AppHandle, CloseRequestApi, Emitter, Manager, Window};

use crate::terminal::{Hosted, TerminalState};

/// The session ids a close would kill: those hosted by `window`, or every one for a quit (`None`).
pub fn affected_sessions(hosted: &[Hosted], window: Option<&str>) -> Vec<String> {
    hosted
        .iter()
        .filter(|h| window.is_none_or(|w| h.owner == w))
        .map(|h| h.session_id.clone())
        .collect()
}

fn hosted(app: &AppHandle) -> Vec<Hosted> {
    app.try_state::<Mutex<TerminalState>>()
        .and_then(|s| s.lock().ok().map(|g| g.hosted()))
        .unwrap_or_default()
}

/// ⌘W or the close button: a window hosting no terminal closes as before; otherwise it is asked.
pub fn on_close_requested(win: &Window, api: &CloseRequestApi) {
    let app = win.app_handle();
    let ids = affected_sessions(&hosted(app), Some(win.label()));
    if ids.is_empty() {
        return;
    }
    api.prevent_close();
    let _ = app.emit_to(win.label(), "close-requested", ids);
}

/// ⌘Q, as a custom menu item: tao handles `applicationWillTerminate` but not `applicationShouldTerminate`,
/// so the predefined Quit ends the app with no event to cancel. Dock quit and logout still bypass this.
pub fn request_quit(app: &AppHandle) {
    let ids = affected_sessions(&hosted(app), None);
    let windows = app.webview_windows();
    let target = windows.values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| windows.values().next());
    match target {
        Some(w) if !ids.is_empty() => {
            let _ = app.emit_to(w.label(), "quit-requested", ids);
        }
        _ => app.exit(0),
    }
}

#[tauri::command]
pub fn confirm_close(window: tauri::WebviewWindow) -> Result<(), String> {
    window.destroy().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn confirm_quit(app: AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosted(id: &str, owner: &str) -> Hosted {
        Hosted { session_id: id.into(), owner: owner.into(), last_busy_at: 0, last_interrupt_at: 0, program: None, cwd: String::new(), started_at: 0 }
    }

    #[test]
    fn a_window_close_affects_only_that_windows_terminals() {
        let pool = [hosted("a", "main"), hosted("b", "win-2"), hosted("c", "main")];
        let mut ids = affected_sessions(&pool, Some("main"));
        ids.sort();
        assert_eq!(ids, vec!["a", "c"]);
        assert!(affected_sessions(&pool, Some("win-3")).is_empty());
    }

    #[test]
    fn a_quit_affects_every_terminal() {
        let pool = [hosted("a", "main"), hosted("b", "win-2")];
        assert_eq!(affected_sessions(&pool, None).len(), 2);
    }
}
