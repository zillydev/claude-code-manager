//! Live session status, published for third-party consumers.
//!
//! ccm already derives "is this session running / does it want me" from the
//! `.jsonl` files (the sidebar dots). This exposes the same facts as a small
//! JSON file in the config dir so other tools — a status bar, a notifier —
//! can read them without re-implementing the parse, and without depending on
//! Claude Code hooks, which fire unreliably (measured on one machine: 406
//! `UserPromptSubmit` vs 14 `Stop`, leaving every session stranded at
//! "working").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use claude_code_manager_core::session::summary::SessionSummary;
use serde::Serialize;

use crate::commands::SessionActivity;

/// A mid-turn session counts as running until its last write goes this stale —
/// generous so a long thinking pause (which writes nothing) doesn't drop it.
/// Mirrors the frontend's `RUNNING_STALE_SECS`; a completed turn ends "running"
/// at once regardless.
pub const RUNNING_STALE_SECS: i64 = 180;

/// Schema version of the published file. Bump on any incompatible change —
/// consumers are expected to check it.
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusFile {
    pub version: u32,
    /// When this snapshot was taken (epoch seconds).
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

/// Build the snapshot to publish. Pure: same inputs, same output, so the caller
/// can compare it against the last write and skip redundant I/O.
///
/// Included: sessions that are running, unread, or hosted. Everything else is
/// noise — and a session never opened is never "unread" (it has no
/// `last_viewed` entry), matching the sidebar's baseline so a fresh install
/// doesn't publish hundreds of stale "unread"s.
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
        let unread = awaiting && input.last_viewed.get(&id).is_some_and(|v| last_event_at > *v);

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

/// `<config-dir>/session-status.json` — beside `session-meta.json`, so the whole
/// of ccm's state lives in one place a consumer can find.
pub fn status_path() -> PathBuf {
    crate::config::config_dir().join("session-status.json")
}

/// Write `next` iff it differs from `last` in a way consumers can see (the
/// `updated` timestamp alone never counts — otherwise every tick rewrites the
/// file). Returns the newly-published state, or `None` when nothing was written.
/// Failures are swallowed: publishing is a courtesy, never a reason to break the
/// app.
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

/// Remove the file — publishing turned off, or the app is exiting. A status file
/// that outlives the app is worse than none: consumers would keep believing a
/// snapshot that can no longer change.
pub fn unpublish(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};
    use uuid::Uuid;

    const NOW: i64 = 1_800_000_000;
    const A: &str = "00000000-0000-0000-0000-00000000000a";
    const B: &str = "00000000-0000-0000-0000-00000000000b";
    const C: &str = "00000000-0000-0000-0000-00000000000c";
    const D: &str = "00000000-0000-0000-0000-00000000000d";

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
            copied_from: None,
            copied_overlap: None,
            forked_from_message: None,
            copied_from_message: None,
        }
    }
    fn act(id: &str, last: i64, awaiting: bool) -> SessionActivity {
        SessionActivity { id: id.into(), last_event_at: last, awaiting_input: awaiting }
    }

    #[test]
    fn states_follow_the_sidebar_rules() {
        let sessions = vec![
            summary(A, "mid turn", NOW - 5, false),  // running
            summary(B, "your turn", NOW - 60, true), // unread (viewed before the turn)
            summary(C, "seen", NOW - 600, true),     // idle (viewed after the turn)
            summary(D, "quiet", NOW - 99_999, true), // excluded
        ];
        let activity = vec![act(A, NOW - 5, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(61729)), (C.to_string(), None)]);
        let last_viewed = BTreeMap::from([(B.to_string(), NOW - 900), (C.to_string(), NOW - 100)]);
        let out = desired_state(Inputs {
            activity: &activity,
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &last_viewed,
            now: NOW,
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
            activity: &activity,
            sessions: &sessions,
            hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(),
            now: NOW,
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
            activity: &[],
            sessions: &sessions,
            hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(),
            now: NOW,
        });
        assert!(out.sessions.is_empty());
    }

    #[test]
    fn a_hosted_stale_session_is_still_reported() {
        let sessions = vec![summary(A, "t", NOW - 99_999, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let out = desired_state(Inputs {
            activity: &[],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &BTreeMap::new(),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "idle", "not running, not awaiting → idle");
    }

    #[test]
    fn a_session_never_opened_is_not_unread() {
        // No lastViewed entry ⇒ never opened ⇒ not unread, mirroring the
        // sidebar (which baselines unseen sessions so first launch doesn't
        // flood).
        let sessions = vec![summary(A, "t", NOW - 60, true)];
        let out = desired_state(Inputs {
            activity: &[],
            sessions: &sessions,
            hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(),
            now: NOW,
        });
        assert!(out.sessions.is_empty());
    }

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
                state: state.into(),
                awaiting_input: false,
                last_event_at: 1,
                cwd: None,
                title: "t".into(),
                window: None,
                hosted: true,
            },
        )])
    }

    #[test]
    fn publish_writes_then_skips_identical_content() {
        let p = tmp_path("write");
        let _ = std::fs::remove_file(&p);

        let first = publish(&p, file(one("running"), 100.0), None).expect("wrote");
        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"running\"") && raw.contains("\"version\""));

        // Same sessions, newer `updated` → NOT a change: the timestamp alone
        // must not rewrite the file every tick.
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
}
