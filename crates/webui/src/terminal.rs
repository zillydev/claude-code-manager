use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::OnceLock;

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

/// The user's real login PATH. A GUI app launched from Finder/Dock inherits
/// launchd's minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), which omits
/// `/opt/homebrew/bin` etc. — so the spawned `claude` (and the wrapper's
/// `yabai`/`jq`) isn't found, even though the same app launched from a terminal
/// works. Reconstruct PATH from a login shell once and apply it to spawned
/// terminals. `-l` sources the login profile (where Homebrew's `shellenv` lives)
/// but not the interactive rc, avoiding interactive-only hangs. Cached; `None`
/// (leave the inherited PATH alone) if capture fails or isn't a Unix shell.
fn login_path() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
            // A \x01 sentinel brackets the value so any login-profile chatter on
            // stdout is ignored; printf's octal escape emits the raw byte.
            let out = std::process::Command::new(shell)
                .args(["-lc", r#"printf '\001%s\001' "$PATH""#])
                .output()
                .ok()?;
            let s = String::from_utf8_lossy(&out.stdout);
            let path = s.split('\u{1}').nth(1)?.to_string();
            (!path.is_empty()).then_some(path)
        })
        .clone()
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

/// Tear down a PTY child AND its whole process group, then reap it.
///
/// `claude` spawns its tools and MCP servers (bash, `npm exec …`, mcp servers)
/// into ITS OWN process group — portable-pty `setsid`s the child, so the child
/// is the group leader and every descendant shares its pgid. `Child::kill()`
/// signals only the leader, orphaning that tree; the orphans keep the session's
/// lock fd (inherited from claude) open until they each notice stdin EOF and
/// exit a moment later. In that window a `claude --resume <same id>` — e.g. the
/// Restart button right after a close — is refused: "Session ID is already in
/// use." Killing the whole group drops the tree at once; `wait()` reaps the
/// leader so it can't linger as a zombie either.
fn kill_tree(child: &mut Box<dyn Child + Send + Sync>) {
    #[cfg(unix)]
    if let Some(pid) = child.process_id() {
        // Group leader ⇒ pgid == pid, so `kill(-pid)` signals the whole group.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    // Also signal the leader directly (covers non-unix / a missing pid) and reap
    // so no zombie is left behind.
    let _ = child.kill();
    let _ = child.wait();
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
            kill_tree(&mut h.child);
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
                kill_tree(&mut h.child);
            }
        }
    }
    /// Kill every terminal (used as an app-exit backstop).
    pub fn close_all(&mut self) {
        for (_id, mut h) in self.ptys.drain() {
            kill_tree(&mut h.child);
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
    // Repair PATH for Finder/Dock launches (see `login_path`); overrides only
    // PATH, leaving HOME and the rest of the inherited env intact.
    if let Some(path) = login_path() {
        cmd.env("PATH", path);
    }
    // Pin TERM/COLORTERM to the frontend emulator (xterm.js). A Finder/Dock launch
    // inherits neither, so CLIs fall back to little/no color; xterm-256color
    // terminfo is universally present and xterm.js renders 24-bit truecolor.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

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
        // Broadcast to ALL windows that this session's PTY is gone. A window
        // blocked on "open in another window" listens for this and retries its
        // open now that the session is free (the map entry is already removed
        // above, so the retry spawns). Fires for every teardown path — explicit
        // close, window-close, app-exit, and claude self-exit — since all kill
        // the child, ending the read loop here.
        let _ = app.emit("terminal-released", json!({ "sessionId": session_id }));
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

    #[cfg(unix)]
    #[test]
    fn kill_tree_kills_the_whole_process_group() {
        // A leader (`sh`) with a backgrounded child in the SAME group, spawned
        // under a PTY exactly like claude. `Child::kill()` alone would leave the
        // backgrounded `sleep` running (the bug); kill_tree must take the group.
        let pair = native_pty_system()
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.arg("-c");
        cmd.arg("sleep 300 & sleep 300"); // group = {sh, sleep, sleep}
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let pid = child.process_id().expect("a pid") as i32;

        // The group is alive (signal 0 = existence probe on the whole group).
        assert_eq!(unsafe { libc::kill(-pid, 0) }, 0, "group should start alive");

        kill_tree(&mut child);
        std::thread::sleep(std::time::Duration::from_millis(300));

        // The entire group is gone — ESRCH (-1), not just the leader.
        assert_eq!(
            unsafe { libc::kill(-pid, 0) },
            -1,
            "kill_tree must leave no process in the group",
        );
        // Keep the master alive until here so the slave isn't torn down early.
        drop(pair.master);
    }
}
