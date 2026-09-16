//! `claude-code-manager://session/<uuid>` links: which session one names, and which window opens it.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::collections::HashSet;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use crate::terminal::TerminalState;

pub const SCHEME: &str = "claude-code-manager";

/// The session a link names (hyphenated, lowercase); `None` for anything but `claude-code-manager://session/<uuid>`.
pub fn parse_session_link(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let id = rest.strip_prefix("session/")?.trim_end_matches('/');
    uuid::Uuid::parse_str(id).ok().map(|u| u.to_string())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Window(String),
    NewWindow,
}

/// Where a link to `session` opens: the window showing it, else the one running its terminal, else the last
/// focused, else any (`main` first) — or a new window when none exist.
pub fn choose_window(
    session: &str,
    windows: &[String],
    open: &HashMap<String, String>,
    terminal_owner: Option<&str>,
    last_focused: Option<&str>,
) -> Target {
    let exists = |label: &str| windows.iter().any(|w| w == label);
    let mut ordered: Vec<&String> = windows.iter().collect();
    ordered.sort_by(|a, b| (a.as_str() != "main", a.as_str()).cmp(&(b.as_str() != "main", b.as_str())));
    ordered
        .iter()
        .find(|w| open.get(w.as_str()).is_some_and(|s| s == session))
        .map(|w| w.to_string())
        .or_else(|| terminal_owner.filter(|o| exists(o)).map(str::to_string))
        .or_else(|| last_focused.filter(|l| exists(l)).map(str::to_string))
        .or_else(|| ordered.first().map(|w| w.to_string()))
        .map_or(Target::NewWindow, Target::Window)
}

pub const REPEAT_WINDOW: Duration = Duration::from_secs(2);

/// Whether `session` repeats the last handled link within `REPEAT_WINDOW`: macOS can deliver a launch link twice.
pub fn is_repeat(last: Option<&(String, Instant)>, session: &str, now: Instant) -> bool {
    last.is_some_and(|(s, at)| s == session && now.duration_since(*at) < REPEAT_WINDOW)
}

/// What link routing knows about the windows, plus a link waiting for one of them to listen.
#[derive(Default)]
pub struct LinkState {
    /// Window label → the session it has open.
    open: HashMap<String, String>,
    last_focused: Option<String>,
    /// Windows whose frontend has attached its `open-session-link` listener.
    listening: HashSet<String>,
    pending: Option<String>,
    last_handled: Option<(String, Instant)>,
}

fn with_state(app: &AppHandle, f: impl FnOnce(&mut LinkState)) {
    if let Ok(mut state) = app.state::<Mutex<LinkState>>().lock() {
        f(&mut state);
    }
}

/// A URL the OS opened with the app: queue its session and try to deliver it.
pub fn handle_url(app: &AppHandle, url: &str) {
    let Some(session) = parse_session_link(url) else { return };
    let mut fresh = false;
    with_state(app, |s| {
        let now = Instant::now();
        if !is_repeat(s.last_handled.as_ref(), &session, now) {
            s.last_handled = Some((session.clone(), now));
            s.pending = Some(session.clone());
            fresh = true;
        }
    });
    if fresh {
        deliver_pending(app);
    }
}

/// Send the pending link to the window it belongs in, once that window's frontend listens: an event
/// emitted before then is dropped. Locks are taken one at a time and released before any window call.
fn deliver_pending(app: &AppHandle) {
    let mut pending = None;
    with_state(app, |s| pending = s.pending.clone());
    let Some(session) = pending else { return };
    let owner = app
        .state::<Mutex<TerminalState>>()
        .lock()
        .ok()
        .and_then(|t| t.owner(&session).map(str::to_string));
    let windows: Vec<String> = app.webview_windows().keys().cloned().collect();

    let mut target = Target::NewWindow;
    let mut ready = false;
    with_state(app, |s| {
        target = choose_window(&session, &windows, &s.open, owner.as_deref(), s.last_focused.as_deref());
        ready = matches!(&target, Target::Window(label) if s.listening.contains(label));
        if ready {
            s.pending = None;
        }
    });

    match target {
        Target::NewWindow => crate::window::spawn_window(app),
        Target::Window(label) if ready => {
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            let _ = app.emit_to(label.as_str(), "open-session-link", session);
        }
        Target::Window(_) => {} // stays pending until that window's frontend calls link_listener_ready
    }
}

