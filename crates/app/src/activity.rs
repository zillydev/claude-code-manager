//! Stamping the poller's activity rows with what only the app knows: which
//! sessions its terminals run, and what claude's hooks last said.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use claude_code_manager_core::session::summary::SessionSummary;
use claude_code_manager_core::store::paths;
use serde::Serialize;

use crate::commands::SessionActivity;
use crate::hooks::HookPhase;
use crate::terminal::{Hosted, BUSY_QUIET_SECS};

/// A session running in a terminal that discovery can't list yet: claude writes
/// its file on the first message, so the row is built from the terminal instead.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PendingSession {
    pub cwd: String,
    pub file_path: String,
    pub project_label: String,
}

fn stamp(a: &mut SessionActivity, h: Option<&Hosted>, hook: Option<&HookPhase>, now: i64) {
    a.hosted = h.is_some();
    a.owner = h.map(|h| h.owner.clone());
    a.busy = h.is_some_and(|h| now - h.last_busy_at < BUSY_QUIET_SECS);
    a.busy_at = h.map_or(0, |h| h.last_busy_at);
    a.hook = hook.cloned();
    a.interrupt_at = h.map_or(0, |h| h.last_interrupt_at);
    a.program = h.and_then(|h| h.program.clone());
}

/// Stamp `hosted`/`busy`/`hook` on the scanned rows, and mint rows for sessions
/// the scan skipped — it only tail-reads files touched in the last 60s, and a
/// long tool call and a parked question both write nothing, exactly when their
/// indicator matters. `sessions` is read only when a row has to be minted.
///
/// `busy` is hosted AND recently talking: a PTY that exists says claude is
/// alive, not that it is working — a session parked on a question is alive and
/// silent, and used to read as "running" forever.
pub fn stamp_hosted(
    activity: &mut Vec<SessionActivity>,
    hosted: &[Hosted],
    hook_state: &HashMap<String, HookPhase>,
    sessions: impl FnOnce() -> Vec<SessionSummary>,
    root: &Path,
    now: i64,
) {
    let by_id: BTreeMap<&str, &Hosted> = hosted.iter().map(|h| (h.session_id.as_str(), h)).collect();
    for a in activity.iter_mut() {
        stamp(a, by_id.get(a.id.as_str()).copied(), hook_state.get(&a.id), now);
    }
    // Awaiting needs no minted row — the discovery snapshot already says so;
    // attention/running have no other carrier.
    let mut want: std::collections::BTreeSet<&str> = by_id.keys().copied().collect();
    want.extend(hook_state.iter().filter(|(_, h)| h.phase != "awaiting").map(|(id, _)| id.as_str()));
    for a in activity.iter() {
        want.remove(a.id.as_str());
    }
    if want.is_empty() {
        return;
    }
    let sessions = sessions();
    for id in want {
        let s = sessions.iter().find(|s| s.id.to_string() == id);
        let h = by_id.get(id).copied();
        let mut row = SessionActivity {
            id: id.to_string(),
            last_event_at: 0,
            awaiting_input: false,
            hosted: false,
            owner: None,
            busy: false,
            hook: None,
            interrupt_at: 0,
            busy_at: 0,
            program: None,
            pending: None,
        };
        stamp(&mut row, h, hook_state.get(id), now);
        match (s.and_then(|s| s.last_event_at), h) {
            (Some(at), _) => {
                row.last_event_at =
                    at.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                row.awaiting_input = s.is_some_and(|s| s.awaiting_input);
            }
            (None, Some(h)) => {
                let file_path = match s {
                    Some(s) => s.file_path.clone(),
                    None => match uuid::Uuid::parse_str(id) {
                        Ok(u) => paths::session_file_path(root, &h.cwd, u),
                        Err(_) => continue,
                    },
                };
                row.last_event_at = h.started_at;
                // Nothing sent yet, so claude is waiting on the user, whatever its
                // startup paint looked like to the busy meter.
                row.awaiting_input = true;
                row.busy = false;
                row.pending = Some(PendingSession {
                    cwd: h.cwd.clone(),
                    file_path: file_path.to_string_lossy().into_owned(),
                    project_label: paths::label_from_cwd(&h.cwd),
                });
            }
            (None, None) => continue,
        }
        activity.push(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    const NOW: i64 = 1_000_000;
    const ID: &str = "11111111-1111-4111-8111-111111111111";

    fn hosted(id: &str) -> Hosted {
        Hosted {
            session_id: id.into(),
            owner: "main".into(),
            last_busy_at: NOW - 1,
            last_interrupt_at: 0,
            program: None,
            cwd: "/work/mono".into(),
            started_at: NOW - 5,
        }
    }

    fn summary(id: &str, last: Option<i64>) -> SessionSummary {
        SessionSummary {
            id: uuid::Uuid::parse_str(id).unwrap(),
            file_path: PathBuf::from(format!("/found/{id}.jsonl")),
            project_label: "mono".into(),
            cwd: Some("/work/mono".into()),
            git_branch: None,
            title: "t".into(),
            mtime: UNIX_EPOCH,
            last_event_at: last.map(|t| UNIX_EPOCH + Duration::from_secs(t as u64)),
            awaiting_input: false,
            total_tokens: 0,
            forked_from: None,
            copied_from: None,
            copied_overlap: None,
            forked_from_message: None,
            copied_from_message: None,
        }
    }

    fn run(hosted: &[Hosted], hooks: &HashMap<String, HookPhase>, sessions: Vec<SessionSummary>) -> Vec<SessionActivity> {
        let mut activity = Vec::new();
        stamp_hosted(&mut activity, hosted, hooks, || sessions, Path::new("/root"), NOW);
        activity
    }

    #[test]
    fn a_hosted_session_with_no_file_gets_a_pending_row() {
        let rows = run(&[hosted(ID)], &HashMap::new(), vec![]);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert!(r.hosted);
        assert_eq!(r.owner.as_deref(), Some("main"));
        assert_eq!(r.last_event_at, NOW - 5, "dated from the spawn");
        let expected = paths::session_file_path(Path::new("/root"), "/work/mono", uuid::Uuid::parse_str(ID).unwrap());
        assert_eq!(
            r.pending,
            Some(PendingSession {
                cwd: "/work/mono".into(),
                file_path: expected.to_string_lossy().into_owned(),
                project_label: "mono".into(),
            })
        );
    }

    #[test]
    fn a_pending_session_reads_as_waiting_through_its_startup_paint() {
        // last_busy_at NOW-1: the boot banner stamped the terminal busy.
        let r = &run(&[hosted(ID)], &HashMap::new(), vec![])[0];
        assert!(!r.busy);
        assert!(r.awaiting_input);
    }

    #[test]
    fn a_pending_row_carries_hook_state() {
        let hooks = HashMap::from([(ID.to_string(), HookPhase { phase: "attention", since: NOW - 2 })]);
        let r = &run(&[hosted(ID)], &hooks, vec![])[0];
        assert_eq!(r.hook, Some(HookPhase { phase: "attention", since: NOW - 2 }));
        assert!(r.pending.is_some());
    }

    #[test]
    fn a_discovered_session_with_no_turns_is_pending_at_its_own_path() {
        let rows = run(&[hosted(ID)], &HashMap::new(), vec![summary(ID, None)]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pending.as_ref().unwrap().file_path, format!("/found/{ID}.jsonl"));
    }

    #[test]
    fn a_discovered_session_with_turns_is_minted_without_pending() {
        let rows = run(&[hosted(ID)], &HashMap::new(), vec![summary(ID, Some(NOW - 100))]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].last_event_at, NOW - 100);
        assert!(rows[0].busy, "a real session keeps its terminal's busy stamp");
        assert_eq!(rows[0].pending, None);
    }

    #[test]
    fn an_unhosted_session_with_no_file_is_never_minted() {
        let hooks = HashMap::from([(ID.to_string(), HookPhase { phase: "running", since: NOW })]);
        assert!(run(&[], &hooks, vec![]).is_empty());
    }

    #[test]
    fn scanned_rows_are_stamped_not_duplicated() {
        let mut activity = vec![SessionActivity {
            id: ID.into(),
            last_event_at: NOW - 3,
            awaiting_input: false,
            hosted: false,
            owner: None,
            busy: false,
            hook: None,
            interrupt_at: 0,
            busy_at: 0,
            program: None,
            pending: None,
        }];
        stamp_hosted(&mut activity, &[hosted(ID)], &HashMap::new(), || panic!("no row to mint"), Path::new("/root"), NOW);
        assert_eq!(activity.len(), 1);
        assert!(activity[0].hosted && activity[0].busy);
        assert_eq!(activity[0].pending, None);
    }
}
