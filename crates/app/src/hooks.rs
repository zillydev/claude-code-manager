//! Precise session state from Claude Code's own hooks.
//!
//! The `.jsonl` cannot see a question that hasn't been answered, and a PTY can
//! only guess from repaint traffic — but claude announces every transition
//! itself if asked: `UserPromptSubmit` when a turn starts, `PreToolUse` when an
//! `AskUserQuestion`/`ExitPlanMode` card goes up, `PermissionRequest` when it
//! waits for approval, `Stop` when it finishes. A tiny relay (the bundled
//! `ccm-status` Claude Code plugin, see precise_status.rs) appends each event to an
//! ndjson file in the config dir; the poller tails it and folds events into a
//! per-session phase.
//!
//! Hook state is treated as a WITNESS, not the authority: it wins only while it
//! is at least as new as the session file's last event, so a missed edge (Esc
//! interrupts and SIGKILLs fire no hook) strands nothing — the file-based
//! derivation reclaims the verdict on its next line. That reconciliation is
//! what the measured 156-prompts-to-1-Stop starvation of a hooks-only state
//! machine was missing.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Value};

/// Marker in the hook commands ccm used to write into `~/.claude/settings.json`,
/// which is how those entries are found and removed without touching anyone else's.
const RELAY_NAME: &str = "claude-hook-relay.py";

/// Events file the relay appends to and the poller tails.
pub const EVENTS_FILE: &str = "hook-events.ndjson";

pub fn events_path() -> PathBuf {
    crate::config::config_dir().join(EVENTS_FILE)
}

/// (event, PreToolUse/PostToolUse matcher) pairs the plugin's `hooks.json` registers, pinned by test.
/// PermissionRequest and Notification fire rarely, so they need no matcher.
#[cfg(test)]
const HOOK_EVENTS: &[(&str, Option<&str>)] = &[
    ("SessionStart", None),
    ("SessionEnd", None),
    ("UserPromptSubmit", None),
    ("Stop", None),
    ("StopFailure", None),
    ("PreToolUse", Some("AskUserQuestion|ExitPlanMode")),
    ("PostToolUse", Some("AskUserQuestion|ExitPlanMode")),
    ("PermissionRequest", None),
    ("Notification", None),
];

/// How the bundled plugin runs its relay: python by absolute path, so the bundle needn't keep an execute bit.
#[cfg(test)]
const PLUGIN_RELAY_COMMAND: &str = r#"/usr/bin/python3 "${CLAUDE_PLUGIN_ROOT}/relay.py""#;

/// Add our hook entries to a parsed `~/.claude/settings.json`, idempotently:
/// any previous entries of ours are removed first, everyone else's are kept.
#[cfg(test)]
pub fn install_into(settings: &mut Value, relay_cmd: &str) {
    uninstall_from(settings);
    if !settings.is_object() {
        *settings = json!({});
    }
    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    for (event, matcher) in HOOK_EVENTS {
        let mut group = serde_json::Map::new();
        if let Some(m) = matcher {
            group.insert("matcher".into(), json!(m));
        }
        group.insert("hooks".into(), json!([{ "type": "command", "command": relay_cmd }]));
        let arr = hooks
            .as_object_mut()
            .unwrap()
            .entry(*event)
            .or_insert_with(|| json!([]));
        if let Some(a) = arr.as_array_mut() {
            a.push(Value::Object(group));
        }
    }
}

/// Remove every hook entry whose command mentions our relay; prune what
/// empties. The user's own hooks (and unknown settings) pass through untouched.
pub fn uninstall_from(settings: &mut Value) {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return;
    };
    for (_, groups) in hooks.iter_mut() {
        if let Some(arr) = groups.as_array_mut() {
            arr.retain(|g| {
                let ours = g["hooks"].as_array().is_some_and(|hs| {
                    !hs.is_empty()
                        && hs.iter().all(|h| {
                            h["command"].as_str().is_some_and(|c| c.contains(RELAY_NAME))
                        })
                });
                !ours
            });
        }
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        settings.as_object_mut().unwrap().remove("hooks");
    }
}

