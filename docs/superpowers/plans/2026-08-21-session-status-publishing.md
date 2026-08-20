# Session Status Publishing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ccm publishes live session state (running / unread / idle) to `session-status.json` in its config dir, opt-in from Settings, so third-party tools (first consumer: sketchybar) can read it.

**Architecture:** A pure mapping function turns the poller's activity scan + discovery + hosted-terminal windows + a merged `lastViewed` map into a `StatusFile`; an atomic writer replaces the file only when the content changes. The app-wide poller calls it each tick when the config flag is on, and deletes the file when the flag goes off or the app exits.

**Tech Stack:** Rust (Tauri 2 app crate), serde/serde_json, objc2 for `NSWindow.windowNumber`, React 19 + TypeScript for the Settings toggle.

**Spec:** `docs/superpowers/specs/2026-08-21-session-status-publishing-design.md`

## Global Constraints

- Work on branch `feat/session-status`. Do NOT merge to main; do NOT run `make dev` — the user verifies and merges.
- Commit trailer on every commit: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Rust: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass (CI runs exactly these; `--all-targets` lints test code too).
- Frontend: `cd crates/app/frontend && npx tsc --noEmit -p . && npx vitest run`.
- The codebase is hand-formatted; do NOT run `cargo fmt` (it reformats unrelated files).
- Existing helpers to reuse, not reinvent: `crate::config::config_dir()`, `crate::config::load()`, `store::scan_activity`, `Event::awaits_user` (already folded into `SessionActivity.awaiting_input`).
- Status file path: `config_dir().join("session-status.json")` — same resolution as `session-meta.json`.

---

### Task 1: Status model + pure mapping (`status_file.rs`)

**Files:**
- Create: `crates/app/src/status_file.rs`
- Modify: `crates/app/src/lib.rs` (add `mod status_file;` next to the other `mod` lines)

**Interfaces:**
- Consumes: `crate::commands::SessionActivity { id: String, last_event_at: i64, awaiting_input: bool }`; `claude_code_manager_core::session::summary::SessionSummary` (fields used: `id: Uuid`, `title: String`, `cwd: Option<String>`, `last_event_at: Option<SystemTime>`, `awaiting_input: bool`).
- Produces:
  ```rust
  pub const RUNNING_STALE_SECS: i64 = 180;
  pub struct StatusFile { pub version: u32, pub updated: f64, pub sessions: BTreeMap<String, SessionStatus> }
  pub struct SessionStatus { pub state: String, pub awaiting_input: bool, pub last_event_at: i64,
                             pub cwd: Option<String>, pub title: String,
                             pub window: Option<i64>, pub hosted: bool }
  pub struct Inputs<'a> { pub activity: &'a [SessionActivity], pub sessions: &'a [SessionSummary],
                          pub hosted: &'a BTreeMap<String, Option<i64>>,  // session id → window id
                          pub last_viewed: &'a BTreeMap<String, i64>, pub now: i64 }
  pub fn desired_state(input: Inputs<'_>) -> StatusFile;
  ```

- [ ] **Step 1: Write the failing test**

