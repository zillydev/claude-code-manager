use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;

use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

/// Where a session's `claude` will be started from: the program to run and the
/// PATH to run it with.
#[derive(Debug, Clone, PartialEq)]
pub struct Entrypoint {
    pub program: String,
    /// PATH for the child, when it must differ from the inherited one.
    pub path: Option<String>,
}

/// Directories to look in when `claude` is on neither PATH we can ask for —
/// the standard install locations. `~` is expanded against HOME.
const CLAUDE_FALLBACK_DIRS: &[&str] =
    &["~/.local/bin", "~/.claude/local", "/opt/homebrew/bin", "/usr/local/bin"];

/// The first directory of `path` (colon-separated) holding an executable
/// `name`. Pure — the PATH string is passed in, so this is testable without
/// touching the environment.
fn which_in(path: &str, name: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    path.split(':').filter(|d| !d.is_empty()).find_map(|dir| {
        let p = std::path::Path::new(dir).join(name);
        let md = std::fs::metadata(&p).ok()?;
        (md.is_file() && md.permissions().mode() & 0o111 != 0).then_some(p)
    })
}

/// Resolve how to start claude, and with what PATH: the user's `claude-tracked` wrapper when present, else [`find_claude`].
fn resolve_entrypoint() -> Result<Entrypoint, String> {
    // The user's own wrapper, if present, wins — it execs claude after adding
    // window-tracking env (see the claude-tracked script).
    if let Some(home) = std::env::var_os("HOME") {
        let wrapper = std::path::Path::new(&home).join(".local/bin/claude-tracked");
        if wrapper.is_file() {
            return Ok(Entrypoint {
                program: wrapper.to_string_lossy().into_owned(),
                path: login_path(),
            });
        }
    }

    find_claude()
}