/// Whether a parsed settings file carries our entries.
pub fn is_installed(settings: &Value) -> bool {
    settings["hooks"].as_object().is_some_and(|events| {
        events.values().any(|groups| {
            groups.as_array().is_some_and(|a| {
                a.iter().any(|g| {
                    g["hooks"].as_array().is_some_and(|hs| {
                        hs.iter().any(|h| {
                            h["command"].as_str().is_some_and(|c| c.contains(RELAY_NAME))
                        })
                    })
                })
            })
        })
    })
}

/// One relayed hook event.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HookEvent {
    #[serde(default)]
    pub ts: f64,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub sid: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub notif: String,
    #[serde(default)]
    pub src: String,
}

/// What claude last said it was doing, per session.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HookPhase {
    /// `running` | `attention` (a question/permission is on screen) | `awaiting`.
    pub phase: &'static str,
    /// Epoch seconds the session ENTERED this phase — repeats don't re-date it,
    /// so "unread since" stays the moment the question first appeared.
    pub since: i64,
}

fn is_ask(tool: &str) -> bool {
    tool == "AskUserQuestion" || tool == "ExitPlanMode"
}

/// Fold new events into the per-session phase map. Incremental so the poller
/// can feed it tailed lines each tick.
pub fn fold_into(state: &mut HashMap<String, HookPhase>, events: &[HookEvent]) {
    for ev in events {
        if ev.sid.is_empty() {
            continue;
        }
        let phase = match ev.event.as_str() {
            "UserPromptSubmit" => Some("running"),
            "PreToolUse" if is_ask(&ev.tool) => Some("attention"),
            "PermissionRequest" => Some("attention"),
            "Notification" if ev.notif == "permission_prompt" => Some("attention"),
            "PostToolUse" if is_ask(&ev.tool) => Some("running"),
            "Stop" | "StopFailure" => Some("awaiting"),
            // A restart/resume/clear lands claude at an empty prompt; whatever
            // we knew is stale. Mid-turn auto-compaction also fires this, with
            // src=compact — the turn is still going, so keep the phase.
            "SessionStart" if ev.src != "compact" => None,
            "SessionEnd" => None,
            _ => continue,
        };
        let ts = ev.ts as i64;
        match phase {
            None => {
                state.remove(&ev.sid);
            }
            Some(p) => {
                let keep = state.get(&ev.sid).filter(|c| c.phase == p).map(|c| c.since);
                state.insert(ev.sid.clone(), HookPhase { phase: p, since: keep.unwrap_or(ts) });
            }
        }
    }
}

/// Tail of the events file across poller ticks; survives rotation by starting
/// over when the file shrinks.
pub struct EventTail {
    offset: u64,
}

impl EventTail {
    pub fn new() -> Self {
        EventTail { offset: 0 }
    }

    pub fn read_new(&mut self, path: &std::path::Path) -> Vec<HookEvent> {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            self.offset = 0; // rotated
        }
        if len == self.offset {
            return Vec::new();
        }
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_err() {
            return Vec::new();
        }
        // A line still being written has no newline yet; leave it for next tick.
        let complete = match buf.rfind('\n') {
            Some(i) => &buf[..=i],
            None => return Vec::new(),
        };
        self.offset += complete.len() as u64;
        complete.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }
}

/// Our entries from before the plugin, still in `~/.claude/settings.json`.
pub fn legacy_installed() -> bool {
    read_settings().map(|s| is_installed(&s)).unwrap_or(false)
}

/// Removes those entries, leaving everyone else's hooks and settings untouched. Backs the file up once first.
pub fn remove_legacy() -> Result<(), String> {
    edit_settings(uninstall_from)
}

fn claude_settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude").join("settings.json")
}

fn read_settings() -> Result<Value, String> {
    let path = claude_settings_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("~/.claude/settings.json is not valid JSON: {e}"))
}

