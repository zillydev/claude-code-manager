# Update Notice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When GitHub has a release newer than the running build, the sidebar and Settings → About say so and give the `brew upgrade` command.

**Architecture:** A backend thread (`crates/app/src/update_check.rs`) asks GitHub's `releases/latest` 10 s after launch and every 6 h of wall-clock time after, keeps the newest newer version in managed `UpdateState`, and emits `update-available` to every window; `update_available` serves windows opened later. The frontend's `useUpdateAvailable` hook feeds a dismissible `UpdateNote` at the foot of the sidebar (dismissal per version, synced across windows by `usePersistentState`) and the same line in Settings → About.

**Tech Stack:** Rust (Tauri 2.11, `ureq` 3 with native-tls, `semver` 1, `serde_json`), React 19 + TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-09-16-update-notice-design.md`

## Global Constraints

- Endpoint: `https://api.github.com/repos/zillydev/claude-code-manager/releases/latest`, with a `User-Agent` header; only `tag_name` is read.
- `ureq = { version = "3", default-features = false, features = ["native-tls"] }`, agent built with `TlsProvider::NativeTls`, `RootCerts::PlatformVerifier` and a 10 s global timeout.
- First check 10 s after launch; wake every 10 min; check when ≥ 6 h wall-clock since the last attempt.
- Failures are silent and keep the last known version.
- Debug builds check only with `CM_UPDATE_CHECK_AS=<version>` set, which replaces the running version; release builds ignore it.
- Event `update-available` (payload: version string without `v`), command `update_available` → `Option<String>`.
- Note copy: `v<version> available — run brew upgrade --cask claude-code-manager` (command in `<code>`, selectable), ✕ dismisses. No upgrade button.
- Dismissal key: `cm:dismissedUpdate` (value: the dismissed version string or `null`).
- Gate: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test`, `npm --prefix crates/app/frontend run test`, `npm --prefix crates/app/frontend run build`.
- Comments 1–2 lines, current state only. Commits authored `zillydev <zaidsharieff09@gmail.com>`, ending with `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`. Never launch the app (`make dev`/`cargo tauri dev`); never push.

---

### Task 1: Version logic (pure, tested)

**Files:**
- Create: `crates/app/src/update_check.rs`
- Modify: `crates/app/src/lib.rs:1-13` (add `mod update_check;`)
- Modify: `crates/app/Cargo.toml` (add `semver = "1"`)

**Interfaces:**
- Produces: `pub fn newer_release(running: &semver::Version, tag: &str) -> Option<semver::Version>`, `pub fn due(last: Option<SystemTime>, now: SystemTime) -> bool`, `pub fn tag_name(body: &str) -> Option<String>`, `pub fn check_as(debug: bool, pretend: Option<&str>, running: &semver::Version) -> Option<semver::Version>`, `const INTERVAL: Duration`.

- [ ] **Step 1: Add the dependency and module with stubs**

`crates/app/Cargo.toml`, after `tauri-plugin-deep-link = "2"`:

```toml
# tauri's own version type (package_info().version); already in the tree
semver = "1"
```

`crates/app/src/lib.rs`: add `mod update_check;` between `mod terminal;` and `mod win_id;` (the list is alphabetical).

`crates/app/src/update_check.rs`:

```rust
//! Whether a newer release is out: one background check for the whole app, reported to every window.

use std::time::{Duration, SystemTime};

use semver::Version;

/// Time between checks, measured on the wall clock so a sleeping Mac still checks when it wakes.
pub const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// The release `tag` names (`v0.1.3` or `0.1.3`), when it is newer than `running`.
pub fn newer_release(running: &Version, tag: &str) -> Option<Version> {
    let _ = (running, tag);
    None
}

/// Whether a check is due: never attempted, `INTERVAL` since the last attempt, or the clock went backwards.
pub fn due(last: Option<SystemTime>, now: SystemTime) -> bool {
    let _ = (last, now);
    false
}

/// `tag_name` from a GitHub release body.
pub fn tag_name(body: &str) -> Option<String> {
    let _ = body;
    None
}

