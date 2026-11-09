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
///
/// It is only a guess at "the process died mid-turn": a long tool call (a
/// build, a CI poll) writes nothing for minutes and is still running. Hosted
/// sessions skip it entirely — ccm holds their PTY, so liveness is known.
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
        let (mut last_event_at, mut awaiting, busy, busy_at, interrupt_at) =
            match live.get(id.as_str()) {
                Some(a) => (a.last_event_at, a.awaiting_input, a.busy, a.busy_at, a.interrupt_at),
                None => match s.last_event_at {
                    Some(t) => (epoch_secs(t), s.awaiting_input, false, 0, 0),
                    None => continue, // no turns yet — nothing to report
                },
            };
        let hosted = input.hosted.get(&id);
        // An Esc / Ctrl-C in ccm's own terminal ends the turn as surely as the
        // file's interrupt marker — and Esc before the first token writes no
        // marker and fires no hook, so the keystroke is the only witness.
        if interrupt_at > last_event_at && !awaiting {
            last_event_at = interrupt_at;
            awaiting = true;
        }
        // Claude's own account (the opt-in hook relay) outranks inference
        // unless the file refutes it: a running phase is only contradicted by
        // a finished turn or an interrupt (both `awaiting`), never by the tool
        // lines a turn writes — those handed mid-turn verdicts back to the
        // quiet-terminal guess, which read a long silence as "parked".
        let hook = live
            .get(id.as_str())
            .and_then(|a| a.hook.as_ref())
            .filter(|h| h.since >= last_event_at || (h.phase == "running" && !awaiting));
        // Claude's own report from ccm's terminal states what everything below
        // infers; `idle` leaves the file's verdict on its last turn standing.
        let program = live
            .get(id.as_str())
            .and_then(|a| a.program.as_ref())
            .filter(|_| hosted.is_some());
        let (running, waiting_since) = match program {
            Some(p) => (
                p.state == "working",
                matches!(p.state.as_str(), "blocked" | "done" | "error").then_some(p.since),
            ),
            None => {
                // Without hooks, a hosted session's terminal is the authority: claude's
                // TUI talks while working and falls silent when it waits. The
                // file-mtime backstop stays for sessions ccm cannot see into at all.
                let running = match hook {
                    // Unhosted: no PTY to prove liveness, so decay like the mtime guess —
                    // from the last sign of life, hook or file line.
                    Some(h) => {
                        h.phase == "running"
                            && (hosted.is_some()
                                || input.now - h.since.max(last_event_at) < RUNNING_STALE_SECS)
                    }
                    None => {
                        !awaiting
                            && if hosted.is_some() {
                                busy
                            } else {
                                input.now - last_event_at < RUNNING_STALE_SECS
                            }
                    }
                };
                // Something is waiting on the user that the file cannot show — a
                // question, a permission prompt, a plan. Hooks date it exactly; the
                // quiet-terminal inference dates it by when the talking stopped. Either
                // way it reads as unread from that moment, because the file's last
                // event is your own prompt, which you have by definition already seen.
                let waiting_since = match hook {
                    Some(h) if h.phase != "running" => Some(h.since),
                    Some(_) => None,
                    None => (hosted.is_some() && !busy && !awaiting).then_some(busy_at),
                };
                (running, waiting_since)
            }
        };
        let attention_at = waiting_since.unwrap_or(last_event_at);
        let unread = !running
            && (awaiting || waiting_since.is_some())
            && input.last_viewed.get(&id).is_some_and(|v| attention_at > *v);

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

/// What the published file is called. A dev build writes its own, so running
/// `make dev` beside the installed app cannot disturb whatever reads the real
/// one: both poll the same config dir, and a single file with two writers means
/// each overwrites the other's opinion every few seconds.
///
/// Keyed on the build profile rather than `tauri::is_dev()` — that is only
/// `!cfg!(feature = "custom-protocol")`, a feature this crate never declares,
/// so it would read as "dev" inside the installed app too.
pub fn status_file_name(dev_build: bool) -> &'static str {
    if dev_build {
        "session-status.dev.json"
    } else {
        "session-status.json"
    }
}