/// The real `claude` executable and the PATH it needs, never the `claude-tracked` wrapper.
///
/// A GUI app inherits launchd's minimal PATH, so ccm asks a LOGIN shell for the
/// real one (see [`login_path`]). That misses a common case: `zsh -lc` reads
/// `.zprofile` but NOT `.zshrc`, and `.zshrc` is exactly where Claude Code's
/// native installer, nvm, fnm and asdf put their PATH. A user whose `claude`
/// lives in `~/.local/bin` exported from `.zshrc` therefore had the terminal
/// fail with "no such file", despite `claude` working fine in their terminal.
///
/// So: try the login PATH, then an INTERACTIVE login shell's PATH (which does
/// read `.zshrc`), then the standard install directories. Whichever wins also
/// supplies the child's PATH, so claude's own subprocesses resolve too.
pub(crate) fn find_claude() -> Result<Entrypoint, String> {
    if let Some(path) = login_path() {
        if which_in(&path, "claude").is_some() {
            return Ok(Entrypoint { program: "claude".into(), path: Some(path) });
        }
    }
    if let Some(path) = interactive_path() {
        if which_in(&path, "claude").is_some() {
            return Ok(Entrypoint { program: "claude".into(), path: Some(path) });
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    for dir in CLAUDE_FALLBACK_DIRS {
        let dir = dir.replacen('~', &home, 1);
        if let Some(found) = which_in(&dir, "claude") {
            // Put its directory on PATH too, so anything claude shells out to
            // from the same install resolves.
            let base = login_path().unwrap_or_else(|| "/usr/bin:/bin".into());
            return Ok(Entrypoint {
                program: found.to_string_lossy().into_owned(),
                path: Some(format!("{dir}:{base}")),
            });
        }
    }
    Err(format!(
        "couldn't find the `claude` executable.\r\n\
         Looked on your login-shell PATH, your interactive shell PATH, and in {}.\r\n\
         If claude is installed elsewhere, add its directory to PATH in ~/.zprofile \
         (a GUI app can't see ~/.zshrc), then relaunch the app.",
        CLAUDE_FALLBACK_DIRS.join(", ")
    ))
}

/// The args after the claude entrypoint: bypass-permissions (matching the user's
/// alias) + either start a fresh session (`--session-id`, for a new session) or
/// resume an existing one (`--resume`). Pure.
fn claude_args(session_id: &str, fresh: bool) -> Vec<String> {
    let flag = if fresh { "--session-id" } else { "--resume" };
    vec!["--dangerously-skip-permissions".into(), flag.into(), session_id.into()]
}

/// The full argv for a session's terminal: resolved entrypoint + `claude_args`.
pub fn terminal_argv(session_id: &str, fresh: bool) -> Result<(Vec<String>, Option<String>), String> {
    let entry = resolve_entrypoint()?;
    let mut v = vec![entry.program];
    v.extend(claude_args(session_id, fresh));
    Ok((v, entry.path))
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

/// The PATH an INTERACTIVE login shell sees — the one that includes whatever
/// `.zshrc`/`.bashrc` adds (nvm, fnm, asdf, Claude Code's native installer).
/// Only consulted when the login PATH has no `claude`, because an interactive
/// rc can print prompts or block; a 3s deadline caps that. Cached.
fn interactive_path() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
            let mut child = std::process::Command::new(shell)
                .args(["-ilc", r#"printf '\001%s\001' "$PATH""#])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    // A hung interactive rc must not hang the app.
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return None;
                    }
                }
            }
            let out = child.wait_with_output().ok()?;
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
    /// Epoch seconds claude last produced output that was its own doing (a
    /// `BUSY_MIN_BYTES` burst not caused by user input). Stamped by the reader.
    last_busy: Arc<AtomicI64>,
    /// Epoch seconds of the user's last keystroke or resize on this PTY, so the
    /// reader can tell an echo repaint from claude actually saying something.
    last_input: Arc<AtomicI64>,
    /// Epoch seconds of the user's last Esc / Ctrl-C — the keystroke Claude
    /// Code defines as "stop". Esc before the first token leaves no trace in
    /// the session file and fires no hook; this is the only witness.
    last_interrupt: Arc<AtomicI64>,
}

/// One session ccm is running, for the poller.
pub struct Hosted {
    pub session_id: String,
    pub owner: String,
    /// Epoch seconds of the last substantial burst of claude-authored output.
    pub last_busy_at: i64,
    /// Epoch seconds of the user's last interrupt keystroke (0 if none).
    pub last_interrupt_at: i64,
}

/// How long after claude's last burst of output we still call the session busy.
/// Working output is bursty — measured at 1–2.6 kB/s with 2–4s of silence
/// between bursts inside a single turn — so this has to ride out those gaps
/// while still dropping the session soon after it really stops.
pub const BUSY_QUIET_SECS: i64 = 10;

/// Bytes within a second that mean "claude is working", not "the TUI twitched".
///
/// A parked TUI is not silent: it repaints its footer now and then, measured at
/// 42 and 79 bytes, twice in 75 seconds of sitting at the prompt. Treating any
/// output as work therefore left a session parked on a question reading as busy
/// forever. Working output is an order of magnitude bigger — a spinner with a
/// ticking elapsed counter, plus streamed text — so volume, not mere presence,
/// is what separates them. 256 leaves ~3x headroom over the loudest idle
/// repaint and ~4x under the quietest working second.
pub const BUSY_MIN_BYTES: usize = 256;

/// Output within this window after a keystroke or resize is the TUI answering
/// the user (echo, a question card repaint), not claude working.
pub const INPUT_ECHO_SECS: i64 = 2;

/// Rolling per-second byte count behind the busy stamp: a burst split across
/// several reads still counts as one, since reads arrive in chunks of whatever
/// the kernel had, not in tidy seconds.
struct BusyWindow {
    second: i64,
    bytes: usize,
}

impl BusyWindow {
    fn new(now: i64) -> Self {
        BusyWindow { second: now, bytes: 0 }
    }

    /// Feed one read. True once this second's output looks like claude working:
    /// loud enough to be more than a footer twitch, and not an echo of input.
    fn feed(&mut self, now: i64, n: usize, last_input: i64) -> bool {
        if now.saturating_sub(last_input) < INPUT_ECHO_SECS {
            return false; // the user caused this repaint, whatever its size
        }
        if now != self.second {
            *self = BusyWindow::new(now);
        }
        self.bytes += n;
        self.bytes >= BUSY_MIN_BYTES
    }
}

/// A bare Esc or Ctrl-C. xterm.js delivers each key event as one write, so
/// an arrow key (`ESC [ A`) or Alt-chord (`ESC x`) never matches.
pub fn is_interrupt_keystroke(data: &[u8]) -> bool {
    data == b"\x1b" || data == b"\x03"
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
    /// Every live PTY as `(session id, owning window label)` — the app's own
    /// answer to "where is this session running", used to attribute published
    /// status entries to a macOS window.
    pub fn hosted(&self) -> Vec<Hosted> {
        self.ptys
            .iter()
            .map(|(id, h)| Hosted {
                session_id: id.clone(),
                owner: h.owner.clone(),
                last_busy_at: h.last_busy.load(Ordering::Relaxed),
                last_interrupt_at: h.last_interrupt.load(Ordering::Relaxed),
            })
            .collect()
    }
    pub fn insert(&mut self, session_id: &str, handle: PtyHandle) {
        self.ptys.insert(session_id.to_string(), handle);
    }
    pub fn write(&mut self, session_id: &str, data: &[u8]) -> std::io::Result<()> {
        if let Some(h) = self.ptys.get_mut(session_id) {
            h.last_input.store(now_secs(), Ordering::Relaxed);
            if is_interrupt_keystroke(data) {
                h.last_interrupt.store(now_secs(), Ordering::Relaxed);
            }
            h.writer.write_all(data)?;
            h.writer.flush()?;
        }
        Ok(())
    }
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) {
        if let Some(h) = self.ptys.get(session_id) {
            // A resize triggers a full repaint; that burst is the user's doing.
            h.last_input.store(now_secs(), Ordering::Relaxed);
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

    let (argv, path) = terminal_argv(&session_id, fresh).map_err(std::io::Error::other)?;
    let mut cmd = CommandBuilder::new(&argv[0]);
    for a in &argv[1..] {
        cmd.arg(a);
    }
    cmd.cwd(&cwd);
    // Repair PATH for Finder/Dock launches (see `resolve_entrypoint`); overrides
    // only PATH, leaving HOME and the rest of the inherited env intact.
    if let Some(path) = path {
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
    // The TUI repaints both when claude talks and when the user pokes it, so
    // work = a loud burst NOT attributable to recent input (see BusyWindow).
    let last_busy = Arc::new(AtomicI64::new(0));
    // Spawn counts as input: the boot paint answers the user opening the tab.
    let last_input = Arc::new(AtomicI64::new(now_secs()));
    let last_interrupt = Arc::new(AtomicI64::new(0));
    let stamp = Arc::clone(&last_busy);
    let input_at = Arc::clone(&last_input);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut window = BusyWindow::new(now_secs());
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let now = now_secs();
                    if window.feed(now, n, input_at.load(Ordering::Relaxed)) {
                        stamp.store(now, Ordering::Relaxed);
                    }
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

    Ok(PtyHandle { master: pair.master, writer, child, owner, last_busy, last_input, last_interrupt })
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
        // Skips where claude isn't installed at all (CI) — resolution is
        // covered by `which_in_*` below, which needs no real claude.
        if let Ok((v, _path)) = terminal_argv("xyz", true) {
            assert!(!v[0].is_empty(), "resolved a claude entrypoint");
            assert_eq!(&v[1..], &["--dangerously-skip-permissions", "--session-id", "xyz"]);
        }
    }

    #[test]
    fn which_in_finds_only_executables_in_order() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("cm-which-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        // A non-executable file in the FIRST dir must be ignored, so the
        // executable in the second wins — a mode bit, not just a name match.
        std::fs::write(a.join("claude"), "not executable").unwrap();
        let real = b.join("claude");
        std::fs::write(&real, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();

        let path = format!("{}:{}", a.display(), b.display());
        assert_eq!(which_in(&path, "claude"), Some(real.clone()));
        assert_eq!(which_in(&path, "nope"), None);
        // Empty segments (a trailing colon) must not resolve to the cwd.
        assert_eq!(which_in(":", "claude"), None);

        std::fs::set_permissions(a.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(which_in(&path, "claude"), Some(a.join("claude")), "first match wins");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_claude_reports_where_it_looked() {
        // The message has to be actionable in the terminal pane: a bare
        // "No such file or directory (os error 2)" told a user nothing.
        let msg = super::resolve_entrypoint()
            .err()
            .unwrap_or_else(|| "couldn't find the `claude` executable. Looked ... in ~/.local/bin".into());
        assert!(msg.contains("claude"));
        assert!(msg.contains("~/.local/bin"), "names the directories it searched");
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

    /// Per-second byte counts measured off a real `claude` TUI under a PTY.
    /// A turn is bursty — kilobyte repaints separated by seconds of silence —
    /// and once it ends the TUI only repaints its footer, twice in 75 seconds.
    const WORKING_TRACE: &[usize] = &[81, 2618, 0, 0, 1374, 264, 120, 1950, 0, 0, 1024, 1457];
    const IDLE_TRACE: &[usize] = &[0, 0, 0, 42, 0, 79, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    /// Seconds at which a trace would stamp the session busy. `inputs` are the
    /// seconds the user typed (or resized) — output near them is echo.
    fn busy_seconds(trace: &[usize], from: i64, inputs: &[i64]) -> Vec<i64> {
        let mut w = BusyWindow::new(from);
        (0..trace.len() as i64)
            .filter(|i| {
                let now = from + i;
                let last_input = inputs.iter().copied().filter(|t| *t <= now).max().unwrap_or(i64::MIN);
                trace[*i as usize] > 0 && w.feed(now, trace[*i as usize], last_input)
            })
            .map(|i| from + i)
            .collect()
    }

    #[test]
    fn an_idle_tui_never_looks_busy() {
        assert!(busy_seconds(IDLE_TRACE, 0, &[]).is_empty(), "a footer repaint read as work");
        // Worst case: a whole idle stretch arriving inside one second.
        let total: usize = IDLE_TRACE.iter().sum();
        assert!(!BusyWindow::new(0).feed(0, total, i64::MIN), "{total}B of idle repaints read as work");
    }

    #[test]
    fn a_working_tui_stays_busy_across_its_silent_gaps() {
        let busy = busy_seconds(WORKING_TRACE, 0, &[]);
        assert!(!busy.is_empty(), "a whole turn went unnoticed");
        // Not every working second crosses the bar (that 120B repaint doesn't);
        // what matters is that no gap between stamps outlasts the hold, or the
        // indicator would blink out mid-turn.
        let longest = busy.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
        assert!(longest < BUSY_QUIET_SECS, "{longest}s between stamps drops the dot mid-turn");
        // And the turn's end is what clears it, within the hold.
        assert_eq!(*busy.last().unwrap(), 11);
    }

    #[test]
    fn a_burst_split_across_reads_still_counts() {
        let mut w = BusyWindow::new(7);
        // 1kB arriving as four reads inside one second: reads are chunks, not seconds.
        let quiet = i64::MIN;
        assert!(w.feed(7, 64, quiet) || w.feed(7, 64, quiet) || w.feed(7, 64, quiet) || w.feed(7, 64, quiet));
        // …and the count does not leak into the next second.
        assert!(!w.feed(8, 79, quiet));
    }

    #[test]
    fn toggling_a_parked_question_never_looks_like_work() {
        // Arrow keys through an AskUserQuestion card: every kilobyte repaint
        // lands the same second as the keystroke that caused it. However long
        // the user browses the options, claude is still just waiting.
        let inputs: Vec<i64> = (0..30).step_by(2).collect();
        let mut trace = [0usize; 30];
        for t in inputs.iter() {
            trace[*t as usize] = 1500;
        }
        assert!(busy_seconds(&trace, 0, &inputs).is_empty(), "an echo repaint read as work");
    }

    #[test]
    fn answering_a_question_reads_as_work_once_the_echo_window_passes() {
        // Enter at t=0, claude streams from t=0 on. The first INPUT_ECHO_SECS
        // are indistinguishable from echo; everything after must count.
        let trace = [1500usize; 10];
        let busy = busy_seconds(&trace, 0, &[0]);
        assert_eq!(busy.first().copied(), Some(INPUT_ECHO_SECS), "work never attributed to claude");
        // The gap until then stays within the hold, so the dot never drops.
        const { assert!(INPUT_ECHO_SECS < BUSY_QUIET_SECS) }
    }

    #[test]
    fn only_a_bare_esc_or_ctrl_c_counts_as_an_interrupt() {
        assert!(is_interrupt_keystroke(b"\x1b"));
        assert!(is_interrupt_keystroke(b"\x03"));
        // xterm.js delivers one key event per write, so these are never "Esc".
        assert!(!is_interrupt_keystroke(b"\x1b[A"), "arrow key");
        assert!(!is_interrupt_keystroke(b"\x1b\r"), "alt-enter");
        assert!(!is_interrupt_keystroke(b"\x1b[200~x\x1b[201~"), "bracketed paste");
        assert!(!is_interrupt_keystroke(b"y"));
    }

    #[test]
    fn typing_while_claude_works_keeps_the_dot_lit() {
        // Queueing a message mid-turn: keystrokes pepper the stream, but the
        // spinner keeps painting in the pauses, and pauses longer than the echo
        // window re-stamp before the hold runs out.
        let trace = [900usize; 20];
        let inputs = [0, 1, 2, 6, 7, 12, 13];
        let busy = busy_seconds(&trace, 0, &inputs);
        let mut last = 0;
        for t in busy {
            assert!(t - last < BUSY_QUIET_SECS, "{}s unstamped drops the dot mid-turn", t - last);
            last = t;
        }
        assert!(20 - last < BUSY_QUIET_SECS);
    }
}
