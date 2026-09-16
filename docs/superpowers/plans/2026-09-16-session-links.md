# Session Links Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Copy a `claude-code-manager://session/<uuid>` link from a session row; clicking it (from an Obsidian note) opens the installed app on that session.

**Architecture:** `tauri-plugin-deep-link` registers the scheme and delivers opened URLs to Rust. A new `session_link.rs` parses links, picks the window (showing the session → running its terminal → last focused → any → new) and delivers an `open-session-link` event, holding the link as pending until the chosen window's frontend has called `link_listener_ready`. The frontend adds "Copy link" to the session menus and opens received ids through the existing `openSession`.

**Tech Stack:** Rust (Tauri 2.11, `tauri-plugin-deep-link` 2.x, `uuid`), React + TypeScript (vitest).

**Spec:** `docs/superpowers/specs/2026-09-16-session-links-design.md`

## Global Constraints

- Scheme: exactly `claude-code-manager`; link form `claude-code-manager://session/<uuid>`.
- Plugin: `tauri-plugin-deep-link` pinned to major `2` (crates.io's newest is `3.0.0-alpha.0`, for Tauri 3 — never use it). It requires tauri `2.10`+; the app's `Cargo.lock` tauri must stay `2.11.x`.
- macOS only; no single-instance plugin; no deep-link capability permission (the frontend never calls the plugin's JS API).
- Event name `open-session-link` (payload: session id string); command `link_listener_ready`.
- CI gate: `cargo clippy --workspace --all-targets -- -D warnings` must pass, plus `cargo test`, `npm --prefix crates/app/frontend run test`, `npm --prefix crates/app/frontend run build`.
- Comments: 1–2 lines, the non-obvious why, current state only (no "previously"/"now does").
- Markdown prose is never hard-wrapped; commit message bodies are wrapped at 72.
- Commit messages end with `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
- `TODOS.md` carries an uncommitted reorder by the user: never commit it except for this item's checkbox (Task 6 shows how).

---

### Task 1: Link parsing, window choice and repeat detection (pure Rust)

**Files:**
- Create: `crates/app/src/session_link.rs`
- Modify: `crates/app/src/lib.rs` (add `mod session_link;` above `mod session_meta;`)

**Interfaces:**
- Produces: `pub const SCHEME: &str`; `pub fn parse_session_link(url: &str) -> Option<String>`; `pub enum Target { Window(String), NewWindow }`; `pub fn choose_window(session: &str, windows: &[String], open: &HashMap<String, String>, terminal_owner: Option<&str>, last_focused: Option<&str>) -> Target`; `pub const REPEAT_WINDOW: Duration`; `pub fn is_repeat(last: Option<&(String, Instant)>, session: &str, now: Instant) -> bool`.

- [ ] **Step 1: Write the module with stubs and failing tests**

Create `crates/app/src/session_link.rs`:

```rust
//! `claude-code-manager://session/<uuid>` links: which session one names, and which window opens it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const SCHEME: &str = "claude-code-manager";

/// The session a link names (hyphenated, lowercase); `None` for anything but `claude-code-manager://session/<uuid>`.
pub fn parse_session_link(_url: &str) -> Option<String> {
    None
}

#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Window(String),
    NewWindow,
}

/// Where a link to `session` opens: the window showing it, else the one running its terminal, else the last
/// focused, else any (`main` first) — or a new window when none exist.
pub fn choose_window(
    _session: &str,
    _windows: &[String],
    _open: &HashMap<String, String>,
    _terminal_owner: Option<&str>,
    _last_focused: Option<&str>,
) -> Target {
    Target::NewWindow
}

pub const REPEAT_WINDOW: Duration = Duration::from_secs(2);

/// Whether `session` repeats the last handled link within `REPEAT_WINDOW`: macOS can deliver a launch link twice.
pub fn is_repeat(_last: Option<&(String, Instant)>, _session: &str, _now: Instant) -> bool {
    false
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
```

In `crates/app/src/lib.rs`, add `mod session_link;` immediately above `mod session_meta;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p claude-code-manager-app --lib session_link`
Expected: FAIL — `parses_a_session_link`, `window_choice_follows_the_priority_order`, `window_choice_skips_windows_that_no_longer_exist` and `a_repeat_is_the_same_session_within_the_window` fail on assertions (`rejects_anything_else` passes against the stub).

- [ ] **Step 3: Implement the three functions**

Replace the three stub bodies:

```rust
pub fn parse_session_link(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let id = rest.strip_prefix("session/")?.trim_end_matches('/');
    uuid::Uuid::parse_str(id).ok().map(|u| u.to_string())
}
```

```rust
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
```

```rust
pub fn is_repeat(last: Option<&(String, Instant)>, session: &str, now: Instant) -> bool {
    last.is_some_and(|(s, at)| s == session && now.duration_since(*at) < REPEAT_WINDOW)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p claude-code-manager-app --lib session_link`
Expected: PASS, 5 tests. (The module is unused until Task 2; `dead_code` warnings are expected here and cleared there.)

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/session_link.rs crates/app/src/lib.rs
git commit -F - <<'MSG'
feat(app): parse session links and choose the window that opens one

A `claude-code-manager://session/<uuid>` link names a session; this
parses it (case-insensitive scheme and id, trailing slash tolerated)
and picks the window a click should open it in: the one showing it,
else the one running its terminal, else the last focused, else any,
else a new one. `is_repeat` drops the same link twice within 2 s, as
macOS can deliver a launch link both ways.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
MSG
```

---

### Task 2: Register the scheme and deliver links to a window (Rust runtime)

**Files:**
- Modify: `crates/app/Cargo.toml` (dependency, via `cargo add`)
- Modify: `crates/app/tauri.conf.json` (top-level `plugins`)
- Modify: `crates/app/src/session_link.rs` (runtime half)
- Modify: `crates/app/src/lib.rs` (plugin, managed state, setup, window events, command)
- Modify: `crates/app/src/commands.rs` (`open_session`, `stop_watching`)

**Interfaces:**
- Consumes: Task 1's `parse_session_link`, `choose_window`, `Target`, `is_repeat`; `crate::terminal::TerminalState::owner(&self, &str) -> Option<&str>`; `crate::window::spawn_window(&AppHandle)`.
- Produces: `pub struct LinkState` (managed as `Mutex<LinkState>`); `pub fn handle_url(app: &AppHandle, url: &str)`; `pub fn record_open(app: &AppHandle, label: &str, session: &str)`; `pub fn clear_open(app: &AppHandle, label: &str)`; `pub fn focused(app: &AppHandle, label: &str)`; `pub fn forget_window(app: &AppHandle, label: &str)`; command `link_listener_ready`; event `open-session-link` (payload: session id `String`).

- [ ] **Step 1: Add the plugin, pinned to 2.x, and confirm tauri didn't move**

Run: `cargo add tauri-plugin-deep-link@2 -p claude-code-manager-app`
Then run: `grep -A1 '^name = "tauri"$' Cargo.lock && grep -A1 '^name = "tauri-plugin-deep-link"$' Cargo.lock`
Expected: tauri `2.11.x`; tauri-plugin-deep-link `2.x` (not `3.0.0-alpha`).

- [ ] **Step 2: Declare the scheme in `crates/app/tauri.conf.json`**

Add a top-level key after `"bundle": { ... }`:

```json
  "plugins": {
    "deep-link": {
      "desktop": {
        "schemes": ["claude-code-manager"]
      }
    }
  }
```

- [ ] **Step 3: Add the runtime half to `session_link.rs`**

Add to the `use` lines at the top:

```rust
use std::collections::HashSet;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use crate::terminal::TerminalState;
```

Add below `is_repeat` (above `#[cfg(test)]`):

```rust
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
```

- [ ] **Step 4: Wire it into `lib.rs`**

In the builder, directly after `.plugin(tauri_plugin_opener::init())`:

```rust
        .plugin(tauri_plugin_deep_link::init())
        .manage(Mutex::new(session_link::LinkState::default()))
```

At the top of the `.setup(|app| {` closure body, before the dock-name block:

```rust
            // Links arrive through the plugin; one that launched the app is also readable once here.
            let link_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    session_link::handle_url(&link_handle, url.as_str());
                }
            });
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    session_link::handle_url(app.handle(), url.as_str());
                }
            }
```

Add `use tauri_plugin_deep_link::DeepLinkExt;` next to the other `use tauri::` lines.

In `.on_window_event(|win, event| {`, add after the `CloseRequested` block:

```rust
            if let WindowEvent::Focused(true) = event {
                session_link::focused(win.app_handle(), win.label());
            }
```

and inside the existing `if let WindowEvent::Destroyed = event {` block, as its first statement:

```rust
                session_link::forget_window(win.app_handle(), win.label());
```

In `tauri::generate_handler![`, add after `close_guard::confirm_quit,`:

```rust
            session_link::link_listener_ready,
```

- [ ] **Step 5: Record which session each window has open (`commands.rs`)**

In `open_session`, directly after `let label = window.label().to_string();` (the one following the `spawn_blocking` parse):

```rust
    crate::session_link::record_open(&app, &label, &rendered.id);
```

In `stop_watching`, before `Ok(())`:

```rust
    crate::session_link::clear_open(&app, window.label());
```

(`useTranscript.open` calls `stop_watching` before `open_session`, and `close` calls it on its own, so the map follows the open session.)

- [ ] **Step 6: Build, test and run the CI gate**

Run: `cargo build 2>&1 | grep -E "^(warning|error)"` — Expected: no output.
Run: `cargo test 2>&1 | grep -E "test result:"` — Expected: all `ok`, 0 failed.
Run: `cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"` — Expected: `exit=0`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/app/Cargo.toml crates/app/tauri.conf.json crates/app/src/session_link.rs crates/app/src/lib.rs crates/app/src/commands.rs
git commit -F - <<'MSG'
feat(app): register claude-code-manager:// and route links to a window

tauri-plugin-deep-link (2.x) declares the scheme in the bundle's
Info.plist and hands opened URLs to `session_link::handle_url`, with
`get_current` read once in setup for a link that launched the app.

Routing needs to know the windows: `LinkState` records each window's
open session (`open_session` / `stop_watching`), the last focused one
and which frontends listen. A link is held as pending and delivered as
`open-session-link` only to a window whose frontend has called
`link_listener_ready`, since an event emitted before a listener exists
is dropped — the case for a link that launches the app or opens a new
window. The chosen window is unminimised and focused, switching Space.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
MSG
```

---

### Task 3: Build links and resolve received ones (pure TypeScript)

**Files:**
- Create: `crates/app/frontend/src/lib/sessionLink.ts`
- Test: `crates/app/frontend/src/lib/sessionLink.test.ts`

**Interfaces:**
- Produces: `export const SESSION_LINK_SCHEME: string`; `export function sessionLink(sessionId: string): string`; `export type LinkResolution = { kind: "wait" } | { kind: "open"; session: SessionSummary } | { kind: "refresh" } | { kind: "missing" }`; `export function resolveLink(id: string, sessions: SessionSummary[], loading: boolean, refreshed: boolean): LinkResolution`.

- [ ] **Step 1: Write the failing tests**

Create `crates/app/frontend/src/lib/sessionLink.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { resolveLink, sessionLink } from "./sessionLink";
import type { SessionSummary } from "../types";

const ID = "0dd9d545-8a0a-44cc-a85b-23bb6896da97";
const session = (id: string) => ({ id, title: id }) as SessionSummary;

describe("sessionLink", () => {
  it("is the scheme the app registers, pointing at the session", () => {
    expect(sessionLink(ID)).toBe(`claude-code-manager://session/${ID}`);
  });
});

describe("resolveLink", () => {
  it("waits while the session list is loading", () => {
    expect(resolveLink(ID, [], true, false)).toEqual({ kind: "wait" });
  });

  it("opens a session that is in the list", () => {
    const s = session(ID);
    expect(resolveLink(ID, [session("other"), s], false, false)).toEqual({ kind: "open", session: s });
  });

  it("refreshes once before calling a session missing", () => {
    expect(resolveLink(ID, [session("other")], false, false)).toEqual({ kind: "refresh" });
    expect(resolveLink(ID, [session("other")], false, true)).toEqual({ kind: "missing" });
  });
});
```

Create `crates/app/frontend/src/lib/sessionLink.ts` with stubs:

```ts
import type { SessionSummary } from "../types";

export const SESSION_LINK_SCHEME = "claude-code-manager";

export function sessionLink(_sessionId: string): string {
  return "";
}

export type LinkResolution =
  | { kind: "wait" }
  | { kind: "open"; session: SessionSummary }
  | { kind: "refresh" }
  | { kind: "missing" };

export function resolveLink(_id: string, _sessions: SessionSummary[], _loading: boolean, _refreshed: boolean): LinkResolution {
  return { kind: "wait" };
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd crates/app/frontend && npx vitest run src/lib/sessionLink.test.ts`
Expected: FAIL — 3 of 4 fail (the "waits" case passes against the stub).

- [ ] **Step 3: Implement**

Replace the two stub bodies:

```ts
/** A link to a session; the backend registers `SESSION_LINK_SCHEME` and parses the same form. */
export function sessionLink(sessionId: string): string {
  return `${SESSION_LINK_SCHEME}://session/${sessionId}`;
}
```

```ts
/** What to do with a received session id: wait for the list, open it, refresh once for a new file, or give up. */
export function resolveLink(id: string, sessions: SessionSummary[], loading: boolean, refreshed: boolean): LinkResolution {
  if (loading) return { kind: "wait" };
  const session = sessions.find((s) => s.id === id);
  if (session) return { kind: "open", session };
  return refreshed ? { kind: "missing" } : { kind: "refresh" };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd crates/app/frontend && npx vitest run src/lib/sessionLink.test.ts`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/app/frontend/src/lib/sessionLink.ts crates/app/frontend/src/lib/sessionLink.test.ts
git commit -F - <<'MSG'
feat(app): build session links and decide what a received one does

`sessionLink` builds `claude-code-manager://session/<id>`, the form the
backend parses. `resolveLink` decides what a received id does: wait
while the session list loads, open a listed session, refresh once for
a file discovery hasn't seen, then report it missing.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
MSG
```

---

### Task 4: "Copy link" in the session menus

**Files:**
- Modify: `crates/app/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: Task 3's `sessionLink(sessionId: string): string`.

- [ ] **Step 1: Add the copy state and helper**

Add the import next to the other `../lib` imports:

```tsx
import { sessionLink } from "../lib/sessionLink";
```

Next to `const [noteFor, setNoteFor] = useState<SessionSummary | null>(null);` add:

```tsx
  // The row whose link was just copied; its time label confirms, since the menu closes on click.
  const [linkCopiedId, setLinkCopiedId] = useState<string | null>(null);
  const copyLink = (id: string) =>
    navigator.clipboard.writeText(sessionLink(id)).then(
      () => {
        setLinkCopiedId(id);
        setTimeout(() => setLinkCopiedId((c) => (c === id ? null : c)), 1200);
      },
      () => {},
    );
```

- [ ] **Step 2: Add the ⋯ menu item after "Note…"**

Directly after the ⋯ menu's `Note…` button (the `<button ... onClick={(e) => { e.stopPropagation(); close(); setNoteFor(s); }}>Note…</button>`), before `<ConfirmMenuItem label="Delete"`:

```tsx
                        <button
                          role="menuitem"
                          className={menuItemClass}
                          onClick={(e) => {
                            e.stopPropagation();
                            close();
                            copyLink(s.id);
                          }}
                        >
                          Copy link
                        </button>
```

- [ ] **Step 3: Add the right-click item after "Note…"**

Directly after `{item("Note…", () => setNoteFor(s))}`:

```tsx
                {item("Copy link", () => copyLink(s.id))}
```

- [ ] **Step 4: Confirm on the row**

Replace `{relativeTime(sessionUpdatedAt(s, act?.at))}` with:

```tsx
                      {linkCopiedId === s.id ? "link copied ✓" : relativeTime(sessionUpdatedAt(s, act?.at))}
```

- [ ] **Step 5: Typecheck, test, build**

Run: `cd crates/app/frontend && npx tsc --noEmit -p . && npx vitest run && npm run build`
Expected: no tsc output; all tests pass; `✓ built`.

- [ ] **Step 6: Commit**

```bash
git add crates/app/frontend/src/components/Sidebar.tsx
git commit -F - <<'MSG'
feat(app): copy a session's link from its menus

"Copy link" follows "Note…" in a session row's ⋯ menu and right-click
menu and copies the bare `claude-code-manager://session/<id>` link.
Menus close on click, so the row confirms instead: its time reads
"link copied ✓" for 1.2 s, the label swap the copy-id and copy-resume
buttons use.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
MSG
```

---

### Task 5: Open a received link in the window

**Files:**
- Modify: `crates/app/frontend/src/App.tsx`

**Interfaces:**
- Consumes: Task 3's `resolveLink(id, sessions, loading, refreshed): LinkResolution`; Task 2's event `open-session-link` (payload: session id) and command `link_listener_ready`; App's existing `openSession`, `sessions`, `loading`, `refresh` (from `useSessions()`), and `getCurrentWebviewWindow` (already imported).

- [ ] **Step 1: Import**

Next to `import { workingSessions } from "./lib/closeGuard";`:

```tsx
import { resolveLink } from "./lib/sessionLink";
```

- [ ] **Step 2: Listen, announce readiness, resolve**

Place directly after the `openSession` `useCallback` (it must be declared before these effects use it):

```tsx
  // A clicked session link arrives from the backend; it is held until the session list can answer.
  const [pendingLink, setPendingLink] = useState<{ id: string; refreshed: boolean } | null>(null);
  const [linkMissing, setLinkMissing] = useState(false);
  useEffect(() => {
    const unlisten = getCurrentWebviewWindow().listen<string>("open-session-link", (e) => {
      setLinkMissing(false);
      setPendingLink({ id: e.payload, refreshed: false });
    });
    // Only once listening: the backend holds a link until this window can receive it.
    unlisten.then(() => invoke("link_listener_ready"));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
  useEffect(() => {
    if (!pendingLink) return;
    const r = resolveLink(pendingLink.id, sessions, loading, pendingLink.refreshed);
    if (r.kind === "open") {
      setPendingLink(null);
      openSession(r.session);
    } else if (r.kind === "refresh") {
      setPendingLink({ ...pendingLink, refreshed: true });
      refresh();
    } else if (r.kind === "missing") {
      setPendingLink(null);
      setLinkMissing(true);
    }
  }, [pendingLink, sessions, loading, openSession, refresh]);
```

(`refresh` sets `loading` synchronously before its first `await`, so the effect waits out the refresh before judging the id missing.)

- [ ] **Step 3: Show the notice**

Inside `<main className="flex-1 overflow-hidden flex flex-col">`, as its first child:

```tsx
        {linkMissing && (
          <div className="mx-4 mt-2 px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] text-xs flex items-center justify-between gap-3">
            <span className="text-[var(--color-text-dim)]">
              No session with that id — it may have been deleted, or lives on another machine.
            </span>
            <button
              onClick={() => setLinkMissing(false)}
              className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
            >
              dismiss
            </button>
          </div>
        )}
```

- [ ] **Step 4: Typecheck, test, build**

Run: `cd crates/app/frontend && npx tsc --noEmit -p . && npx vitest run && npm run build`
Expected: no tsc output; all tests pass; `✓ built`.

- [ ] **Step 5: Commit**

```bash
git add crates/app/frontend/src/App.tsx
git commit -F - <<'MSG'
feat(app): open a clicked session link in the window it reaches

The window attaches its `open-session-link` listener and only then
calls `link_listener_ready`, so the backend never sends into the gap.
A received id waits for the session list (a launch link beats
discovery), opens through `openSession` like a row click, refreshes
once for a file discovery hasn't seen, and otherwise shows a
dismissible notice. Sidebar filters are left as they are.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
MSG
```

---

### Task 6: Docs, full verification, and the TODOS checkbox

**Files:**
- Modify: `FEATURES.md` (new entry, `## Multi-window` section)
- Modify: `CHANGELOG.md` (`## Unreleased` → `**Added**`)
- Modify: `TODOS.md` (this item's checkbox only)

- [ ] **Step 1: FEATURES.md entry**

Append to the end of the `## Multi-window` section's bullet list:

```markdown
- **Session links** — "Copy link" in a session row's ⋯ / right-click menu copies `claude-code-manager://session/<uuid>`; clicking one (e.g. from an Obsidian note) launches or focuses the app and opens that session — in the window already showing it, else the one running its terminal, else the last focused, else a new one — or says the id wasn't found. `tauri-plugin-deep-link` registers the scheme in the bundle, so links reach the app installed by `make install`, never the dev build. A link that launches the app waits in the backend until a window's frontend reports it is listening. → `2026-09-16-session-links-design`
```

- [ ] **Step 2: CHANGELOG.md entry**

Insert as the first bullet under `## Unreleased` → `**Added**`:

```markdown
- **Session links.** "Copy link" in a session's menu copies a `claude-code-manager://session/…` link; clicking it — say, in an Obsidian note — opens the app on that session, launching it if it isn't running and bringing forward the window that already has it. Links open the installed app, not a development build.
```

- [ ] **Step 3: Full verification**

Run, from the repo root:

```bash
cargo clippy --workspace --all-targets -- -D warnings; echo "clippy exit=$?"
cargo test 2>&1 | grep -E "test result:"
cargo build 2>&1 | grep -cE "^(warning|error)"
npm --prefix crates/app/frontend run test 2>&1 | grep -E "Tests "
npm --prefix crates/app/frontend run build 2>&1 | grep -E "error|built in"
```

Expected: `clippy exit=0`; every `test result: ok`; `0`; all frontend tests pass; `✓ built`.

- [ ] **Step 4: Commit the docs with only this item's checkbox**

`TODOS.md` holds the user's uncommitted reorder. Commit only the checkbox on the committed version, then put the reorder back:

```bash
S=/private/tmp/claude-501/-Users-zillydev-my-Dev-Personal-claude-manager/26484468-d1b4-445c-808c-4379dbe99541/scratchpad
cp TODOS.md "$S/TODOS.links.md"
git checkout HEAD -- TODOS.md
python3 - "$S/TODOS.links.md" <<'PY'
import sys
prefix = '- [ ] copy link to a session'
for p in ['TODOS.md', sys.argv[1]]:
    lines = open(p).read().split('\n')
    hits = [i for i, l in enumerate(lines) if l.startswith(prefix)]
    assert len(hits) == 1, (p, hits)
    lines[hits[0]] = lines[hits[0]].replace('- [ ] ', '- [x] ', 1)
    open(p, 'w').write('\n'.join(lines))
PY
git add FEATURES.md CHANGELOG.md TODOS.md
git commit -F - <<'MSG'
docs: session links

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
MSG
cp "$S/TODOS.links.md" TODOS.md
diff <(git show HEAD:TODOS.md | sort) <(sort TODOS.md) && echo "reorder intact"
```

Expected: `reorder intact`.

- [ ] **Step 5: Manual check (installed app — the user runs this)**

`make install`, quit any running copy, then with the installed app:

1. Copy a link from a session row (row shows "link copied ✓"); click it from an Obsidian note while the app runs → that session opens.
2. Open a session in window A, focus window B, click its link → window A comes forward showing it.
3. A session whose terminal is live in window A, open elsewhere nowhere, click from window B → window A.
4. Quit the app, click a link → the app launches and opens the session.
5. Edit a link's uuid to a random valid one, click → the "No session with that id" notice.