fn edit_settings(f: impl FnOnce(&mut Value)) -> Result<(), String> {
    let path = claude_settings_path();
    let mut settings = read_settings()?;
    let backup = path.with_extension("json.ccm-backup");
    if path.exists() && !backup.exists() {
        std::fs::copy(&path, &backup).map_err(|e| e.to_string())?;
    }
    f(&mut settings);
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_plugin_registers_exactly_our_hook_events() {
        let bundled: Value =
            serde_json::from_str(include_str!("../plugin/plugins/ccm-status/hooks/hooks.json")).unwrap();
        let hooks = bundled["hooks"].as_object().expect("hooks object");
        let mut got: Vec<(String, Option<String>, String)> = hooks
            .iter()
            .flat_map(|(event, groups)| {
                groups.as_array().unwrap().iter().map(move |g| {
                    let cmd = g["hooks"][0]["command"].as_str().unwrap().to_string();
                    (event.clone(), g["matcher"].as_str().map(str::to_string), cmd)
                })
            })
            .collect();
        got.sort();
        let mut want: Vec<(String, Option<String>, String)> = HOOK_EVENTS
            .iter()
            .map(|(e, m)| (e.to_string(), m.map(str::to_string), PLUGIN_RELAY_COMMAND.to_string()))
            .collect();
        want.sort();
        assert_eq!(got, want);
    }

    fn ev(line: &str) -> HookEvent {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn folds_the_measured_question_timeline() {
        // The sequence a live claude fired around a real AskUserQuestion,
        // relayed: prompt → question up (PreToolUse + PermissionRequest in the
        // same second) → six option-toggles (no events at all) → answered →
        // finished. Phases must land exactly on those edges.
        let mut state = HashMap::new();
        let sid = "s1";
        let feed = |state: &mut HashMap<String, HookPhase>, line: String| {
            fold_into(state, &[ev(&line)]);
            state.get(sid).cloned()
        };
        let mk = |ts: f64, event: &str, tool: &str| {
            format!(r#"{{"ts":{ts},"event":"{event}","sid":"s1","tool":"{tool}","notif":"","src":""}}"#)
        };
        assert_eq!(feed(&mut state, mk(13.5, "UserPromptSubmit", "")).unwrap(), HookPhase { phase: "running", since: 13 });
        assert_eq!(feed(&mut state, mk(19.4, "PreToolUse", "AskUserQuestion")).unwrap().phase, "attention");
        // The permission twin a moment later must not re-date the wait.
        assert_eq!(feed(&mut state, mk(19.5, "PermissionRequest", "AskUserQuestion")).unwrap(), HookPhase { phase: "attention", since: 19 });
        assert_eq!(feed(&mut state, mk(31.6, "PostToolUse", "AskUserQuestion")).unwrap().phase, "running");
        assert_eq!(feed(&mut state, mk(33.5, "Stop", "")).unwrap(), HookPhase { phase: "awaiting", since: 33 });
    }

    #[test]
    fn folds_a_live_relay_capture_verbatim() {
        // Captured from a real session driven through an AskUserQuestion, with
        // the relay wired exactly as the installer wires it.
        let lines = [
            r#"{"ts":1788185783.8,"event":"SessionStart","sid":"ccf2f4b3","tool":"","notif":"","src":"startup"}"#,
            r#"{"ts":1788185788.69,"event":"UserPromptSubmit","sid":"ccf2f4b3","tool":"","notif":"","src":""}"#,
            r#"{"ts":1788185793.03,"event":"PreToolUse","sid":"ccf2f4b3","tool":"AskUserQuestion","notif":"","src":""}"#,
            r#"{"ts":1788185793.07,"event":"PermissionRequest","sid":"ccf2f4b3","tool":"AskUserQuestion","notif":"","src":""}"#,
            r#"{"ts":1788185799.08,"event":"Notification","sid":"ccf2f4b3","tool":"","notif":"permission_prompt","src":""}"#,
            r#"{"ts":1788185805.01,"event":"PostToolUse","sid":"ccf2f4b3","tool":"AskUserQuestion","notif":"","src":""}"#,
            r#"{"ts":1788185806.84,"event":"Stop","sid":"ccf2f4b3","tool":"","notif":"","src":""}"#,
        ];
        let mut state = HashMap::new();
        let seq: Vec<Option<(&str, i64)>> = lines
            .iter()
            .map(|l| {
                fold_into(&mut state, &[ev(l)]);
                state.get("ccf2f4b3").map(|p| (p.phase, p.since))
            })
            .collect();
        assert_eq!(seq, vec![
            None, // SessionStart: an empty prompt — nothing to claim yet
            Some(("running", 1788185788)),
            Some(("attention", 1788185793)),
            Some(("attention", 1788185793)), // the permission twin doesn't re-date it
            Some(("attention", 1788185793)), // nor does the OS notification 6s later
            Some(("running", 1788185805)),
            Some(("awaiting", 1788185806)),
        ]);
    }

    #[test]
    fn a_restart_clears_but_a_compaction_does_not() {
        let mut state = HashMap::new();
        let start = |src: &str| {
            format!(r#"{{"ts":50,"event":"SessionStart","sid":"s1","tool":"","notif":"","src":"{src}"}}"#)
        };
        fold_into(&mut state, &[ev(r#"{"ts":10,"event":"UserPromptSubmit","sid":"s1"}"#)]);
        fold_into(&mut state, &[ev(&start("compact"))]);
        assert!(state.contains_key("s1"), "mid-turn auto-compaction must not drop the phase");
        fold_into(&mut state, &[ev(&start("resume"))]);
        assert!(!state.contains_key("s1"), "a resume lands at an empty prompt — old state is stale");
    }

    #[test]
    fn other_tools_and_notifications_do_not_flag_attention() {
        let mut state = HashMap::new();
        fold_into(&mut state, &[
            ev(r#"{"ts":1,"event":"UserPromptSubmit","sid":"s1"}"#),
            ev(r#"{"ts":2,"event":"PreToolUse","sid":"s1","tool":"Bash"}"#),
            ev(r#"{"ts":3,"event":"Notification","sid":"s1","notif":"idle_prompt"}"#),
        ]);
        assert_eq!(state["s1"].phase, "running");
        fold_into(&mut state, &[ev(r#"{"ts":4,"event":"Notification","sid":"s1","notif":"permission_prompt"}"#)]);
        assert_eq!(state["s1"].phase, "attention");
    }

    #[test]
    fn install_preserves_everyone_else_and_round_trips() {
        // The user's real shape: their own state script on Stop/UserPromptSubmit.
        let mut settings = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "$HOME/.claude/hooks/claude-state.sh" }] }],
                "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "$HOME/.claude/hooks/claude-state.sh" }] }]
            }
        });
        let before = settings.clone();
        install_into(&mut settings, "/cfg/claude-hook-relay.py");
        install_into(&mut settings, "/cfg/claude-hook-relay.py"); // idempotent
        assert!(is_installed(&settings));
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 2, "theirs + ours, once");
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["matcher"], "AskUserQuestion|ExitPlanMode",
            "only the tools whose card means 'your turn' spawn the relay"
        );
        uninstall_from(&mut settings);
        assert!(!is_installed(&settings));
        assert_eq!(settings, before, "uninstall must be an exact inverse around their config");
    }

    #[test]
    fn editing_settings_keeps_the_users_key_order() {
        // The toggle rewrites a file the user hand-edits; alphabetizing it
        // would read as vandalism. Needs serde_json's preserve_order.
        let mut settings: Value =
            serde_json::from_str(r#"{"zeta":1,"model":"opus","alpha":2}"#).unwrap();
        install_into(&mut settings, "/cfg/claude-hook-relay.py");
        uninstall_from(&mut settings);
        assert_eq!(serde_json::to_string(&settings).unwrap(), r#"{"zeta":1,"model":"opus","alpha":2}"#);
    }

    #[test]
    fn tail_survives_partial_lines_and_rotation() {
        let dir = std::env::temp_dir().join(format!("ccm-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("ev.ndjson");
        let mut tail = EventTail::new();
        std::fs::write(&f, "{\"ts\":1,\"event\":\"Stop\",\"sid\":\"a\"}\n{\"ts\":2,\"ev").unwrap();
        assert_eq!(tail.read_new(&f).len(), 1, "the half-written line waits for its newline");
        std::fs::write(&f, "{\"ts\":1,\"event\":\"Stop\",\"sid\":\"a\"}\n{\"ts\":2,\"event\":\"Stop\",\"sid\":\"b\"}\n").unwrap();
        assert_eq!(tail.read_new(&f).len(), 1, "only the completed remainder");
        // Rotation shrinks the file: start over rather than misread offsets.
        std::fs::write(&f, "{\"ts\":9,\"event\":\"Stop\",\"sid\":\"c\"}\n").unwrap();
        let evs = tail.read_new(&f);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].sid, "c");
        std::fs::remove_dir_all(&dir).ok();
    }
}