/// `<config-dir>/session-status.json` — beside `session-meta.json`, so the whole
/// of ccm's state lives in one place a consumer can find.
pub fn status_path() -> PathBuf {
    crate::config::config_dir().join(status_file_name(cfg!(debug_assertions)))
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
    use crate::program_status::ProgramStatus;

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
        SessionActivity {
            id: id.into(),
            last_event_at: last,
            awaiting_input: awaiting,
            hosted: false,
            owner: None,
            busy: false,
            busy_at: 0,
            hook: None,
            interrupt_at: 0,
            program: None,
        }
    }
    /// Hosted with a talking terminal — claude is actually working.
    fn busy_act(id: &str, last: i64) -> SessionActivity {
        SessionActivity {
            id: id.into(),
            last_event_at: last,
            awaiting_input: false,
            hosted: true,
            owner: None,
            busy: true,
            busy_at: last,
            hook: None,
            interrupt_at: 0,
            program: None,
        }
    }
    /// Hosted but silent since `quiet_at` — parked on a question, or just sitting.
    fn quiet_act(id: &str, last: i64, quiet_at: i64) -> SessionActivity {
        SessionActivity {
            id: id.into(),
            last_event_at: last,
            awaiting_input: false,
            hosted: true,
            owner: None,
            busy: false,
            busy_at: quiet_at,
            hook: None,
            interrupt_at: 0,
            program: None,
        }
    }

    #[test]
    fn an_esc_before_the_first_token_ends_the_turn() {
        // Prompt written, Esc 0.4s later: the file gets no reply and no
        // interrupt marker, hooks record only UserPromptSubmit, and the PTY
        // stays open. Every source but the keystroke says "running".
        let sessions = vec![summary(A, "escaped", NOW - 60, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let mut a = hooked(quiet_act(A, NOW - 60, NOW - 59), "running", NOW - 60);
        a.interrupt_at = NOW - 59;
        let state = |a: SessionActivity, viewed: i64| {
            desired_state(Inputs {
                activity: &[a],
                sessions: &sessions,
                hosted: &hosted,
                last_viewed: &BTreeMap::from([(A.to_string(), viewed)]),
                now: NOW,
            })
            .sessions[A]
                .state
                .clone()
        };
        assert_eq!(state(a.clone(), NOW - 100), "unread", "your turn, dated at the Esc");
        assert_eq!(state(a.clone(), NOW - 59), "idle", "…and seen, since you pressed it");

        // An Esc that cleared typed text at an idle prompt changes nothing.
        let mut idle = quiet_act(A, NOW - 60, NOW - 59);
        idle.awaiting_input = true;
        idle.interrupt_at = NOW - 5;
        assert_eq!(state(idle, NOW - 59), "idle", "already your turn; the Esc is not news");

        // A file line newer than the Esc (a fresh prompt, claude working on it)
        // outranks it: the keystroke is history.
        let mut later = busy_act(A, NOW - 30);
        later.interrupt_at = NOW - 59;
        assert_eq!(state(later, NOW - 100), "running", "the Esc is older than the file — inference decides");
    }

    #[test]
    fn claudes_own_report_outranks_the_terminal_guess() {
        let sessions = vec![summary(A, "reported", NOW - 60, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let state = |a: SessionActivity, viewed: i64| {
            desired_state(Inputs {
                activity: &[a],
                sessions: &sessions,
                hosted: &hosted,
                last_viewed: &BTreeMap::from([(A.to_string(), viewed)]),
                now: NOW,
            })
            .sessions[A]
                .state
                .clone()
        };
        let report = |a: SessionActivity, state: &str, since: i64| SessionActivity {
            program: Some(ProgramStatus { state: state.into(), kind: None, msg: None, since }),
            ..a
        };
        // A long silent tool call: the terminal is quiet, claude says working.
        assert_eq!(state(report(quiet_act(A, NOW - 60, NOW - 50), "working", NOW - 60), NOW - 100), "running");
        // A spinner repaint is loud, but claude says it is waiting on you.
        assert_eq!(state(report(busy_act(A, NOW - 60), "blocked", NOW - 10), NOW - 100), "unread");
        assert_eq!(state(report(busy_act(A, NOW - 60), "blocked", NOW - 10), NOW - 10), "idle", "seen");
        // Esc: nothing new to read, whatever the terminal did.
        assert_eq!(state(report(busy_act(A, NOW - 60), "idle", NOW - 5), NOW - 100), "idle");
        // Unhosted sessions can't have one; a stale report is ignored.
        let unhosted = desired_state(Inputs {
            activity: &[report(act(A, NOW - 60, false), "working", NOW - 60)],
            sessions: &sessions,
            hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(),
            now: NOW + 600,
        });
        assert!(unhosted.sessions.is_empty(), "falls back to the mtime decay");
    }

    #[test]
    fn a_dev_build_publishes_to_its_own_file() {
        // Two apps polling one config dir means two writers on one file, each
        // undoing the other every few seconds — which is what a status bar sees.
        assert_ne!(status_file_name(true), status_file_name(false));
        assert_eq!(status_file_name(false), "session-status.json", "consumers poll this name");
        assert!(status_file_name(true).ends_with(".json"), "still parseable by hand");
    }

    #[test]
    fn states_follow_the_sidebar_rules() {
        let sessions = vec![
            summary(A, "mid turn", NOW - 5, false),  // running (hosted + busy)
            summary(B, "your turn", NOW - 60, true), // unread (viewed before the turn)
            summary(C, "seen", NOW - 600, true),     // idle (viewed after the turn)
            summary(D, "quiet", NOW - 99_999, true), // excluded
        ];
        // A is hosted, so its terminal decides: busy ⇒ running.
        let activity = vec![busy_act(A, NOW - 5)];
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
    fn a_busy_terminal_stays_running_through_a_long_tool_call() {
        // Mid-turn and silent in the .jsonl far past the backstop, but the TUI
        // is still redrawing — a 20-minute build must keep its indicator.
        let sessions = vec![summary(A, "t", NOW - 99_999, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let out = desired_state(Inputs {
            activity: &[busy_act(A, NOW - 99_999)],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &BTreeMap::new(),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "running");
    }

    #[test]
    fn a_hosted_session_with_a_quiet_terminal_is_not_running() {
        // The reported bug: claude asks a question and waits. Claude Code
        // writes nothing until it is answered, so the last turn stays a
        // tool_result (awaiting_input = false) while the PTY sits alive and
        // silent — which read as "running" forever when `hosted` alone
        // bypassed the backstop.
        let sessions = vec![summary(A, "parked on a question", NOW - 5, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        // Five seconds old, so the mtime backstop would say "running" — but the
        // terminal outranks it for a hosted session, which is the whole point:
        // a question asked right after a prompt lands inside that window.
        let out = desired_state(Inputs {
            activity: &[quiet_act(A, NOW - 5, NOW - 3)],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &BTreeMap::new(), // never opened ⇒ can't be unread either
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "idle");
    }

    #[test]
    fn a_session_parked_on_a_question_is_unread() {
        // The other half of that bug: once it stopped reading as "running" it
        // read as nothing at all, when in fact claude is waiting on you. The
        // file can't date this — it still ends on YOUR prompt — so the moment
        // the terminal fell quiet is what "have I seen this?" compares against.
        let sessions = vec![summary(A, "parked on a question", NOW - 30, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let inputs = |viewed: i64| {
            BTreeMap::from([(A.to_string(), viewed)])
        };
        // Prompted at NOW-30 (seen), claude worked until NOW-10 and has sat
        // silent since: that silence is news.
        let out = desired_state(Inputs {
            activity: &[quiet_act(A, NOW - 30, NOW - 10)],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &inputs(NOW - 30),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "unread");

        // …but not once you have looked at it since it went quiet.
        let out = desired_state(Inputs {
            activity: &[quiet_act(A, NOW - 30, NOW - 10)],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &inputs(NOW - 5),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "idle");

        // …and a talking terminal is claude's turn, not yours.
        let out = desired_state(Inputs {
            activity: &[busy_act(A, NOW - 30)],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &inputs(NOW - 30),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "running");
    }

    /// `act` + a hook phase.
    fn hooked(mut a: SessionActivity, phase: &'static str, since: i64) -> SessionActivity {
        a.hook = Some(crate::hooks::HookPhase { phase, since });
        a
    }

    #[test]
    fn a_hook_phase_outranks_every_inference() {
        let sessions = vec![summary(A, "hooked", NOW - 30, false)];
        let viewed = BTreeMap::from([(A.to_string(), NOW - 30)]);
        let state = |activity: Vec<SessionActivity>, hosted: BTreeMap<String, Option<i64>>| {
            desired_state(Inputs {
                activity: &activity,
                sessions: &sessions,
                hosted: &hosted,
                last_viewed: &viewed,
                now: NOW,
            })
            .sessions
            .get(A)
            .map(|s| s.state.clone())
            // Idle + unhosted sessions are excluded from the file entirely.
            .unwrap_or_else(|| "idle".into())
        };
        let hosted_a = || BTreeMap::from([(A.to_string(), Some(1))]);

        // Question on screen: attention says unread even while the terminal is
        // still repainting (the user is browsing the options).
        assert_eq!(state(vec![hooked(busy_act(A, NOW - 30), "attention", NOW - 10)], hosted_a()), "unread");
        // Mid-turn through a silent tool call: running with no volume at all.
        assert_eq!(state(vec![hooked(quiet_act(A, NOW - 30, NOW - 25), "running", NOW - 29)], hosted_a()), "running");
        // Turn finished: unread instantly, no quiet-hold to wait out.
        assert_eq!(state(vec![hooked(quiet_act(A, NOW - 30, NOW - 2), "awaiting", NOW - 1)], hosted_a()), "unread");
        // Not hosted: hook running still decays like the mtime guess would, so
        // a SIGKILLed session (which fires no hook) cannot strand as running.
        assert_eq!(state(vec![hooked(act(A, NOW - 30, false), "running", NOW - 29)], BTreeMap::new()), "running");
        assert_eq!(state(vec![hooked(act(A, NOW - 300, false), "running", NOW - 299)], BTreeMap::new()), "idle");
    }

    #[test]
    fn tool_lines_mid_turn_do_not_hand_the_verdict_back_to_the_terminal() {
        // Hook says running since the prompt; the file has newer tool lines
        // (a turn in progress) and the terminal has been quiet a while (a long
        // API wait). The quiet-terminal guess would call that "parked" → unread.
        let sessions = vec![summary(A, "mid-turn", NOW - 5, false)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let out = desired_state(Inputs {
            activity: &[hooked(quiet_act(A, NOW - 5, NOW - 40), "running", NOW - 120)],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &BTreeMap::from([(A.to_string(), NOW - 300)]),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "running");

        // Unhosted: the file's own lines are the liveness evidence.
        let out = desired_state(Inputs {
            activity: &[hooked(act(A, NOW - 5, false), "running", NOW - 600)],
            sessions: &sessions,
            hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::new(),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "running");
    }

    #[test]
    fn a_finished_or_interrupted_turn_refutes_hook_running() {
        // awaiting_input covers end_turn, an interrupt, and an API error.
        let sessions = vec![summary(A, "done", NOW - 5, true)];
        let mut done = quiet_act(A, NOW - 5, NOW - 40);
        done.awaiting_input = true;
        let out = desired_state(Inputs {
            activity: &[hooked(done, "running", NOW - 120)],
            sessions: &sessions,
            hosted: &BTreeMap::from([(A.to_string(), Some(1))]),
            last_viewed: &BTreeMap::from([(A.to_string(), NOW - 300)]),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "unread", "the file's finished turn wins");
    }

    #[test]
    fn a_stale_hook_phase_defers_to_the_file() {
        // The file has a newer event than the hook saw — an interrupt, or a
        // turn continued elsewhere. Whatever the hook thinks is over.
        let sessions = vec![summary(A, "interrupted", NOW - 5, false)];
        let out = desired_state(Inputs {
            activity: &[hooked(act(A, NOW - 5, false), "attention", NOW - 60)],
            sessions: &sessions,
            hosted: &BTreeMap::new(),
            last_viewed: &BTreeMap::from([(A.to_string(), NOW - 300)]),
            now: NOW,
        });
        // Falls back to the mtime guess: 5s-old file, not awaiting → running.
        assert_eq!(out.sessions[A].state, "running");
    }

    #[test]
    fn a_hosted_session_that_finished_is_not_running() {
        let sessions = vec![summary(A, "t", NOW - 99_999, true)];
        let hosted = BTreeMap::from([(A.to_string(), Some(1))]);
        let out = desired_state(Inputs {
            activity: &[],
            sessions: &sessions,
            hosted: &hosted,
            last_viewed: &BTreeMap::new(),
            now: NOW,
        });
        assert_eq!(out.sessions[A].state, "idle", "awaiting → never running");
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