pub fn record_open(app: &AppHandle, label: &str, session: &str) {
    with_state(app, |s| {
        s.open.insert(label.to_string(), session.to_string());
    });
}

pub fn clear_open(app: &AppHandle, label: &str) {
    with_state(app, |s| {
        s.open.remove(label);
    });
}

pub fn focused(app: &AppHandle, label: &str) {
    with_state(app, |s| s.last_focused = Some(label.to_string()));
}

pub fn forget_window(app: &AppHandle, label: &str) {
    with_state(app, |s| {
        s.open.remove(label);
        s.listening.remove(label);
        if s.last_focused.as_deref() == Some(label) {
            s.last_focused = None;
        }
    });
}

/// A window's frontend is listening for `open-session-link`; hand it any link that was waiting for it.
#[tauri::command]
pub fn link_listener_ready(window: tauri::WebviewWindow, app: AppHandle) {
    with_state(&app, |s| {
        s.listening.insert(window.label().to_string());
    });
    deliver_pending(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0dd9d545-8a0a-44cc-a85b-23bb6896da97";

    #[test]
    fn parses_a_session_link() {
        assert_eq!(parse_session_link(&format!("claude-code-manager://session/{ID}")), Some(ID.into()));
        assert_eq!(parse_session_link(&format!("claude-code-manager://session/{ID}/")), Some(ID.into()), "trailing slash");
        assert_eq!(
            parse_session_link(&format!("Claude-Code-Manager://session/{}", ID.to_uppercase())),
            Some(ID.into()),
            "scheme and uuid are case-insensitive; the id comes back lowercase"
        );
    }

    #[test]
    fn rejects_anything_else() {
        assert_eq!(parse_session_link(&format!("https://session/{ID}")), None, "wrong scheme");
        assert_eq!(parse_session_link(&format!("claude-code-manager://message/{ID}")), None, "not a session link");
        assert_eq!(parse_session_link("claude-code-manager://session/not-a-uuid"), None);
        assert_eq!(parse_session_link("claude-code-manager://session/"), None);
    }

    fn labels(ls: &[&str]) -> Vec<String> {
        ls.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn window_choice_follows_the_priority_order() {
        let windows = labels(&["main", "win-1", "win-2"]);
        let open: HashMap<String, String> = [("win-2".to_string(), "s1".to_string())].into();

        assert_eq!(choose_window("s1", &windows, &open, Some("win-1"), Some("main")), Target::Window("win-2".into()), "showing it");
        assert_eq!(choose_window("s2", &windows, &open, Some("win-1"), Some("main")), Target::Window("win-1".into()), "its terminal");
        assert_eq!(choose_window("s2", &windows, &open, None, Some("win-2")), Target::Window("win-2".into()), "last focused");
        assert_eq!(choose_window("s2", &windows, &open, None, None), Target::Window("main".into()), "any, main first");
        assert_eq!(choose_window("s2", &[], &HashMap::new(), None, None), Target::NewWindow);
    }

    #[test]
    fn window_choice_skips_windows_that_no_longer_exist() {
        let windows = labels(&["win-1"]);
        let open: HashMap<String, String> = [("win-9".to_string(), "s1".to_string())].into();
        assert_eq!(choose_window("s1", &windows, &open, Some("win-8"), Some("win-7")), Target::Window("win-1".into()));
    }

    #[test]
    fn a_repeat_is_the_same_session_within_the_window() {
        let t0 = Instant::now();
        let last = ("s1".to_string(), t0);
        assert!(is_repeat(Some(&last), "s1", t0 + Duration::from_millis(500)));
        assert!(!is_repeat(Some(&last), "s1", t0 + REPEAT_WINDOW), "window elapsed");
        assert!(!is_repeat(Some(&last), "s2", t0 + Duration::from_millis(500)), "another session");
        assert!(!is_repeat(None, "s1", t0));
    }
}
