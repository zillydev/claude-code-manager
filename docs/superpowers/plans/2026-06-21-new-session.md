# New Session From a Directory — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Start a new Claude Code session in a chosen directory (paste a path, or use the selected project) — spawning a fresh `claude --session-id` in a terminal.

**Architecture:** Fix `encode_project_dir` to match CC; add a `new_session` command (validate dir + compute path) and a fresh-vs-resume terminal spawn; the frontend drives the Terminal tab off a `terminalTarget` that carries the new session's cwd so its first spawn is fresh.

**Tech Stack:** Rust (Tauri 2, `portable-pty`), React/TS.

## Global Constraints

- New session spawns `claude … --session-id <uuid>` (fresh) in the chosen cwd; existing sessions keep `--resume`. The fresh signal end-to-end is a non-null cwd.
- `encode_project_dir` must map every non-alphanumeric char to `-` (CC's rule).
- No polling; no native folder picker (paste a path). Manager shows the new session after its first turn via the normal sidebar/open path.
- Tauri converts JS camelCase arg names to Rust snake_case (`newCwd` → `new_cwd`).
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Modify** `crates/core/src/store/paths.rs` — `encode_project_dir` + tests.
- **Modify** `crates/webui/src/terminal.rs` — `claude_args`/`terminal_argv`/`spawn_terminal` take `fresh`; tests.
- **Modify** `crates/webui/src/commands.rs` — `new_session`; `terminal_open` gains `new_cwd`.
- **Modify** `crates/webui/src/lib.rs` — register `new_session`.
- **Frontend modify** `App.tsx` (terminalTarget + newSession handler), `components/TerminalPane.tsx` + `components/TerminalView.tsx` (cwd flow), `components/Sidebar.tsx` (+ New session popover, `onNewSession`).

---

## Task 1: Fix `encode_project_dir` (core)

**Files:**
- Modify: `crates/core/src/store/paths.rs`

- [ ] **Step 1: Write the failing test**

In `paths.rs` `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn encode_project_dir_matches_cc() {
        // CC maps every non-alphanumeric char to '-' (verified empirically).
        assert_eq!(encode_project_dir("/Users/x/my.app"), "-Users-x-my-app");
        assert_eq!(encode_project_dir("/tmp/a_b c"), "-tmp-a-b-c");
        assert_eq!(encode_project_dir("/Users/x/v1.2-rc"), "-Users-x-v1-2-rc");
        assert_eq!(
            encode_project_dir("/Users/zillydev/my/Dev/Personal/claude-manager"),
            "-Users-zillydev-my-Dev-Personal-claude-manager"
        );
    }
```

- [ ] **Step 2: Run it — fails**

Run: `cargo test -p claude-code-manager-core encode_project_dir_matches_cc`
Expected: FAIL on the `my.app` / `a_b c` / `v1.2-rc` cases (current code only replaces `/`).

- [ ] **Step 3: Fix the function**

Replace `encode_project_dir`:

```rust
/// Encode a cwd into Claude Code's project-dir name. CC replaces every
/// non-alphanumeric character with `-` (so `/`, `.`, `_`, space all collapse);
/// letters/digits and case are kept. Lossy (a real `-` is indistinguishable),
/// but matches CC so the resulting dir is the one CC reads/writes.
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}
```

- [ ] **Step 4: Run it — passes**

Run: `cargo test -p claude-code-manager-core paths`
Expected: PASS (the new test + existing `fallback_label`/`label_from_cwd` tests).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/store/paths.rs
git commit -m "fix(core): encode_project_dir maps all non-alphanumerics to '-' (matches CC)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Fresh-vs-resume spawn + `new_session` (webui backend)

**Files:**
- Modify: `crates/webui/src/terminal.rs`, `crates/webui/src/commands.rs`, `crates/webui/src/lib.rs`

**Interfaces:**
- Produces: `terminal::spawn_terminal(app, label, session_id, cwd, rows, cols, fresh: bool)`; command `new_session(cwd) -> { session_id, file_path }`; `terminal_open` extra param `new_cwd: Option<String>`.

- [ ] **Step 1: Parameterize the argv by `fresh` in `terminal.rs`**

Replace `claude_args` and `terminal_argv`:

```rust
fn claude_args(session_id: &str, fresh: bool) -> Vec<String> {
    let flag = if fresh { "--session-id" } else { "--resume" };
    vec!["--dangerously-skip-permissions".into(), flag.into(), session_id.into()]
}

pub fn terminal_argv(session_id: &str, fresh: bool) -> Vec<String> {
    let mut v = vec![claude_program()];
    v.extend(claude_args(session_id, fresh));
    v
}
```

In `spawn_terminal`, add a `fresh: bool` param and use it. Change the signature and the argv line:

```rust
pub fn spawn_terminal(
    app: AppHandle,
    label: String,
    session_id: String,
    cwd: String,
    rows: u16,
    cols: u16,
    fresh: bool,
) -> std::io::Result<PtyHandle> {
    // ...
    let argv = terminal_argv(&session_id, fresh);
    // ... (rest unchanged)
}
```

- [ ] **Step 2: Update the `terminal.rs` tests**

Replace the two existing tests with:

```rust
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
```

- [ ] **Step 3: `new_session` command + `terminal_open` `new_cwd` in `commands.rs`**

Add `new_session` (near the other terminal commands):

```rust
#[derive(Serialize)]
pub struct NewSessionResult {
    pub session_id: String,
    pub file_path: String,
}

/// Allocate a new session id + its file path under a chosen working directory.
/// Does NOT spawn — the terminal owns the spawn (terminal_open with new_cwd), so
/// no output is lost to a reconnect. `claude --session-id` creates the file.
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
```

Change `terminal_open` to take `new_cwd: Option<String>` and branch fresh vs resume:

```rust
#[tauri::command]
pub fn terminal_open(
    session_id: String,
    file_path: String,
    rows: u16,
    cols: u16,
    new_cwd: Option<String>,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    let label = window.label().to_string();
    {
        let state = app.state::<Mutex<TerminalState>>();
        if state.lock().map_err(|e| e.to_string())?.has(&label, &session_id) {
            return Ok(());
        }
    }
    let (cwd, fresh) = match new_cwd {
        Some(c) => (c, true),
        None => {
            let lines = read_raw(std::path::Path::new(&file_path)).map_err(|e| e.to_string())?;
            let cwd = first_cwd(&lines)
                .ok_or("could not determine the session's working directory")?;
            (cwd, false)
        }
    };
    let handle =
        terminal::spawn_terminal(app.clone(), label.clone(), session_id.clone(), cwd, rows, cols, fresh)
            .map_err(|e| format!("failed to start terminal: {e}"))?;
    app.state::<Mutex<TerminalState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .insert(&label, &session_id, handle);
    Ok(())
}
```

- [ ] **Step 4: Register `new_session` in `lib.rs`**

Add to `invoke_handler![...]` (next to `commands::terminal_open`):

```rust
            commands::new_session,
```

- [ ] **Step 5: Build + test**

Run: `cargo test -p claude-code-manager-webui && cargo build -p claude-code-manager-webui`
Expected: terminal tests pass (resume + fresh); webui compiles.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/src/terminal.rs crates/webui/src/commands.rs crates/webui/src/lib.rs
git commit -m "feat(webui): new_session command + fresh (--session-id) terminal spawn

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Frontend — `terminalTarget` + cwd flow

**Files:**
- Modify: `crates/webui/frontend/src/App.tsx`, `components/TerminalPane.tsx`, `components/TerminalView.tsx`

- [ ] **Step 1: `TerminalView` accepts + forwards `cwd`**

In `components/TerminalView.tsx`, add `cwd` to the props type:

```tsx
export default function TerminalView({
  sessionId,
  filePath,
  active,
  cwd,
}: {
  sessionId: string;
  filePath: string;
  active: boolean;
  cwd?: string;
}) {
```

In the activation effect's `terminal_open` invoke, pass `newCwd`:

```tsx
        invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols, newCwd: cwd ?? null }).catch((err) =>
          t.write(`\r\n\x1b[31m[failed to start: ${err}]\x1b[0m\r\n`),
        );
```

(`cwd` present → fresh `--session-id`; absent → resume. Reconnects no-op via the backend `has` guard.)

- [ ] **Step 2: `TerminalPane` carries `cwd` per session**

Rewrite `components/TerminalPane.tsx`:

```tsx
import { useEffect, useState } from "react";
import TerminalView from "./TerminalView";

/** Keeps one mounted xterm per session opened, so each session's live terminal
 *  and scrollback persist across tab/session switches. Shows the active one. */
export default function TerminalPane({
  sessionId,
  filePath,
  cwd,
  tabActive,
}: {
  sessionId: string | null;
  filePath: string | null;
  cwd?: string;
  tabActive: boolean;
}) {
  // sessionId -> { filePath, cwd? } for every session a terminal was opened for.
  const [opened, setOpened] = useState<Record<string, { filePath: string; cwd?: string }>>({});

  useEffect(() => {
    if (sessionId && filePath) {
      setOpened((m) => (m[sessionId] ? m : { ...m, [sessionId]: { filePath, cwd } }));
    }
  }, [sessionId, filePath, cwd]);

  if (!sessionId) {
    return (
      <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
        Open or create a session to start its terminal.
      </div>
    );
  }

  return (
    <div className="h-full w-full bg-[#0a0a0a]">
      {Object.entries(opened).map(([sid, v]) => (
        <TerminalView
          key={sid}
          sessionId={sid}
          filePath={v.filePath}
          cwd={v.cwd}
          active={tabActive && sid === sessionId}
        />
      ))}
    </div>
  );
}
```

- [ ] **Step 3: `App` — `terminalTarget` state + drive the terminal off it**

In `App.tsx`, add the state (near `activeTab`):

```tsx
  const [terminalTarget, setTerminalTarget] = useState<{
    sessionId: string;
    filePath: string;
    cwd?: string;
  } | null>(null);
```

Add an `openSession` helper that opens an existing session in Manager AND points the terminal at it (so switching sessions re-points the terminal):

```tsx
  const openSession = useCallback(
    (s: { id: string; file_path: string; project_label: string }) => {
      open(s.file_path, s.project_label);
      setTerminalTarget({ sessionId: s.id, filePath: s.file_path });
    },
    [open],
  );
```

Replace the three existing `open(s.file_path, s.project_label)` call sites — the Sidebar `onSelect`, the `LineageBanner` `onOpen`, and `Transcript`'s `onOpenSession` — with `openSession(s)`.

Point `TerminalPane` at `terminalTarget` (replace its `sessionId`/`filePath` props at the current render):

```tsx
            <TerminalPane
              sessionId={terminalTarget?.sessionId ?? null}
              filePath={terminalTarget?.filePath ?? null}
              cwd={terminalTarget?.cwd}
              tabActive={activeTab === "terminal"}
            />
```

- [ ] **Step 4: Typecheck**

Run: `cd crates/webui/frontend && npx tsc --noEmit`
Expected: clean (the `newSession` handler + Sidebar wiring land in Task 4, so nothing here references a missing prop yet).

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/App.tsx crates/webui/frontend/src/components/TerminalPane.tsx crates/webui/frontend/src/components/TerminalView.tsx
git commit -m "feat(webui): terminalTarget drives the terminal; carry cwd for fresh spawn

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Frontend — "+ New session" popover (Sidebar)

**Files:**
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `onNewSession(cwd: string): Promise<void>` (Task 3).

- [ ] **Step 1: Add `onNewSession` to Sidebar's Props + destructure**

In the `Props` interface add:

```tsx
  onNewSession: (cwd: string) => Promise<void>;
```

and add `onNewSession,` to the destructured params.

- [ ] **Step 2: Popover state + selected-cwd resolution**

Near the other `useState`s in `Sidebar`, add:

```tsx
  const [newOpen, setNewOpen] = useState(false);
  const [newPath, setNewPath] = useState("");
  const [newErr, setNewErr] = useState<string | null>(null);

  // The concrete cwd implied by the current project filter, if any.
  const selectedCwd = useMemo(() => {
    if (filter.startsWith("dir:")) return filter.slice(4);
    if (filter.startsWith("name:")) {
      const g = projectGroups.find((x) => x.name === filter.slice(5));
      return g && g.cwds.length === 1 ? g.cwds[0] : null;
    }
    return null;
  }, [filter, projectGroups]);

  const createSession = async (cwd: string) => {
    const c = cwd.trim();
    if (!c) return;
    try {
      await onNewSession(c);
      setNewOpen(false);
      setNewPath("");
      setNewErr(null);
    } catch (e) {
      setNewErr(String(e));
    }
  };
```

- [ ] **Step 3: The "+ New session" button + popover JSX**

Wrap the Filters row in a relative container and add the button + popover. Replace the opening of the filters row:

```tsx
        {/* Filters row */}
        <div className="flex gap-1.5 flex-wrap">
```

with:

```tsx
        {/* Filters row */}
        <div className="relative flex gap-1.5 flex-wrap items-center">
          <button
            onClick={() => setNewOpen((o) => !o)}
            className="text-xs px-2 py-1 rounded bg-[var(--color-accent)] text-white"
            title="New session"
          >
            + New
          </button>
          {newOpen && (
            <div className="absolute top-full left-0 mt-1 z-30 w-64 p-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg space-y-2">
              {selectedCwd && (
                <button
                  onClick={() => createSession(selectedCwd)}
                  className="w-full text-left text-xs px-2 py-1 rounded bg-[var(--color-bg)] border border-[var(--color-border)] hover:border-[var(--color-accent)] truncate"
                  title={selectedCwd}
                >
                  New in {shortenPath(selectedCwd)}
                </button>
              )}
              <div className="flex gap-1">
                <input
                  type="text"
                  value={newPath}
                  onChange={(e) => setNewPath(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") createSession(newPath);
                  }}
                  placeholder="paste a directory path…"
                  className="flex-1 bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
                />
                <button
                  onClick={() => createSession(newPath)}
                  disabled={!newPath.trim()}
                  className="text-xs px-2 py-1 rounded bg-[var(--color-accent)] text-white disabled:opacity-40"
                >
                  Create
                </button>
              </div>
              {newErr && <div className="text-[var(--color-error)] text-[10px]">{newErr}</div>}
            </div>
          )}
```

(The two existing `<select>`s — the project selector and the date selector — stay as the next children inside this same row `<div>`; only the row's opening tag and the inserted button/popover change. The row's closing `</div>` is unchanged.)

- [ ] **Step 4: `App` — `newSession` handler + pass it to `<Sidebar>`**

In `App.tsx`, add (after the `openSession` helper from Task 3):

```tsx
  const newSession = useCallback(
    async (cwd: string) => {
      const res = await invoke<{ session_id: string; file_path: string }>("new_session", { cwd });
      setTerminalTarget({ sessionId: res.session_id, filePath: res.file_path, cwd });
      setActiveTab("terminal");
      refresh();
    },
    [refresh],
  );
```

(`invoke` is already imported in `App.tsx`. The handler throws on a bad dir; the Sidebar popover catches it.) Then at the `<Sidebar … />` element add:

```tsx
              onNewSession={newSession}
```

- [ ] **Step 5: Typecheck + tests**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run`
Expected: tsc clean; existing 20 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/components/Sidebar.tsx crates/webui/frontend/src/App.tsx
git commit -m "feat(webui): + New session popover (paste path / use selected project)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Verification

**Files:** none.

- [ ] **Step 1: Full suite + build**

Run: `cargo test && cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npm run build`
Expected: Rust green (incl. `encode_project_dir_matches_cc`, `claude_args_resume_vs_fresh`); tsc clean; 20 frontend tests; vite build succeeds. Report counts.

- [ ] **Step 2: Hand the user manual steps (I won't launch the webui)**

  1. `+ New` → paste a real directory path → **Create** → the **Terminal** tab opens a fresh `claude` running in that dir.
  2. Type a first message → `claude` responds → the session's `.jsonl` is created; after a sidebar refresh it appears under the correct project (verify a dir with a dot/underscore lands in the right place).
  3. Select a project in the unified selector, then `+ New` → **New in <dir>** creates a session there.
  4. A bogus path → **Create** shows the `not a directory` error in the popover; nothing spawned.
  5. Open an existing session → its terminal still resumes (`--resume`) as before.

- [ ] **Step 3: Report**

Report the suite/build results + the manual steps. Do not merge — the user directs merges.

---

## Notes for the implementer

- `terminal_open`'s `has`-guard runs first, so a reconnect to an already-spawned new session no-ops and never re-spawns with `--session-id` (which would conflict once the file exists).
- The Manager transcript for a brand-new session stays empty until its first turn; that's intended. It opens normally from the sidebar once it has content.
- `FEATURES.md`: add a "New session" line under the embedded-terminal / sessions area as the last merge step (substantive feature).