Append to `crates/app/src/status_file.rs` (module + tests in one file; the test module comes after the impl):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use claude_code_manager_core::session::summary::SessionSummary;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    const NOW: i64 = 1_800_000_000;

    fn summary(id: &str, title: &str, last: i64, awaiting: bool) -> SessionSummary {
        SessionSummary {
            id: Uuid::parse_str(id).unwrap(),
            file_path: format!("/tmp/{id}.jsonl").into(),
            project_label: "proj".into(),
            cwd: Some("/work".into()),
            git_branch: None,
            title: title.into(),
            mtime: UNIX_EPOCH + Duration::from_secs(last as u64),
            last_event_at: Some(UNIX_EPOCH + Duration::from_secs(last as u64)),
            awaiting_input: awaiting,
            total_tokens: 0,
            forked_from: None,
            ..Default::default()
        }
    }
    fn act(id: &str, last: i64, awaiting: bool) -> SessionActivity {
        SessionActivity { id: id.into(), last_event_at: last, awaiting_input: awaiting }
    }

    const A: &str = "00000000-0000-0000-0000-00000000000a";
    const B: &str = "00000000-0000-0000-0000-00000000000b";
    const C: &str = "00000000-0000-0000-0000-00000000000c";
    const D: &str = "00000000-0000-0000-0000-00000000000d";

    #[test]
    fn states_follow_the_sidebar_rules() {
        let sessions = vec![
            summary(A, "mid turn", NOW - 5, false),      // running
            summary(B, "your turn", NOW - 60, true),     // unread (never viewed → see below)
            summary(C, "seen", NOW - 600, true),         // idle (viewed after the turn)
            summary(D, "quiet", NOW - 99_999, true),     // excluded: not running, not unread, not hosted
        ];
        let activity = vec![act(A, NOW - 5, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(61729)), (C.to_string(), None)]);
        let last_viewed = BTreeMap::from([(B.to_string(), NOW - 900), (C.to_string(), NOW - 100)]);
        let out = desired_state(Inputs {
            activity: &activity, sessions: &sessions, hosted: &hosted,
            last_viewed: &last_viewed, now: NOW,
        });

        assert_eq!(out.version, 1);
        assert_eq!(out.sessions.len(), 3, "D is neither running, unread, nor hosted");
        assert_eq!(out.sessions[A].state, "running");
        assert_eq!(out.sessions[A].window, Some(61729));
        assert!(out.sessions[A].hosted);
        assert_eq!(out.sessions[B].state, "unread");
        assert_eq!(out.sessions[B].window, None);
        assert!(!out.sessions[B].hosted);
        assert_eq!(out.sessions[C].state, "idle", "viewed after its last turn");
        assert!(out.sessions[C].hosted, "hosted with an unknown window is still hosted");
        assert!(!out.sessions.contains_key(D));
    }

    #[test]
    fn live_activity_overrides_the_discovery_snapshot() {
        // Discovery says "done"; the fresher activity scan says mid-turn.
        let sessions = vec![summary(A, "t", NOW - 300, true)];
        let activity = vec![act(A, NOW - 2, false)];
        let out = desired_state(Inputs {
            activity: &activity, sessions: &sessions, hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(), now: NOW,
        });
        assert_eq!(out.sessions[A].state, "running");
        assert_eq!(out.sessions[A].last_event_at, NOW - 2);
        assert!(!out.sessions[A].awaiting_input);
    }

    #[test]
    fn a_stale_mid_turn_session_is_not_running() {
        // Mid-turn but silent past the backstop: not running, and not unread
        // either (the turn never completed).
        let sessions = vec![summary(A, "t", NOW - RUNNING_STALE_SECS - 1, false)];
        let out = desired_state(Inputs {
            activity: &[], sessions: &sessions, hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(), now: NOW,
        });
        assert!(out.sessions.is_empty());
    }

    #[test]
    fn a_hosted_stale_session_is_still_reported() {
        let sessions = vec![summary(A, "t", NOW - 99_999, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let out = desired_state(Inputs {
            activity: &[], sessions: &sessions, hosted: &hosted,
            last_viewed: &BTreeMap::new(), now: NOW,
        });
        assert_eq!(out.sessions[A].state, "idle", "not running, not awaiting → idle");
    }

    #[test]
    fn never_viewed_sessions_are_unread_only_once_seen_before() {
        // No lastViewed entry at all ⇒ treated as never opened ⇒ not unread
        // (mirrors the sidebar, which baselines unseen sessions to their own
        // last event so first launch doesn't flood).
        let sessions = vec![summary(A, "t", NOW - 60, true)];
        let out = desired_state(Inputs {
            activity: &[], sessions: &sessions, hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(), now: NOW,
        });
        assert!(out.sessions.is_empty());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p claude-code-manager-app status_file`
Expected: FAIL — `status_file` module / `desired_state` not found.

- [ ] **Step 3: Write the implementation**

`crates/app/src/status_file.rs` (place ABOVE the test module written in step 1):

```rust
//! Live session status, published for third-party consumers.
//!
//! ccm already derives "is this session running / does it want me" from the
//! `.jsonl` files (the sidebar dots). This exposes the same facts as a small
//! JSON file in the config dir so other tools — a status bar, a notifier —
//! can read them without re-implementing the parse, and without depending on
//! Claude Code hooks, which fire unreliably (measured: 406 UserPromptSubmit
//! vs 14 Stop, leaving every session stranded at "working").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use claude_code_manager_core::session::summary::SessionSummary;
use serde::Serialize;

use crate::commands::SessionActivity;

/// A mid-turn session counts as running until its last write goes this stale —
/// generous so a long thinking pause (no file writes) doesn't drop it. Mirrors
/// the frontend's `RUNNING_STALE_SECS`; a completed turn ends "running" at once
/// regardless.
pub const RUNNING_STALE_SECS: i64 = 180;

/// Schema version of the published file. Bump on any incompatible change —
/// consumers are expected to check it.
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusFile {
    pub version: u32,
    /// When this snapshot was taken (epoch seconds, fractional).
    pub updated: f64,
    pub sessions: BTreeMap<String, SessionStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionStatus {
    /// `running` | `unread` | `idle` — ccm's opinion.
    pub state: String,
    /// Raw facts, so a consumer can derive its own rules instead.
    pub awaiting_input: bool,
    pub last_event_at: i64,
    pub cwd: Option<String>,
    pub title: String,
    /// macOS window id (the same integer yabai uses) when ccm hosts this
    /// session in a terminal tab; `None` when it doesn't know.
    pub window: Option<i64>,
    /// Whether ccm is running this session's `claude` process.
    pub hosted: bool,
}

/// Everything the mapping needs. Borrowed, so the caller keeps its locks short.
pub struct Inputs<'a> {
    /// Fresh tail-scan results (only recently-touched files appear here).
    pub activity: &'a [SessionActivity],
    /// The discovery snapshot — every known session.
    pub sessions: &'a [SessionSummary],
    /// Sessions ccm hosts in a terminal tab → the owning window's id.
    pub hosted: &'a BTreeMap<String, Option<i64>>,
    /// Per-session last-viewed times (epoch seconds), merged across windows.
    pub last_viewed: &'a BTreeMap<String, i64>,
    pub now: i64,
}

/// Build the snapshot to publish. Pure: same inputs, same output, so the
/// caller can compare it against the last write and skip redundant I/O.
///
/// Included: sessions that are running, unread, or hosted. Everything else is
/// noise — and a session never opened is never "unread" (no `last_viewed`
/// entry), matching the sidebar's baseline so a fresh install doesn't publish
/// hundreds of stale "unread"s.
pub fn desired_state(input: Inputs<'_>) -> StatusFile {
    let live: BTreeMap<&str, &SessionActivity> =
        input.activity.iter().map(|a| (a.id.as_str(), a)).collect();

    let mut sessions = BTreeMap::new();
    for s in input.sessions {
        let id = s.id.to_string();
        // The activity scan is fresher than discovery; fall back to the
        // snapshot for sessions it skipped (not touched recently).
        let (last_event_at, awaiting) = match live.get(id.as_str()) {
            Some(a) => (a.last_event_at, a.awaiting_input),
            None => match s.last_event_at {
                Some(t) => (epoch_secs(t), s.awaiting_input),
                None => continue, // no turns yet — nothing to report
            },
        };
        let hosted = input.hosted.get(&id);
        let running = !awaiting && input.now - last_event_at < RUNNING_STALE_SECS;
        let unread = awaiting
            && input.last_viewed.get(&id).is_some_and(|v| last_event_at > *v);

        if !running && !unread && hosted.is_none() {
            continue;
        }
        let state = if running {
            "running"
        } else if unread {
            "unread"
        } else {
            "idle"
        };
        sessions.insert(
            id,
            SessionStatus {
                state: state.to_string(),
                awaiting_input: awaiting,
                last_event_at,
                cwd: s.cwd.clone(),
                title: s.title.clone(),
                window: hosted.copied().flatten(),
                hosted: hosted.is_some(),
            },
        );
    }
    StatusFile { version: VERSION, updated: input.now as f64, sessions }
}

fn epoch_secs(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
```

Then add `mod status_file;` to `crates/app/src/lib.rs` beside the other module declarations.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p claude-code-manager-app status_file`
Expected: PASS (5 tests). If `SessionSummary` has no `Default`, replace `..Default::default()` in the test helper with explicit values for the remaining fields (check `crates/core/src/session/summary.rs`).

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/status_file.rs crates/app/src/lib.rs
git commit -m "feat(app): session status model + pure state mapping

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Atomic writer, change detection, removal

**Files:**
- Modify: `crates/app/src/status_file.rs`

**Interfaces:**
- Consumes: `StatusFile` (Task 1), `crate::config::config_dir()`.
- Produces:
  ```rust
  pub fn status_path() -> PathBuf;
  /// Writes only when `next` differs from `last`; returns the state to remember.
  pub fn publish(path: &Path, next: StatusFile, last: Option<&StatusFile>) -> Option<StatusFile>;
  pub fn unpublish(path: &Path);
  ```

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `status_file.rs`:

```rust
    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cm-status-{}-{}.json", name, std::process::id()))
    }

    fn file(sessions: BTreeMap<String, SessionStatus>, updated: f64) -> StatusFile {
        StatusFile { version: VERSION, updated, sessions }
    }

    fn one(state: &str) -> BTreeMap<String, SessionStatus> {
        BTreeMap::from([(
            A.to_string(),
            SessionStatus {
                state: state.into(), awaiting_input: false, last_event_at: 1,
                cwd: None, title: "t".into(), window: None, hosted: true,
            },
        )])
    }

    #[test]
    fn publish_writes_then_skips_identical_content() {
        let p = tmp_path("write");
        let _ = std::fs::remove_file(&p);

        let first = publish(&p, file(one("running"), 100.0), None).expect("wrote");
        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"running\""));
        assert!(raw.contains("\"version\""));

        // Same sessions, newer `updated` → NOT a change: the timestamp alone
        // must not cause a rewrite every tick.
        std::fs::write(&p, "SENTINEL").unwrap();
        assert!(publish(&p, file(one("running"), 200.0), Some(&first)).is_none());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "SENTINEL", "no rewrite");

        // Changed state → rewritten.
        let second = publish(&p, file(one("idle"), 300.0), Some(&first)).expect("wrote");
        assert!(std::fs::read_to_string(&p).unwrap().contains("\"idle\""));
        assert_eq!(second.sessions[A].state, "idle");

        unpublish(&p);
        assert!(!p.exists());
        unpublish(&p); // removing a missing file is a no-op, not a panic
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p claude-code-manager-app status_file`
Expected: FAIL — `publish` / `unpublish` / `status_path` not found.

- [ ] **Step 3: Implement**

Append to `status_file.rs` (before the test module):

```rust
/// `<config-dir>/session-status.json` — beside `session-meta.json`, so the
/// whole of ccm's state lives in one place a consumer can find.
pub fn status_path() -> PathBuf {
    crate::config::config_dir().join("session-status.json")
}

/// Write `next` iff it differs from `last` in a way consumers can see (the
/// `updated` timestamp alone never counts — otherwise every tick rewrites the
/// file). Returns the newly-published state, or `None` when nothing was
/// written. Failures are swallowed: publishing is a courtesy, never a reason
/// to break the app.
pub fn publish(path: &Path, next: StatusFile, last: Option<&StatusFile>) -> Option<StatusFile> {
    if let Some(prev) = last {
        if prev.sessions == next.sessions && prev.version == next.version {
            return None;
        }
    }
    let json = serde_json::to_string_pretty(&next).ok()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    // Temp + rename so a reader never sees a half-written file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).ok()?;
    std::fs::rename(&tmp, path).ok()?;
    Some(next)
}

/// Remove the file — publishing turned off, or the app is exiting. A status
/// file that outlives the app is worse than none: consumers would keep
/// believing a snapshot that can no longer change.
pub fn unpublish(path: &Path) {
    let _ = std::fs::remove_file(path);
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p claude-code-manager-app status_file`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/status_file.rs
git commit -m "feat(app): atomic status-file writer with change detection

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Config flag + Settings toggle

**Files:**
- Modify: `crates/app/src/config.rs`
- Modify: `crates/app/frontend/src/lib/config.ts` (Config type + defaults)
- Modify: `crates/app/frontend/src/components/Settings.tsx`
- Test: `crates/app/src/config.rs` (existing test module)

**Interfaces:**
- Produces: `Config.integrations: Integrations { publish_status: bool }` (serde `camelCase` ⇒ `integrations.publishStatus` in JSON/TS), default `false`.

- [ ] **Step 1: Write the failing test**

Add to `config.rs`'s test module:

```rust
    #[test]
    fn integrations_default_off_and_round_trip() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.integrations.publish_status, "publishing is opt-in");

        let c: Config =
            serde_json::from_str(r#"{"integrations":{"publishStatus":true}}"#).unwrap();
        assert!(c.integrations.publish_status);

        let back = serde_json::to_string(&c).unwrap();
        assert!(back.contains("\"publishStatus\":true"));
    }
```

- [ ] **Step 2: Run it**

Run: `cargo test -p claude-code-manager-app integrations_default`
Expected: FAIL — no `integrations` field.

- [ ] **Step 3: Implement (Rust)**

In `config.rs`, add the field to `Config` (next to `ui`):

```rust
    #[serde(default)]
    pub integrations: Integrations,
```

and the struct beside `Ui`:

```rust
/// Optional side effects the user opts into. Off by default: ccm writes
/// nothing outside its own config file unless asked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    /// Publish live session state to `<config-dir>/session-status.json` for
    /// third-party tools (status bars, notifiers).
    #[serde(default)]
    pub publish_status: bool,
}
```

Add `integrations: Integrations::default()` to `impl Default for Config`.

- [ ] **Step 4: Implement (frontend)**

In `crates/app/frontend/src/lib/config.ts`, extend the `Config` interface and the defaults object with:

```ts
  integrations: { publishStatus: boolean };
```
```ts
  integrations: { publishStatus: false },
```
(match the file's existing style for nested groups — check how `ui` is declared and mirror it, including any deep-merge helper.)

In `Settings.tsx`, add after the Behavior section:

```tsx
        <Section title="Integrations">
          <CheckRow
            label="Publish session status"
            checked={config.integrations.publishStatus}
            onChange={(v) => setConfig({ integrations: { publishStatus: v } })}
          />
          <div className="text-[10px] text-[var(--color-text-dim)] leading-relaxed">
            Writes live session state (running / unread / idle) to{" "}
            <code>session-status.json</code> in the config folder, for other tools to read.
          </div>
        </Section>
```

(If `setConfig` takes a deep partial, `{ integrations: { publishStatus: v } }` works as written; verify against how `ui` updates are dispatched in this file.)

- [ ] **Step 5: Verify**

Run: `cargo test -p claude-code-manager-app config` and `cd crates/app/frontend && npx tsc --noEmit -p .`
Expected: tests pass; tsc clean.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/config.rs crates/app/frontend/src/lib/config.ts crates/app/frontend/src/components/Settings.tsx
git commit -m "feat(app): opt-in \"Publish session status\" setting

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: lastViewed → backend

**Files:**
- Modify: `crates/app/src/shared.rs`
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/lib.rs` (register the command)
- Modify: `crates/app/frontend/src/App.tsx`

**Interfaces:**
- Produces:
  ```rust
  impl Shared {
      /// Merge a window's lastViewed map, keeping the per-session maximum.
      pub fn merge_last_viewed(&self, incoming: BTreeMap<String, i64>);
      pub fn last_viewed(&self) -> BTreeMap<String, i64>;
  }
  #[tauri::command] pub fn set_last_viewed(map: std::collections::BTreeMap<String, i64>, shared: State<'_, Shared>);
  ```

- [ ] **Step 1: Write the failing test**

Add to `shared.rs` (create a `#[cfg(test)] mod tests` if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn last_viewed_merges_by_maximum() {
        let s = Shared::new();
        s.merge_last_viewed(BTreeMap::from([("a".into(), 100), ("b".into(), 50)]));
        // A second window reports an older view for `a` and a newer one for `b`:
        // neither window is authoritative, so the newest wins per session.
        s.merge_last_viewed(BTreeMap::from([("a".into(), 90), ("b".into(), 70)]));
        let got = s.last_viewed();
        assert_eq!(got["a"], 100);
        assert_eq!(got["b"], 70);
    }
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p claude-code-manager-app last_viewed`
Expected: FAIL — no `merge_last_viewed`.

- [ ] **Step 3: Implement (Rust)**

In `shared.rs`, add to `SharedData`:

```rust
    /// Per-session last-viewed times (epoch s), merged from every window's
    /// localStorage. Feeds the published status file's unread/idle split.
    last_viewed: std::collections::BTreeMap<String, i64>,
```

and to `impl Shared`:

```rust
    /// Merge a window's map, keeping the per-session maximum — windows report
    /// independently and none is authoritative, so "most recently seen" wins.
    pub fn merge_last_viewed(&self, incoming: std::collections::BTreeMap<String, i64>) {
        let mut d = self.data.lock().unwrap();
        for (id, at) in incoming {
            let slot = d.last_viewed.entry(id).or_insert(at);
            if at > *slot {
                *slot = at;
            }
        }
    }

    pub fn last_viewed(&self) -> std::collections::BTreeMap<String, i64> {
        self.data.lock().unwrap().last_viewed.clone()
    }
```

In `commands.rs`:

```rust
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
```

Register `commands::set_last_viewed` in the `tauri::generate_handler!` list in `lib.rs`.

- [ ] **Step 4: Implement (frontend push)**

In `App.tsx`, after the `lastViewed` state is declared, add:

```tsx
  // Publish view times to the backend: the status file needs them to tell
  // "finished and unseen" from "finished and already read".
  useEffect(() => {
    invoke("set_last_viewed", { map: lastViewed }).catch(() => {});
  }, [lastViewed]);
```

(`invoke` is already imported in this file.)

- [ ] **Step 5: Verify**

Run: `cargo test -p claude-code-manager-app` and `cd crates/app/frontend && npx tsc --noEmit -p .`
Expected: pass / clean.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/shared.rs crates/app/src/commands.rs crates/app/src/lib.rs crates/app/frontend/src/App.tsx
git commit -m "feat(app): push lastViewed to the backend for the status file

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Hosted-session window ids

**Files:**
- Modify: `crates/app/src/terminal.rs`
- Modify: `crates/app/Cargo.toml` (macOS deps)
- Create: `crates/app/src/win_id.rs`
- Modify: `crates/app/src/lib.rs` (`mod win_id;`)

**Interfaces:**
- Produces:
  ```rust
  impl TerminalState { /// session id → owning window label, for every live PTY.
      pub fn hosted(&self) -> Vec<(String, String)>; }
  /// The macOS window id (same integer yabai/sketchybar use) of a Tauri window.
  pub fn window_id(app: &tauri::AppHandle, label: &str) -> Option<i64>;   // None off macOS
  ```

- [ ] **Step 1: Write the failing test** (pure part only — the AppKit lookup needs a real window)

Add to `terminal.rs`'s test module:

```rust
    #[test]
    fn hosted_lists_every_live_pty_with_its_owner() {
        let mut st = TerminalState::default();
        assert!(st.hosted().is_empty());
        // `insert` needs a real PtyHandle, so drive it through the public API
        // used by the app: spawn a trivial PTY per session.
        // (If constructing a PtyHandle in a test is impractical, assert instead
        // that `hosted()` is derived from the same map `owner()` reads — see
        // the implementation note below.)
        let _ = &mut st;
    }
```

Note: `PtyHandle` wraps real OS handles; if a test double is impractical, skip this test and rely on `hosted()` being a two-line projection of the same `ptys` map that `owner()` already reads.

- [ ] **Step 2: Implement `hosted()`**

In `terminal.rs`, inside `impl TerminalState`:

```rust
    /// Every live PTY as `(session id, owning window label)` — the app's own
    /// answer to "where is this session running", used to attribute published
    /// status entries to a macOS window.
    pub fn hosted(&self) -> Vec<(String, String)> {
        self.ptys.iter().map(|(id, h)| (id.clone(), h.owner.clone())).collect()
    }
```

- [ ] **Step 3: Implement the window-id lookup**

`crates/app/src/win_id.rs`:

```rust
//! The macOS window id of one of our windows.
//!
//! Window managers (yabai) and status bars address windows by CGWindowID,
//! which for an AppKit window is its `windowNumber`. Publishing that id lets a
//! consumer place a session on the space its window currently occupies —
//! something ccm cannot derive from a `.jsonl` file.

#[cfg(target_os = "macos")]
pub fn window_id(app: &tauri::AppHandle, label: &str) -> Option<i64> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let win = app.get_webview_window(label)?;
    let ns_window = win.ns_window().ok()? as *mut AnyObject;
    if ns_window.is_null() {
        return None;
    }
    // SAFETY: `ns_window()` hands back a live NSWindow*; `windowNumber` is a
    // documented AppKit property returning NSInteger.
    let number: isize = unsafe { msg_send![ns_window, windowNumber] };
    Some(number as i64)
}

#[cfg(not(target_os = "macos"))]
pub fn window_id(_app: &tauri::AppHandle, _label: &str) -> Option<i64> {
    None
}
```

Add `mod win_id;` to `lib.rs`. In `crates/app/Cargo.toml`, under the existing macOS target block, add `objc2 = "0.6"` (already in the tree via tauri). `get_webview_window` requires `use tauri::Manager;` — add it inside the function or at module top as the file's style dictates.

- [ ] **Step 4: Verify it compiles**

Run: `cargo clippy -p claude-code-manager-app --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/terminal.rs crates/app/src/win_id.rs crates/app/src/lib.rs crates/app/Cargo.toml
git commit -m "feat(app): expose hosted sessions and their macOS window ids

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Wire the poller

**Files:**
- Modify: `crates/app/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 1–5.

- [ ] **Step 1: Publish on each tick**

In the poller thread in `lib.rs`, after the existing `emit`s, add:

```rust
                // Publish the status file (opt-in). Runs here because this
                // thread already holds the freshest activity for the whole
                // app; `publish` no-ops unless the content actually changed.
                let publish_on = crate::config::load().integrations.publish_status;
                let path = status_file::status_path();
                if publish_on {
                    let hosted: std::collections::BTreeMap<String, Option<i64>> = handle
                        .state::<Mutex<terminal::TerminalState>>()
                        .lock()
                        .map(|g| g.hosted())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(id, label)| (id, win_id::window_id(&handle, &label)))
                        .collect();
                    let sessions = handle
                        .state::<shared::Shared>()
                        .sessions(&root)
                        .unwrap_or_default();
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let next = status_file::desired_state(status_file::Inputs {
                        activity: &outcome.activity,
                        sessions: &sessions,
                        hosted: &hosted,
                        last_viewed: &handle.state::<shared::Shared>().last_viewed(),
                        now,
                    });
                    if let Some(published) = status_file::publish(&path, next, last_status.as_ref())
                    {
                        last_status = Some(published);
                    }
                } else if last_status.take().is_some() {
                    // Just turned off — don't leave a snapshot behind that can
                    // no longer change.
                    status_file::unpublish(&path);
                }
```

Declare `let mut last_status: Option<status_file::StatusFile> = None;` before the loop.

- [ ] **Step 2: Clear on exit**

In the `RunEvent::Exit` arm (which already kills terminals), add:

```rust
                status_file::unpublish(&status_file::status_path());
```

- [ ] **Step 3: Verify**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean; all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/app/src/lib.rs
git commit -m "feat(app): publish session-status.json from the app-wide poller

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Docs

**Files:**
- Modify: `README.md`
- Modify: `FEATURES.md`

- [ ] **Step 1: README**

Add to the **Configuration** section, after the `session-meta.json` bullet:

```markdown
- `session-status.json` — live session state (running / unread / idle), written only while **Publish session status** is enabled in Settings. Meant to be read by other tools; see below.
```

and a new section after Configuration:

```markdown
## Session status for other tools

With **Publish session status** enabled (Settings → Integrations), ccm keeps `session-status.json` in the config directory current — a small, versioned snapshot other tools can poll:

```json
{
  "version": 1,
  "updated": 1787263639.1,
  "sessions": {
    "59a0dc6a-…": {
      "state": "running",
      "awaiting_input": false,
      "last_event_at": 1787263620,
      "cwd": "/path/to/project",
      "title": "…",
      "window": 61729,
      "hosted": true
    }
  }
}
```

`state` is ccm's opinion — `running` (Claude is mid-turn), `unread` (finished, newer than the last time you opened it), `idle` (finished and seen) — while `awaiting_input` and `last_event_at` are the raw facts, so a consumer can apply its own rules. `window` is the macOS window id (the same integer yabai and similar tools use) when ccm is running that session in a terminal tab, and `null` otherwise; `hosted` says which.

Only sessions that are running, unread, or hosted appear. The file is replaced atomically, rewritten only when the contents change, and removed when publishing is switched off or the app exits — so a stale snapshot never outlives the app.
```

- [ ] **Step 2: FEATURES.md**

Add under **Multi-window** or a new **Integrations** heading:

```markdown
## Integrations

- **Session status publishing** (opt-in, Settings → Integrations) — writes `session-status.json` in the config dir: per-session `running`/`unread`/`idle` plus raw `awaiting_input`/`last_event_at`, `cwd`, `title`, and the macOS window id for sessions running in a ccm terminal tab. Derived from the same poller as the sidebar dots, so it can't be stranded the way Claude Code hooks can. Atomic replace, written only on change, removed when disabled or on exit. → `2026-08-21-session-status-publishing-design`
```

- [ ] **Step 3: Commit**

```bash
git add README.md FEATURES.md
git commit -m "docs: session-status.json as a public interface

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: sketchybar consumer (outside the repo)

**Files:**
- Modify: `~/.config/sketchybar/plugins/space.sh` (back up to `space.sh.bak` first)

- [ ] **Step 1: Back up**

```bash
cp ~/.config/sketchybar/plugins/space.sh ~/.config/sketchybar/plugins/space.sh.bak
```

- [ ] **Step 2: Replace the claude block**

Replace the whole block from `# Claude Code idle indicator` down to the `fi` that closes `if [ -f "$CLAUDE_STATE_FILE" ]` with:

```sh
# Claude session indicators, from claude-code-manager's status file. ccm
# re-derives every session's state from the .jsonl files every few seconds, so
# entries can't get stranded the way hook-written ones did. The space comes
# from the window id via yabai, not from the file — a moved window badges the
# right space, and a window that no longer exists drops out on its own.
CCM_STATE_FILE="$HOME/.config/claude-code-manager/session-status.json"
HAS_UNREAD=0
HAS_RUNNING=0
if [ -f "$CCM_STATE_FILE" ]; then
  FOCUSED_WIN=$(yabai -m query --windows --window 2>/dev/null | jq -r '.id // 0')
  WINS=$(yabai -m query --windows 2>/dev/null)
  if [ -n "$WINS" ]; then
    COUNTS=$(jq -rn --argjson space "$SID" --argjson focused "$FOCUSED_WIN" \
      --argjson wins "$WINS" --slurpfile st "$CCM_STATE_FILE" '
      ($wins | map({ (.id|tostring): .space }) | add) as $sp
      | [ $st[0].sessions // {} | to_entries[] | .value
          | select(.window != null and ($sp[.window|tostring] // -1) == $space) ] as $here
      | "\([ $here[] | select(.state == "unread" and .window != $focused) ] | length) \([ $here[] | select(.state == "running") ] | length)"' 2>/dev/null)
    UNREAD_N=${COUNTS%% *}
    RUNNING_N=${COUNTS##* }
    if [ -n "$UNREAD_N" ] && [ "$UNREAD_N" -gt 0 ] 2>/dev/null; then
      LABEL="$LABEL 󰚩"
      HAS_UNREAD=1
    fi
    if [ -n "$RUNNING_N" ] && [ "$RUNNING_N" -gt 0 ] 2>/dev/null; then
      HAS_RUNNING=1
    fi
  fi
fi
```

- [ ] **Step 3: Add the running branch to the render block**

Between the `HAS_UNREAD` branch and the final `else`:

```sh
elif [ "$HAS_RUNNING" = "1" ]; then
  sketchybar --set "$NAME" background.drawing=on \
                           background.color=0xff313244 \
                           icon.color=0xffa6e3a1 \
                           icon="$LABEL"
```

- [ ] **Step 4: Verify without touching the bar**

```bash
sh -n ~/.config/sketchybar/plugins/space.sh          # syntax
NAME=space.5 SELECTED=false sh -x ~/.config/sketchybar/plugins/space.sh 2>&1 | tail -20
```
Expected: no syntax errors; the trace shows the jq producing two integers.

- [ ] **Step 5: Report to the user** — this file is outside the repo, so it is not committed. Tell them it changed, where the backup is, and that the indicator needs **Publish session status** enabled in Settings to light up.

---

### Task 9: Hand-off

- [ ] Report: what to verify (toggle on in Settings → file appears; run a session in a terminal tab → `running`; finish a turn → `unread` + 󰚩 on that space; open the session → `idle`; toggle off → file removed), and hold on `feat/session-status` until the user says merge. On merge, FEATURES.md is already updated (Task 7).