/// The version to check against, or `None` to not check: debug builds only check as `CM_UPDATE_CHECK_AS`.
pub fn check_as(debug: bool, pretend: Option<&str>, running: &Version) -> Option<Version> {
    let _ = (debug, pretend, running);
    None
}
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/app/src/update_check.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn a_newer_tag_is_a_release() {
        assert_eq!(newer_release(&v("0.1.2"), "v0.1.3"), Some(v("0.1.3")));
        assert_eq!(newer_release(&v("0.1.2"), "0.2.0"), Some(v("0.2.0")));
    }

    #[test]
    fn the_same_or_an_older_tag_is_not() {
        assert_eq!(newer_release(&v("0.1.2"), "v0.1.2"), None);
        assert_eq!(newer_release(&v("0.1.2"), "v0.1.1"), None);
    }

    #[test]
    fn a_prerelease_build_is_older_than_its_release() {
        assert_eq!(newer_release(&v("0.2.0-rc1"), "v0.2.0"), Some(v("0.2.0")));
        assert_eq!(newer_release(&v("0.2.0-rc1"), "v0.1.9"), None);
    }

    #[test]
    fn an_unparseable_tag_is_not_a_release() {
        assert_eq!(newer_release(&v("0.1.2"), "latest"), None);
        assert_eq!(newer_release(&v("0.1.2"), ""), None);
    }

    #[test]
    fn a_first_check_is_due() {
        assert!(due(None, SystemTime::now()));
    }

    #[test]
    fn a_check_is_due_after_the_interval() {
        let last = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(!due(Some(last), last + INTERVAL - Duration::from_secs(1)));
        assert!(due(Some(last), last + INTERVAL));
    }

    #[test]
    fn a_clock_that_went_backwards_is_due() {
        let last = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(due(Some(last), last - Duration::from_secs(60)));
    }

    #[test]
    fn reads_the_tag_from_a_release_body() {
        let body = r#"{"url":"x","tag_name":"v0.1.3","name":"v0.1.3","draft":false}"#;
        assert_eq!(tag_name(body).as_deref(), Some("v0.1.3"));
    }

    #[test]
    fn a_body_without_a_tag_has_none() {
        assert_eq!(tag_name(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(tag_name("<html>"), None);
    }

    #[test]
    fn a_release_build_checks_as_itself() {
        assert_eq!(check_as(false, None, &v("0.1.2")), Some(v("0.1.2")));
        assert_eq!(check_as(false, Some("0.0.1"), &v("0.1.2")), Some(v("0.1.2")));
    }

    #[test]
    fn a_debug_build_checks_only_as_the_pretend_version() {
        assert_eq!(check_as(true, None, &v("0.1.2")), None);
        assert_eq!(check_as(true, Some("0.1.0"), &v("0.1.2")), Some(v("0.1.0")));
        assert_eq!(check_as(true, Some("v0.1.0"), &v("0.1.2")), Some(v("0.1.0")));
        assert_eq!(check_as(true, Some("nope"), &v("0.1.2")), None);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p claude-code-manager-app update_check`
Expected: FAIL — `a_newer_tag_is_a_release`, `a_first_check_is_due`, `reads_the_tag_from_a_release_body`, `a_release_build_checks_as_itself` and the others asserting `Some`/`true` fail against the stubs.

- [ ] **Step 4: Implement**

Replace the four stub bodies:

```rust
fn parse_version(s: &str) -> Option<Version> {
    Version::parse(s.strip_prefix('v').unwrap_or(s)).ok()
}

pub fn newer_release(running: &Version, tag: &str) -> Option<Version> {
    parse_version(tag).filter(|v| v > running)
}

pub fn due(last: Option<SystemTime>, now: SystemTime) -> bool {
    match last.map(|t| now.duration_since(t)) {
        None | Some(Err(_)) => true,
        Some(Ok(elapsed)) => elapsed >= INTERVAL,
    }
}

pub fn tag_name(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    serde_json::from_str::<Release>(body).ok().map(|r| r.tag_name)
}

pub fn check_as(debug: bool, pretend: Option<&str>, running: &Version) -> Option<Version> {
    if !debug {
        return Some(running.clone());
    }
    parse_version(pretend?)
}
```

(Keep each `pub fn`'s doc comment from Step 1 above it; `parse_version` goes before `newer_release`.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p claude-code-manager-app update_check`
Expected: 11 passed. (Dead-code warnings for the not-yet-used functions are expected until Task 2.)

- [ ] **Step 6: Commit**

```bash
git add crates/app/Cargo.toml Cargo.lock crates/app/src/lib.rs crates/app/src/update_check.rs
git commit -m "feat(app): decide whether a release is newer than this build"
```

---

### Task 2: Background check, state, event and command

**Files:**
- Modify: `crates/app/src/update_check.rs`
- Modify: `crates/app/src/lib.rs` (manage state, start thread in `setup`, register command)
- Modify: `crates/app/Cargo.toml` (add `ureq`)

**Interfaces:**
- Consumes: Task 1's `newer_release`, `due`, `tag_name`, `check_as`.
- Produces: `pub struct UpdateState` (`Default`), `pub fn start(app: &tauri::AppHandle)`, `#[tauri::command] pub fn update_available(state: tauri::State<UpdateState>) -> Option<String>`, event `update-available` with a `String` payload.

- [ ] **Step 1: Add the dependency**

`crates/app/Cargo.toml`, after the `semver` line:

```toml
# the release check; native-tls uses macOS's Security framework instead of pulling in rustls + ring
ureq = { version = "3", default-features = false, features = ["native-tls"] }
```

- [ ] **Step 2: Write the network test (ignored by default)**

In `update_check.rs`'s `tests` module:

```rust
    #[test]
    #[ignore = "network: run with --ignored to check the real endpoint"]
    fn fetches_the_latest_release_tag() {
        let tag = fetch_latest_tag().expect("GitHub returned a release");
        assert!(newer_release(&v("0.0.0"), &tag).is_some(), "unparseable tag {tag}");
    }
```

Run: `cargo test -p claude-code-manager-app update_check -- --ignored`
Expected: FAIL to compile — `fetch_latest_tag` not found.

- [ ] **Step 3: Implement fetch, state, thread and command**

Update the imports at the top of `update_check.rs`:

```rust
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use semver::Version;
use tauri::{AppHandle, Emitter, Manager};
use ureq::tls::{TlsConfig, TlsProvider};
```

Add after `INTERVAL`:

```rust
const LATEST_URL: &str = "https://api.github.com/repos/zillydev/claude-code-manager/releases/latest";
const FIRST_CHECK: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_secs(10 * 60);

/// The newest release found that is newer than this build.
#[derive(Default)]
pub struct UpdateState(Mutex<Option<String>>);
```

Add after `check_as`:

```rust
/// The latest release's tag, or `None` on any network, HTTP or parse failure.
fn fetch_latest_tag() -> Option<String> {
    // ureq defaults to rustls even when only native-tls is compiled in.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).build())
        .build()
        .into();
    let mut response = agent
        .get(LATEST_URL)
        .header("User-Agent", "claude-code-manager")
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    tag_name(&response.body_mut().read_to_string().ok()?)
}

/// Starts the app-wide check; windows learn of a release through `update-available` or `update_available`.
pub fn start(app: &AppHandle) {
    let pretend = std::env::var("CM_UPDATE_CHECK_AS").ok();
    let Some(running) = check_as(cfg!(debug_assertions), pretend.as_deref(), &app.package_info().version)
    else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        let mut last = None;
        loop {
            let now = SystemTime::now();
            if due(last, now) {
                last = Some(now);
                if let Some(release) = fetch_latest_tag().and_then(|tag| newer_release(&running, &tag)) {
                    record(&app, release.to_string());
                }
            }
            std::thread::sleep(TICK);
        }
    });
}

fn record(app: &AppHandle, version: String) {
    let state = app.state::<UpdateState>();
    let Ok(mut known) = state.0.lock() else { return };
    if known.as_deref() == Some(version.as_str()) {
        return;
    }
    *known = Some(version.clone());
    drop(known);
    let _ = app.emit("update-available", version);
}

/// A release newer than this build, if the check has found one.
#[tauri::command]
pub fn update_available(state: tauri::State<UpdateState>) -> Option<String> {
    state.0.lock().ok().and_then(|known| known.clone())
}
```

- [ ] **Step 4: Wire into `lib.rs`**

In the builder, after `.manage(dev_reload::DevReloadState::default())`:

```rust
        .manage(update_check::UpdateState::default())
```

In `setup`, after `dev_reload::restore_windows(app.handle());`:

```rust
            update_check::start(app.handle());
```

In `generate_handler![...]`, after `session_link::link_listener_ready,`:

```rust
            update_check::update_available,
```

- [ ] **Step 5: Run the network test and the unit tests**

Run: `cargo test -p claude-code-manager-app update_check -- --include-ignored`
Expected: 12 passed (the network test reaches GitHub and parses `v0.1.2` or later).

- [ ] **Step 6: Clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings. Run `cargo fmt -p claude-code-manager-app` only if the repo's other files are rustfmt-clean (check `cargo fmt --check -p claude-code-manager-app` on `main` first; if main isn't clean, format just by hand).

- [ ] **Step 7: Commit**

```bash
git add crates/app/Cargo.toml Cargo.lock crates/app/src/lib.rs crates/app/src/update_check.rs
git commit -m "feat(app): check GitHub for a newer release in the background"
```

---

### Task 3: Note visibility (pure, tested)

**Files:**
- Create: `crates/app/frontend/src/lib/updateNotice.ts`
- Test: `crates/app/frontend/src/lib/updateNotice.test.ts`

**Interfaces:**
- Produces: `export const UPGRADE_COMMAND: string`, `export function noteVersion(latest: string | null, dismissed: string | null): string | null`.

- [ ] **Step 1: Stub**

```ts
/** How a Homebrew install upgrades; the app doesn't upgrade itself. */
export const UPGRADE_COMMAND = "brew upgrade --cask claude-code-manager";

/** The release the sidebar note shows: the newest known, unless that one was dismissed. */
export function noteVersion(latest: string | null, dismissed: string | null): string | null {
  void latest;
  void dismissed;
  return null;
}
```

- [ ] **Step 2: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { noteVersion, UPGRADE_COMMAND } from "./updateNotice";

describe("noteVersion", () => {
  it("shows nothing when no newer release is known", () => {
    expect(noteVersion(null, null)).toBeNull();
    expect(noteVersion(null, "0.1.3")).toBeNull();
  });

  it("shows a release that wasn't dismissed", () => {
    expect(noteVersion("0.1.3", null)).toBe("0.1.3");
  });

  it("hides the release that was dismissed", () => {
    expect(noteVersion("0.1.3", "0.1.3")).toBeNull();
  });

  it("shows a newer release than the dismissed one", () => {
    expect(noteVersion("0.1.4", "0.1.3")).toBe("0.1.4");
  });
});

describe("UPGRADE_COMMAND", () => {
  it("upgrades the Homebrew cask", () => {
    expect(UPGRADE_COMMAND).toBe("brew upgrade --cask claude-code-manager");
  });
});
```

- [ ] **Step 3: Run to verify it fails**

Run: `npm --prefix crates/app/frontend run test -- updateNotice`
Expected: FAIL — "shows a release that wasn't dismissed" and "shows a newer release than the dismissed one" get `null`.

- [ ] **Step 4: Implement**

```ts
export function noteVersion(latest: string | null, dismissed: string | null): string | null {
  return latest !== null && latest !== dismissed ? latest : null;
}
```

- [ ] **Step 5: Run to verify it passes**

Run: `npm --prefix crates/app/frontend run test -- updateNotice`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/app/frontend/src/lib/updateNotice.ts crates/app/frontend/src/lib/updateNotice.test.ts
git commit -m "feat(app): decide when the update note shows"
```

---

### Task 4: Hook, sidebar note and Settings line

**Files:**
- Create: `crates/app/frontend/src/hooks/useUpdateAvailable.ts`
- Create: `crates/app/frontend/src/components/UpdateNote.tsx`
- Modify: `crates/app/frontend/src/components/Sidebar.tsx:854-856` (render `<UpdateNote />` before `</aside>`)
- Modify: `crates/app/frontend/src/components/Settings.tsx:264-270` (About section)

**Interfaces:**
- Consumes: Task 2's `update_available` command and `update-available` event; Task 3's `noteVersion`, `UPGRADE_COMMAND`.
- Produces: `export function useUpdateAvailable(): string | null`; `export default function UpdateNote()`; `export function UpdateLine({ version }: { version: string })`.

- [ ] **Step 1: The hook**

`hooks/useUpdateAvailable.ts`:

```ts
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** A release newer than this build, once the backend's check has found one. */
export function useUpdateAvailable(): string | null {
  const [latest, setLatest] = useState<string | null>(null);
  useEffect(() => {
    const un = listen<string>("update-available", (e) => setLatest(e.payload));
    // Fills in only if no event beat it, so a stale reply can't replace a newer release.
    invoke<string | null>("update_available")
      .then((v) => setLatest((cur) => cur ?? v))
      .catch(() => {});
    return () => {
      un.then((f) => f());
    };
  }, []);
  return latest;
}
```

- [ ] **Step 2: The note component**

`components/UpdateNote.tsx`:

```tsx
import { usePersistentState } from "../hooks/usePersistentState";
import { useUpdateAvailable } from "../hooks/useUpdateAvailable";
import { noteVersion, UPGRADE_COMMAND } from "../lib/updateNotice";

/** "v0.1.3 available — run brew upgrade …", with the command selectable for copying. */
export function UpdateLine({ version }: { version: string }) {
  return (
    <span className="select-text">
      <span className="text-[var(--color-accent)]">v{version}</span> available — run{" "}
      <code className="font-mono text-[var(--color-text)] break-words">{UPGRADE_COMMAND}</code>
    </span>
  );
}

/** Foot of the sidebar: a newer release is out. Dismissing hides that version in every window. */
export default function UpdateNote() {
  const latest = useUpdateAvailable();
  const [dismissed, setDismissed] = usePersistentState<string | null>("cm:dismissedUpdate", null);
  const version = noteVersion(latest, dismissed);
  if (!version) return null;
  return (
    <div className="shrink-0 flex items-start gap-2 px-3 py-2 border-t border-[var(--color-border)] text-xs text-[var(--color-text-dim)]">
      <p className="flex-1 min-w-0">
        <UpdateLine version={version} />
      </p>
      <button
        onClick={() => setDismissed(version)}
        title="Dismiss"
        aria-label="Dismiss update notice"
        className="shrink-0 leading-none px-1 text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
      >
        ✕
      </button>
    </div>
  );
}
```

Before writing, match the ✕ button classes to an existing sidebar dismiss button (`grep -rn "✕" crates/app/frontend/src/components | head`) and use those instead if they differ.

- [ ] **Step 3: Mount in the sidebar**

`Sidebar.tsx`: import `UpdateNote from "./UpdateNote"` with the other component imports, and insert before `</aside>`:

```tsx
      <UpdateNote />
```

- [ ] **Step 4: Settings → About**

`Settings.tsx`: import `useUpdateAvailable` and `UpdateLine`; inside the component near `version` state:

```tsx
  const latest = useUpdateAvailable();
```

Directly after the Version `</Row>` in the About section:

```tsx
          {latest && (
            <p className="text-xs text-[var(--color-text-dim)]">
              <UpdateLine version={latest} />
            </p>
          )}
```

- [ ] **Step 5: Build and test**

Run: `npm --prefix crates/app/frontend run build && npm --prefix crates/app/frontend run test`
Expected: tsc and vite succeed; all tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/app/frontend/src/hooks/useUpdateAvailable.ts crates/app/frontend/src/components/UpdateNote.tsx crates/app/frontend/src/components/Sidebar.tsx crates/app/frontend/src/components/Settings.tsx
git commit -m "feat(app): show a newer release at the foot of the sidebar and in About"
```

---

### Task 5: Docs, TODO and full gate

**Files:**
- Modify: `FEATURES.md` (new entry — a new capability)
- Modify: `CHANGELOG.md` (Unreleased → Added)
- Modify: `TODOS.md` (tick the item only; the user's other uncommitted edits stay uncommitted)

- [ ] **Step 1: FEATURES.md**

Under `## Feedback`, append:

```markdown
- **Update notice** — one app-wide backend check of GitHub's latest release (10 s after launch, then every 6 h of wall-clock time; failures silent) tells every window when a newer version is out: a dismissible line at the foot of the sidebar, "v0.1.3 available — run `brew upgrade --cask claude-code-manager`" (dismissed per version, across windows), repeated in Settings → About. Debug builds check only with `CM_UPDATE_CHECK_AS=<version>`. → `2026-09-16-update-notice-design`
```

- [ ] **Step 2: CHANGELOG.md**

First bullet under `## Unreleased` → `**Added**`:

```markdown
- **A notice when a new version is out.** The app checks GitHub for a newer release now and then, and when there is one, the foot of the sidebar says so, with the command that upgrades it: `brew upgrade --cask claude-code-manager`. Dismiss it with ✕ and it stays hidden until the next release; Settings → About keeps showing it.
```

- [ ] **Step 3: Full gate**

Run each; all must pass with no warnings:

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test
npm --prefix crates/app/frontend run test
npm --prefix crates/app/frontend run build
```

- [ ] **Step 4: Commit docs, ticking TODOS without the user's other edits**

```bash
set -e
S=/private/tmp/claude-501/-Users-zillydev-my-Dev-Personal-claude-manager/26484468-d1b4-445c-808c-4379dbe99541/scratchpad
cp TODOS.md "$S/TODOS.backup.md"
git checkout HEAD -- TODOS.md
sed -i '' 's/^- \[ \] fetch latest version and show upgrade notice/- [x] fetch latest version and show upgrade notice/' TODOS.md
sed -i '' 's/^- \[ \] fetch latest version and show upgrade notice/- [x] fetch latest version and show upgrade notice/' "$S/TODOS.backup.md"
git diff --stat TODOS.md   # exactly 1 line changed
git add FEATURES.md CHANGELOG.md TODOS.md
git commit -m "docs: update notice in FEATURES, CHANGELOG and TODOS"
cp "$S/TODOS.backup.md" TODOS.md
cmp TODOS.md "$S/TODOS.backup.md"
```

- [ ] **Step 5: Hand to the user**

Ask the user to test with `CM_UPDATE_CHECK_AS=0.1.0 make dev` (note ~10 s after launch in every window, including a new one; ✕ hides it in all windows and after relaunch; Settings → About still shows it; plain `make dev` shows nothing). Merge only when they say so.
