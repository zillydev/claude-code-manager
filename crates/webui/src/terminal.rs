use std::collections::HashMap;
use std::io::{Read, Write};

use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

/// Resolve the claude entrypoint. Prefer the user's `claude-tracked` wrapper
/// (matches their shell alias — it exports yabai window env for hooks then
/// `exec claude`), falling back to `claude` on PATH if the wrapper is absent.
fn claude_program() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let wrapper = std::path::Path::new(&home).join(".local/bin/claude-tracked");
        if wrapper.is_file() {
            return wrapper.to_string_lossy().into_owned();
        }
    }
    "claude".to_string()
}

/// The args after the claude entrypoint: bypass-permissions (matching the user's
/// alias) + either start a fresh session (`--session-id`, for a new session) or
/// resume an existing one (`--resume`). Pure.
fn claude_args(session_id: &str, fresh: bool) -> Vec<String> {
    let flag = if fresh { "--session-id" } else { "--resume" };
    vec!["--dangerously-skip-permissions".into(), flag.into(), session_id.into()]
}

/// The full argv for a session's terminal: resolved entrypoint + `claude_args`.
pub fn terminal_argv(session_id: &str, fresh: bool) -> Vec<String> {
    let mut v = vec![claude_program()];
    v.extend(claude_args(session_id, fresh));
    v
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenDecision {
    Spawn,
    Reuse,
    Busy,
}

/// Given the current owner of a session's PTY (if any) and the window asking to
/// open it: spawn if free, reuse if the caller already owns it, else refuse.
pub fn open_decision(existing_owner: Option<&str>, requester: &str) -> OpenDecision {
    match existing_owner {
        None => OpenDecision::Spawn,
        Some(o) if o == requester => OpenDecision::Reuse,
        Some(_) => OpenDecision::Busy,
    }
}

/// A live PTY: master (resize), writer (input), child (kill), and the window
/// label that owns it (one PTY per session, app-wide).
pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    owner: String,
}

#[derive(Default)]
pub struct TerminalState {
    ptys: HashMap<String, PtyHandle>,
}

impl TerminalState {
    pub fn owner(&self, session_id: &str) -> Option<&str> {
        self.ptys.get(session_id).map(|h| h.owner.as_str())
    }
    pub fn insert(&mut self, session_id: &str, handle: PtyHandle) {
        self.ptys.insert(session_id.to_string(), handle);
    }
    pub fn write(&mut self, session_id: &str, data: &[u8]) -> std::io::Result<()> {
        if let Some(h) = self.ptys.get_mut(session_id) {
            h.writer.write_all(data)?;
            h.writer.flush()?;
        }
        Ok(())
    }
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) {
        if let Some(h) = self.ptys.get(session_id) {
            let _ = h.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }
    pub fn close(&mut self, session_id: &str) {
        if let Some(mut h) = self.ptys.remove(session_id) {
            let _ = h.child.kill();
        }
    }
    /// Kill+drop every PTY owned by a window (window-close cleanup).
    pub fn close_window(&mut self, label: &str) {
        let ids: Vec<String> = self
            .ptys
            .iter()
            .filter(|(_, h)| h.owner == label)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(mut h) = self.ptys.remove(&id) {
                let _ = h.child.kill();
            }
        }
    }
    /// Kill every terminal (used as an app-exit backstop).
    pub fn close_all(&mut self) {
        for (_id, mut h) in self.ptys.drain() {
            let _ = h.child.kill();
        }
    }
    fn remove_key(&mut self, session_id: &str) {
        self.ptys.remove(session_id);
    }
}

/// Spawn `claude` under a PTY in `cwd` (`fresh` → `--session-id` a new session,
/// else `--resume`); stream its output to the window as base64 `terminal-output`
/// events; emit `terminal-exit` on EOF.
pub fn spawn_terminal(
    app: AppHandle,
    label: String,
    session_id: String,
    cwd: String,
    rows: u16,
    cols: u16,
    fresh: bool,
) -> std::io::Result<PtyHandle> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let argv = terminal_argv(&session_id, fresh);
    let mut cmd = CommandBuilder::new(&argv[0]);
    for a in &argv[1..] {
        cmd.arg(a);
    }
    cmd.cwd(&cwd);

    let child =
        pair.slave.spawn_command(cmd).map_err(|e| std::io::Error::other(e.to_string()))?;
    drop(pair.slave);

    let mut reader =
        pair.master.try_clone_reader().map_err(|e| std::io::Error::other(e.to_string()))?;
    let writer = pair.master.take_writer().map_err(|e| std::io::Error::other(e.to_string()))?;

    let owner = label.clone();
    let cleanup_id = session_id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    let _ = app.emit_to(
                        label.as_str(),
                        "terminal-output",
                        json!({ "sessionId": session_id, "data": data }),
                    );
                }
            }
        }
        let _ = app.emit_to(
            label.as_str(),
            "terminal-exit",
            json!({ "sessionId": session_id }),
        );
        if let Some(state) = app.try_state::<std::sync::Mutex<TerminalState>>() {
            if let Ok(mut g) = state.lock() {
                g.remove_key(&cleanup_id);
            }
        }
    });

    Ok(PtyHandle { master: pair.master, writer, child, owner })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_args_resume_vs_fresh() {
        assert_eq!(
            claude_args("abc-123", false),
            vec!["--dangerously-skip-permissions", "--resume", "abc-123"]
        );
        assert_eq!(
            claude_args("abc-123", true),
            vec!["--dangerously-skip-permissions", "--session-id", "abc-123"]
        );
    }

    #[test]
    fn terminal_argv_is_program_then_args() {
        let v = terminal_argv("xyz", true);
        assert!(!v[0].is_empty(), "resolved a claude entrypoint");
        assert_eq!(&v[1..], &["--dangerously-skip-permissions", "--session-id", "xyz"]);
    }

    #[test]
    fn open_decision_spawn_reuse_busy() {
        assert_eq!(open_decision(None, "main"), OpenDecision::Spawn);
        assert_eq!(open_decision(Some("main"), "main"), OpenDecision::Reuse);
        assert_eq!(open_decision(Some("main"), "win-1"), OpenDecision::Busy);
    }
}
