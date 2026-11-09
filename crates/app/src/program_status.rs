//! Claude Code's own state reports (Program Status Protocol, `OSC 7501`, ≥ 2.1.295).
//! The probe must be answered before DA1, which xterm.js answers itself, so the PTY reader does it.

use base64::Engine;
use serde::{Deserialize, Serialize};

/// What the PTY writes back to claude's probe: "this terminal speaks 7501".
pub const PROBE_REPLY: &[u8] = b"\x1b]7501;?\x07";

const INTRO: &[u8] = b"\x1b]7501;";

/// Longest payload worth buffering across reads. Claude caps `msg` at 2048
/// characters before base64; anything bigger is not a status sequence.
const MAX_PENDING: usize = 8192;

/// The main session's state as claude last reported it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramStatus {
    /// `working` | `blocked` | `done` | `idle` | `error`.
    pub state: String,
    /// For `blocked`: `question` | `permission` | `auth`, absent for a dialog.
    pub kind: Option<String>,
    /// What it is doing or waiting on, e.g. the question being asked.
    pub msg: Option<String>,
    /// Epoch seconds the session ENTERED this state — a new `msg` or progress
    /// within the same state doesn't re-date it.
    pub since: i64,
}

/// One `OSC 7501` sequence found in claude's output.
#[derive(Debug, PartialEq)]
pub enum Osc {
    /// Claude's "do you support this?" query.
    Probe,
    /// A status report; the raw `key=value:…` payload.
    Report(String),
}

/// Pulls `OSC 7501` sequences out of PTY reads, carrying a sequence that a
/// read boundary split until its terminator arrives.
#[derive(Default)]
pub struct Scanner {
    pending: Vec<u8>,
}

impl Scanner {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Osc> {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(chunk);
        let mut found = Vec::new();
        let mut pos = 0;
        while let Some(start) = find(&data[pos..], INTRO).map(|i| pos + i) {
            let body = start + INTRO.len();
            match terminator(&data[body..]) {
                Some((end, len)) => {
                    let payload = &data[body..body + end];
                    found.push(if payload == b"?" {
                        Osc::Probe
                    } else {
                        Osc::Report(String::from_utf8_lossy(payload).into_owned())
                    });
                    pos = body + end + len;
                }
                None => {
                    if data.len() - start <= MAX_PENDING {
                        self.pending = data[start..].to_vec();
                    }
                    return found;
                }
            }
        }
        // The read may end partway through the introducer itself.
        let tail = &data[pos..];
        if let Some(esc) = tail.iter().rposition(|&b| b == 0x1b) {
            if INTRO.starts_with(&tail[esc..]) {
                self.pending = tail[esc..].to_vec();
            }
        }
        found
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Offset and length of the BEL or ST ending an OSC body.
fn terminator(body: &[u8]) -> Option<(usize, usize)> {
    body.iter().enumerate().find_map(|(i, &b)| match b {
        0x07 => Some((i, 1)),
        0x1b if body.get(i + 1) == Some(&b'\\') => Some((i, 2)),
        _ => None,
    })
}

/// Fold one report into the session's state. Reports carrying an `id` describe
/// background agents and tasks, not the session, so they're ignored.
pub fn apply(current: &mut Option<ProgramStatus>, report: &str, now: i64) {
    let mut state = None;
    let mut kind = None;
    let mut msg = None;
    for field in report.split(':') {
        let (key, value) = field.split_once('=').unwrap_or((field, ""));
        match key {
            "id" => return,
            "state" => state = Some(value),
            "kind" => kind = Some(value.to_string()),
            "msg" => msg = decode(value),
            _ => {}
        }
    }
    match state {
        Some("clear") => *current = None,
        Some(s @ ("working" | "blocked" | "done" | "idle" | "error")) => {
            let since = current.as_ref().filter(|c| c.state == s).map_or(now, |c| c.since);
            *current = Some(ProgramStatus { state: s.to_string(), kind, msg, since });
        }
        _ => {}
    }
}

fn decode(b64: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(s: &str) -> Osc {
        Osc::Report(s.into())
    }

    #[test]
    fn finds_the_probe_and_reports_among_tui_output() {
        let mut sc = Scanner::default();
        let out = sc.feed(b"\x1b[>0q\x1b]7501;?\x07\x1b[c");
        assert_eq!(out, vec![Osc::Probe]);
        let out = sc.feed(b"\x1b[2Jhello\x1b]7501;state=working:app=claude-code\x07more\x1b]7501;state=done:app=claude-code\x1b\\");
        assert_eq!(
            out,
            vec![report("state=working:app=claude-code"), report("state=done:app=claude-code")]
        );
    }

    #[test]
    fn a_sequence_split_across_reads_still_arrives_whole() {
        let whole = b"xx\x1b]7501;state=blocked:app=claude-code:kind=question\x07yy";
        for cut in 1..whole.len() {
            let mut sc = Scanner::default();
            let mut out = sc.feed(&whole[..cut]);
            out.extend(sc.feed(&whole[cut..]));
            assert_eq!(out, vec![report("state=blocked:app=claude-code:kind=question")], "cut at {cut}");
        }
    }

    #[test]
    fn other_oscs_and_runaway_bodies_are_not_buffered() {
        let mut sc = Scanner::default();
        assert!(sc.feed(b"\x1b]0;title\x07\x1b]8;;http://x\x07").is_empty());
        assert!(sc.pending.is_empty());
        sc.feed(b"\x1b]7501;state=working");
        sc.feed(&vec![b'a'; MAX_PENDING + 1]);
        assert!(sc.pending.is_empty(), "an unterminated body is dropped, not grown forever");
    }

    #[test]
    fn folds_the_measured_session_timeline() {
        let mut st = None;
        apply(&mut st, "state=idle:app=claude-code", 7);
        assert_eq!(st.as_ref().unwrap().state, "idle");
        apply(&mut st, "state=working:app=claude-code", 12);
        apply(&mut st, "state=working:app=claude-code:progress=50:msg=cmVhZGluZw==", 13);
        let s = st.clone().unwrap();
        assert_eq!((s.state.as_str(), s.since, s.msg.as_deref()), ("working", 12, Some("reading")));
        let q = base64::engine::general_purpose::STANDARD.encode("answer: Tea or coffee?");
        apply(&mut st, &format!("state=blocked:app=claude-code:kind=question:msg={q}"), 44);
        let s = st.clone().unwrap();
        assert_eq!(s.kind.as_deref(), Some("question"));
        assert_eq!(s.msg.as_deref(), Some("answer: Tea or coffee?"));
        assert_eq!(s.since, 44);
        apply(&mut st, "state=clear", 70);
        assert_eq!(st, None);
    }

    #[test]
    fn background_task_entries_leave_the_session_state_alone() {
        let mut st = None;
        apply(&mut st, "state=done:app=claude-code", 5);
        apply(&mut st, "state=working:app=claude-code:id=agent-1:title=eA==", 6);
        apply(&mut st, "state=clear:id=agent-1", 7);
        assert_eq!(st.unwrap().state, "done");
    }

    #[test]
    fn unknown_states_are_ignored() {
        let mut st = None;
        apply(&mut st, "state=working:app=claude-code", 1);
        apply(&mut st, "state=paused:app=claude-code", 2);
        assert_eq!(st.unwrap().state, "working");
    }
}
