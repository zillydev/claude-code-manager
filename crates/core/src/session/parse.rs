use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::event::Event;

/// Outcome of parsing a single jsonl file.
pub struct ParseOutcome {
    pub events: Vec<Event>,
    /// Count of unparseable lines — exposed for a future warnings surface.
    #[allow(dead_code)]
    pub parse_errors: usize,
    pub bytes_read: u64,
}

/// Read a jsonl file from the start, parsing every line. Unparseable lines
/// are counted but otherwise ignored — we never abort on a single bad line.
pub fn parse_file(path: &Path) -> Result<ParseOutcome> {
    let file = File::open(path)?;
    let bytes_read = file.metadata().map(|m| m.len()).unwrap_or(0);
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut parse_errors = 0;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => {
                parse_errors += 1;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(ev) => events.push(ev),
            Err(_) => parse_errors += 1,
        }
    }

    Ok(ParseOutcome {
        events,
        parse_errors,
        bytes_read,
    })
}

/// Parse new lines appended to a jsonl file since `from_offset`. Returns the
/// new events, parse error count, and the new offset to record.
pub fn parse_tail(path: &Path, from_offset: u64) -> Result<ParseOutcome> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len <= from_offset {
        return Ok(ParseOutcome {
            events: Vec::new(),
            parse_errors: 0,
            bytes_read: from_offset,
        });
    }
    file.seek(SeekFrom::Start(from_offset))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    let mut events = Vec::new();
    let mut parse_errors = 0;

    // Last (possibly partial) line — if it doesn't end with \n we treat the
    // last fragment as incomplete and rewind the offset to just before it.
    let mut consumed = 0usize;
    let bytes = buf.as_bytes();
    let mut start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            let line = &buf[start..i];
            consumed = i + 1;
            start = i + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<Event>(trimmed) {
                Ok(ev) => events.push(ev),
                Err(_) => parse_errors += 1,
            }
        }
    }

    Ok(ParseOutcome {
        events,
        parse_errors,
        bytes_read: from_offset + consumed as u64,
    })
}

/// The last real conversation turn's `(timestamp, awaiting_input)`, read from only
/// the file's tail (no full parse) — the cheap activity probe behind the sidebar
/// running/unread dots. `awaiting_input` is true when that turn is a completed
/// (non-sidechain `end_turn`) assistant turn, i.e. claude is done and it's the
/// user's turn. Skips meta turns and the rename `custom-title` line.
pub fn last_activity(path: &Path) -> Option<(DateTime<Utc>, bool)> {
    const TAIL_BYTES: u64 = 65_536;
    let len = std::fs::metadata(path).ok()?.len();
    let outcome = parse_tail(path, len.saturating_sub(TAIL_BYTES)).ok()?;
    let ev = outcome
        .events
        .iter()
        .rev()
        .find(|ev| ev.is_activity_turn() && ev.timestamp().is_some())?;
    let awaiting = matches!(
        ev,
        Event::Assistant(a)
            if !a.is_sidechain && a.message.stop_reason.as_deref() == Some("end_turn")
    );
    Some((ev.timestamp()?, awaiting))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile_lite::TempPath;

    // tiny inline tempfile helper so we don't add a dev dep just for this
    mod tempfile_lite {
        use std::path::PathBuf;
        pub struct TempPath(pub PathBuf);
        impl Drop for TempPath {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
    }

    fn tmp(name: &str) -> TempPath {
        let mut p = std::env::temp_dir();
        p.push(format!("claude-manager-test-{}-{}.jsonl", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        TempPath(p)
    }

    #[test]
    fn parses_basic_file() {
        let p = tmp("basic");
        let mut f = File::create(&p.0).unwrap();
        writeln!(f, r#"{{"type":"ai-title","aiTitle":"hi","sessionId":"x"}}"#).unwrap();
        writeln!(f, r#"{{"type":"user","uuid":"00000000-0000-0000-0000-000000000001","message":{{"role":"user","content":"hello"}}}}"#).unwrap();
        writeln!(f, "this is not json").unwrap();
        drop(f);

        let out = parse_file(&p.0).unwrap();
        assert_eq!(out.events.len(), 2);
        assert_eq!(out.parse_errors, 1);
    }

    #[test]
    fn tail_only_returns_new_lines() {
        let p = tmp("tail");
        let mut f = File::create(&p.0).unwrap();
        writeln!(f, r#"{{"type":"ai-title","aiTitle":"a","sessionId":"x"}}"#).unwrap();
        drop(f);

        let out1 = parse_file(&p.0).unwrap();
        assert_eq!(out1.events.len(), 1);

        let mut f = std::fs::OpenOptions::new().append(true).open(&p.0).unwrap();
        writeln!(f, r#"{{"type":"ai-title","aiTitle":"b","sessionId":"x"}}"#).unwrap();
        drop(f);

        let out2 = parse_tail(&p.0, out1.bytes_read).unwrap();
        assert_eq!(out2.events.len(), 1);
    }

    #[test]
    fn last_activity_skips_trailing_custom_title() {
        let p = tmp("last_activity_title");
        let mut f = File::create(&p.0).unwrap();
        writeln!(f, r#"{{"type":"user","uuid":"00000000-0000-0000-0000-000000000001","timestamp":"2026-06-22T10:00:05Z","message":{{"role":"user","content":"hello"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"custom-title","customTitle":"renamed"}}"#).unwrap();
        drop(f);

        let expected: DateTime<Utc> = "2026-06-22T10:00:05Z".parse().unwrap();
        // last real turn is a user message → not awaiting (mid-turn).
        assert_eq!(last_activity(&p.0), Some((expected, false)));
    }

    #[test]
    fn last_activity_awaiting_when_assistant_end_turn_last() {
        let p = tmp("last_activity_endturn");
        let mut f = File::create(&p.0).unwrap();
        writeln!(f, r#"{{"type":"user","uuid":"00000000-0000-0000-0000-000000000001","timestamp":"2026-06-22T10:00:05Z","message":{{"role":"user","content":"hi"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","uuid":"00000000-0000-0000-0000-000000000002","timestamp":"2026-06-22T10:00:09Z","message":{{"stop_reason":"end_turn","content":[]}}}}"#).unwrap();
        drop(f);

        let expected: DateTime<Utc> = "2026-06-22T10:00:09Z".parse().unwrap();
        assert_eq!(last_activity(&p.0), Some((expected, true)));
    }

    #[test]
    fn last_activity_none_for_missing_file() {
        assert_eq!(last_activity(Path::new("/no/such/file-xyz.jsonl")), None);
    }
}
