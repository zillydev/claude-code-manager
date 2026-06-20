use std::collections::HashMap;
use std::io::{Read, Write};

use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

/// The program + args for a session's terminal. Pure — unit-testable.
pub fn terminal_argv(session_id: &str) -> Vec<String> {
    vec!["claude".into(), "--resume".into(), session_id.into()]
}

fn key(label: &str, session_id: &str) -> String {
    format!("{label}|{session_id}")
}

/// A live PTY: master (resize), writer (input), child (kill).
pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct TerminalState {
    ptys: HashMap<String, PtyHandle>,
}

impl TerminalState {
    pub fn has(&self, label: &str, session_id: &str) -> bool {
        self.ptys.contains_key(&key(label, session_id))
    }
    pub fn insert(&mut self, label: &str, session_id: &str, handle: PtyHandle) {
        self.ptys.insert(key(label, session_id), handle);
    }
    pub fn write(&mut self, label: &str, session_id: &str, data: &[u8]) -> std::io::Result<()> {
        if let Some(h) = self.ptys.get_mut(&key(label, session_id)) {
            h.writer.write_all(data)?;
            h.writer.flush()?;
        }
        Ok(())
    }
    pub fn resize(&self, label: &str, session_id: &str, rows: u16, cols: u16) {
        if let Some(h) = self.ptys.get(&key(label, session_id)) {
            let _ = h.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }
    pub fn close(&mut self, label: &str, session_id: &str) {
        if let Some(mut h) = self.ptys.remove(&key(label, session_id)) {
            let _ = h.child.kill();
        }
    }
    pub fn close_window(&mut self, label: &str) {
        let prefix = format!("{label}|");
        let keys: Vec<String> =
            self.ptys.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        for k in keys {
            if let Some(mut h) = self.ptys.remove(&k) {
                let _ = h.child.kill();
            }
        }
    }
    fn remove_key(&mut self, k: &str) {
        self.ptys.remove(k);
    }
}

/// Spawn `claude --resume <id>` under a PTY in `cwd`; stream its output to the
/// window as base64 `terminal-output` events; emit `terminal-exit` on EOF.
pub fn spawn_terminal(
    app: AppHandle,
    label: String,
    session_id: String,
    cwd: String,
    rows: u16,
    cols: u16,
) -> std::io::Result<PtyHandle> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let argv = terminal_argv(&session_id);
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

    let k = key(&label, &session_id);
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
                g.remove_key(&k);
            }
        }
    });

    Ok(PtyHandle { master: pair.master, writer, child })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_argv_resumes_the_session() {
        assert_eq!(terminal_argv("abc-123"), vec!["claude", "--resume", "abc-123"]);
    }
}
