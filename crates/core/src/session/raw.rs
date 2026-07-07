use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

/// One physical line of a session jsonl, kept as raw JSON so no fields are lost
/// on a read→write round-trip.
#[derive(Debug, Clone)]
pub struct RawLine {
    pub value: Value,
    pub uuid: Option<Uuid>,
    pub parent: Option<Uuid>,
    pub kind: String,
}

impl RawLine {
    pub fn from_value(value: Value) -> Self {
        let uuid = value
            .get("uuid")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok());
        let parent = value
            .get("parentUuid")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok());
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Self { value, uuid, parent, kind }
    }
}

/// Read every line of a session file as raw JSON. Blank/unparseable lines are skipped.
pub fn read_raw(path: &Path) -> Result<Vec<RawLine>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            out.push(RawLine::from_value(v));
        }
    }
    Ok(out)
}

/// Resolve the active leaf: the last line in file order that has a uuid.
/// (Mirrors `tree::resolve_leaf` — the append-only tip is the true active node.)
pub fn resolve_leaf(lines: &[RawLine]) -> Option<Uuid> {
    lines.iter().rev().find_map(|l| l.uuid)
}

/// Return the path ending at `leaf`, oldest first, by walking `parent` back to
/// its tree root. Works for ANY node — including a message in an orphaned
/// pre-compaction tree — so a fork can be built from the fork point's own tree
/// rather than only the active path. Lines without a uuid (metadata) are skipped.
pub fn path_to(lines: &[RawLine], leaf: Uuid) -> Vec<&RawLine> {
    let mut by_uuid: HashMap<Uuid, &RawLine> = HashMap::with_capacity(lines.len());
    for l in lines {
        if let Some(u) = l.uuid {
            by_uuid.insert(u, l);
        }
    }
    let mut path = Vec::new();
    let mut cursor = Some(leaf);
    let mut guard = 0u32;
    while let Some(u) = cursor {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        let Some(l) = by_uuid.get(&u) else { break };
        path.push(*l);
        cursor = l.parent;
    }
    path.reverse();
    path
}

/// Return references to the lines on the active path, oldest first, by walking
/// `parent` from the leaf. Lines without a uuid (metadata) are not on the path.
pub fn active_path(lines: &[RawLine]) -> Vec<&RawLine> {
    match resolve_leaf(lines) {
        Some(leaf) => path_to(lines, leaf),
        None => Vec::new(),
    }
}

/// Total input tokens the model was sent for this line's turn — `input +
/// cache_read + cache_creation` from its `usage` — i.e. the true size of the
/// context window at that point. 0 for non-assistant or unmetered lines.
fn context_tokens(l: &RawLine) -> u64 {
    let usage = l.value.get("message").and_then(|m| m.get("usage"));
    let field = |k: &str| usage.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    field("input_tokens") + field("cache_read_input_tokens") + field("cache_creation_input_tokens")
}

/// Whether `l` is a real user prompt (a turn start) rather than a tool_result
/// echo or metadata — a safe, resumable place to begin a fork.
fn is_user_prompt(l: &RawLine) -> bool {
    if l.kind != "user" {
        return false;
    }
    match l.value.get("message").and_then(|m| m.get("content")) {
        Some(Value::String(_)) => true,
        Some(Value::Array(blocks)) => {
            blocks.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        }
        _ => false,
    }
}

/// Index into a fork `prefix` (as returned by [`path_to`]) at which claude's
/// current context segment begins.
///
/// Claude shrinks its live context (auto-compaction, rewind) without always
/// leaving a summary on the `parentUuid` chain, so replaying a raw prefix can
/// feed the model far more than it actually held — overflowing the window on
/// resume ("Prompt is too long") and forcing an immediate compaction. Each
/// assistant turn's `usage` records the true context size; a large drop marks a
/// reset. Return the start of the LAST reset before the end, snapped to a clean
/// user-prompt turn start, so a fork replays ≈ what claude really loads. Returns
/// 0 when there is no reset (an uncompacted session forks whole, unchanged).
pub fn context_segment_start(prefix: &[&RawLine]) -> usize {
    let mut peak: u64 = 0;
    let mut reset: usize = 0;
    for (i, l) in prefix.iter().enumerate() {
        let t = context_tokens(l);
        if t == 0 {
            continue;
        }
        // A big, unambiguous drop — under half the running peak AND >100k tokens
        // — is a context reset, not prompt-cache churn (which re-creates the
        // cache but keeps the total roughly the same).
        if peak > 100_000 && t.saturating_mul(2) < peak && peak - t > 100_000 {
            reset = i;
            peak = t;
        } else if t > peak {
            peak = t;
        }
    }
    if reset == 0 {
        return 0;
    }
    // Snap to a clean turn start: prefer a user prompt at/just before the reset
    // (its turn start, or a compaction summary sitting there), bounded so we
    // never walk back into the discarded pre-reset bulk; else the next one.
    let lo = reset.saturating_sub(8);
    (lo..=reset)
        .rev()
        .find(|&i| is_user_prompt(prefix[i]))
        .or_else(|| (reset..prefix.len()).find(|&i| is_user_prompt(prefix[i])))
        .unwrap_or(reset)
}

/// Every conversation tree in the file, oldest first; each is the path of one
/// tree root. The LAST element is the active tree (the one containing the global
/// leaf) and equals `active_path`. Mirrors `tree::segments`, but on `RawLine`:
/// a compaction writes a fresh null-parent root, orphaning the prior tree in the
/// same file, so each root yields its own segment.
pub fn segments(lines: &[RawLine]) -> Vec<Vec<&RawLine>> {
    let mut by_uuid: HashMap<Uuid, &RawLine> = HashMap::with_capacity(lines.len());
    for l in lines {
        if let Some(u) = l.uuid {
            by_uuid.insert(u, l);
        }
    }
    if by_uuid.is_empty() {
        return Vec::new();
    }

    fn root_of(
        start: Uuid,
        by_uuid: &HashMap<Uuid, &RawLine>,
        cache: &mut HashMap<Uuid, Uuid>,
    ) -> Uuid {
        if let Some(r) = cache.get(&start) {
            return *r;
        }
        let mut chain: Vec<Uuid> = Vec::new();
        let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        let mut cur = start;
        let root = loop {
            if !seen.insert(cur) {
                break cur; // cycle: treat current node as the root
            }
            chain.push(cur);
            match by_uuid.get(&cur).and_then(|l| l.parent) {
                Some(p) if by_uuid.contains_key(&p) => cur = p,
                _ => break cur,
            }
        };
        for c in chain {
            cache.insert(c, root);
        }
        root
    }

    let mut cache: HashMap<Uuid, Uuid> = HashMap::with_capacity(by_uuid.len());
    let mut groups: HashMap<Uuid, Vec<&RawLine>> = HashMap::new();
    let mut root_order: Vec<Uuid> = Vec::new();
    for l in lines {
        let Some(u) = l.uuid else { continue };
        let r = root_of(u, &by_uuid, &mut cache);
        if !groups.contains_key(&r) {
            root_order.push(r);
        }
        groups.entry(r).or_default().push(l);
    }

    let active_root = resolve_leaf(lines).map(|leaf| root_of(leaf, &by_uuid, &mut cache));
    let mut ordered: Vec<Uuid> =
        root_order.iter().copied().filter(|r| Some(*r) != active_root).collect();
    if let Some(ar) = active_root {
        ordered.push(ar);
    }

    let mut out: Vec<Vec<&RawLine>> = Vec::with_capacity(ordered.len());
    for r in ordered {
        if let Some(tree_leaf) = groups[&r].last().and_then(|l| l.uuid) {
            out.push(path_to(lines, tree_leaf));
        }
    }
    out
}

/// The session's launch cwd: the first `cwd` field in FILE ORDER. This is the
/// directory the session was started in (and thus the project dir it lives in),
/// even if the agent later `cd`'d elsewhere or the active path begins
/// post-compaction in a different directory. Copy-on-edit writers use this so a
/// new session lands in the same project dir as its source and stays resumable
/// (Claude Code resolves `--resume <id>` within the current directory's project).
pub fn first_cwd(lines: &[RawLine]) -> Option<String> {
    lines
        .iter()
        .find_map(|l| l.value.get("cwd").and_then(Value::as_str).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(uuid: &str, parent: Option<&str>, kind: &str) -> String {
        let p = parent
            .map(|p| format!("\"{}\"", p))
            .unwrap_or_else(|| "null".to_string());
        format!(
            r#"{{"type":"{}","uuid":"{}","parentUuid":{},"message":{{"role":"user","content":"x"}}}}"#,
            kind, uuid, p
        )
    }

    fn write_tmp(name: &str, body: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("cm-raw-{}-{}.jsonl", name, std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn active_path_walks_parents_from_last_uuid() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, Some(b), "user"),
            r#"{"type":"last-prompt","leafUuid":"00000000-0000-0000-0000-0000000000c3","lastPrompt":"x"}"#
        );
        let path = write_tmp("walk", &body);
        let lines = read_raw(&path).unwrap();
        let active = active_path(&lines);
        let ids: Vec<String> = active.iter().filter_map(|l| l.uuid.map(|u| u.to_string())).collect();
        assert_eq!(ids, vec![a.to_string(), b.to_string(), c.to_string()]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn active_path_follows_appended_branch_after_fork() {
        // a -> b (abandoned), then a -> c (the active branch, appended last).
        // The leaf is the last line with a uuid (c), so the path is a -> c, not a -> b.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let body = format!(
            "{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, Some(a), "assistant"),
        );
        let path = write_tmp("fork", &body);
        let lines = read_raw(&path).unwrap();
        let ids: Vec<String> = active_path(&lines)
            .iter()
            .filter_map(|l| l.uuid.map(|u| u.to_string()))
            .collect();
        assert_eq!(ids, vec![a.to_string(), c.to_string()]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn first_cwd_is_the_file_order_launch_dir() {
        // The launch cwd is the FIRST cwd in file order — not whatever the agent
        // had cd'd into later (here the second line carries an inner dir).
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"cwd":"/launch","message":{"role":"user","content":"x"}}"#,
            "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","cwd":"/launch/inner","message":{"role":"user","content":"y"}}"#,
            "\n",
        );
        let path = write_tmp("firstcwd", body);
        let lines = read_raw(&path).unwrap();
        assert_eq!(first_cwd(&lines).as_deref(), Some("/launch"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn path_to_walks_the_targets_own_tree_not_the_active_one() {
        // Two disconnected trees (mirrors a compaction): tree 1 a->b, tree 2
        // (active) c->d. path_to(b) returns tree 1's path, so a fork from a
        // pre-compaction message yields that message's own history — while
        // active_path still resolves to the active tree.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let d = "00000000-0000-0000-0000-0000000000d4";
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, None, "user"),
            line(d, Some(c), "assistant"),
        );
        let path = write_tmp("pathto", &body);
        let lines = read_raw(&path).unwrap();
        let ids = |ls: Vec<&RawLine>| -> Vec<String> {
            ls.iter().filter_map(|l| l.uuid.map(|u| u.to_string())).collect()
        };
        assert_eq!(ids(path_to(&lines, b.parse().unwrap())), vec![a.to_string(), b.to_string()]);
        assert_eq!(ids(active_path(&lines)), vec![c.to_string(), d.to_string()]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn segments_single_tree_equals_active_path() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let body = format!(
            "{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, Some(b), "user"),
        );
        let p = write_tmp("seg-one", &body);
        let lines = read_raw(&p).unwrap();
        let segs = segments(&lines);
        assert_eq!(segs.len(), 1);
        let seg_ids: Vec<_> = segs[0].iter().map(|l| l.uuid).collect();
        let ap_ids: Vec<_> = active_path(&lines).iter().map(|l| l.uuid).collect();
        assert_eq!(seg_ids, ap_ids);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn segments_two_roots_active_last() {
        // Pre-compaction tree (a->b), then a fresh null-parent root (c->d) appended.
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let c = "00000000-0000-0000-0000-0000000000c3";
        let d = "00000000-0000-0000-0000-0000000000d4";
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(c, None, "user"),
            line(d, Some(c), "assistant"),
        );
        let p = write_tmp("seg-two", &body);
        let lines = read_raw(&p).unwrap();
        let segs = segments(&lines);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].last().unwrap().uuid.unwrap().to_string(), d);
        assert_eq!(segs[0].first().unwrap().uuid.unwrap().to_string(), a);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn segments_dangling_parent_starts_new_tree() {
        let a = "00000000-0000-0000-0000-0000000000a1";
        let b = "00000000-0000-0000-0000-0000000000b2";
        let x = "00000000-0000-0000-0000-0000000000e5";
        let missing = "00000000-0000-0000-0000-0000000000ff";
        let body = format!(
            "{}\n{}\n{}\n",
            line(a, None, "user"),
            line(b, Some(a), "assistant"),
            line(x, Some(missing), "user"),
        );
        let p = write_tmp("seg-dangle", &body);
        let lines = read_raw(&p).unwrap();
        assert_eq!(segments(&lines).len(), 2);
        std::fs::remove_file(&p).ok();
    }

    // --- context_segment_start (fork trim) ---

    fn asst(ctx: u64) -> RawLine {
        RawLine::from_value(serde_json::json!({
            "type": "assistant",
            "uuid": Uuid::new_v4().to_string(),
            "message": { "role": "assistant", "usage": {
                "input_tokens": 0, "cache_read_input_tokens": ctx, "cache_creation_input_tokens": 0
            }}
        }))
    }
    fn prompt(text: &str) -> RawLine {
        RawLine::from_value(serde_json::json!({
            "type": "user", "uuid": Uuid::new_v4().to_string(),
            "message": { "role": "user", "content": text }
        }))
    }
    fn tool_result() -> RawLine {
        RawLine::from_value(serde_json::json!({
            "type": "user", "uuid": Uuid::new_v4().to_string(),
            "message": { "role": "user", "content": [{ "type": "tool_result", "content": "x" }] }
        }))
    }
    fn refs(lines: &[RawLine]) -> Vec<&RawLine> {
        lines.iter().collect()
    }

    #[test]
    fn is_user_prompt_distinguishes_prompt_from_tool_result() {
        assert!(is_user_prompt(&prompt("hi")));
        assert!(!is_user_prompt(&tool_result()));
        assert!(!is_user_prompt(&asst(100)));
    }

    #[test]
    fn segment_start_is_zero_without_a_reset() {
        // Monotonic growth: nothing to trim, fork the whole prefix.
        let p = vec![
            prompt("a"), asst(100_000), prompt("b"), asst(300_000), prompt("c"), asst(500_000),
        ];
        assert_eq!(context_segment_start(&refs(&p)), 0);
    }

    #[test]
    fn segment_start_trims_to_last_reset_snapped_to_prompt() {
        // Grows to 900k, a compaction drops it to 150k, then grows again.
        let p = vec![
            prompt("a"), asst(400_000), prompt("b"), asst(900_000), // pre-reset (discarded)
            prompt("c"), asst(150_000), prompt("d"), asst(300_000), // post-reset (kept)
        ];
        // Reset detected at the 150k turn (idx 5), snapped back to prompt "c" (idx 4).
        let start = context_segment_start(&refs(&p));
        assert_eq!(start, 4);
        assert!(is_user_prompt(refs(&p)[start]));
    }

    #[test]
    fn segment_start_ignores_cache_churn() {
        // A dip that is NOT a real reset: >50% retained, so the total barely
        // moves (prompt-cache re-creation, not a compaction). No trim.
        let p = vec![
            prompt("a"), asst(500_000), prompt("b"), asst(400_000), prompt("c"), asst(600_000),
        ];
        assert_eq!(context_segment_start(&refs(&p)), 0);
    }

    #[test]
    fn segment_start_keeps_only_the_last_of_several_resets() {
        // Two compactions: keep from the most recent one.
        let p = vec![
            prompt("a"), asst(900_000),                 // 0,1
            prompt("b"), asst(150_000), asst(400_000),  // 2,3,4  first reset
            prompt("c"), asst(120_000), prompt("d"), asst(250_000), // 5,6,7,8 second reset
        ];
        // Last reset at idx 6 (120k), snapped back to prompt "c" (idx 5).
        assert_eq!(context_segment_start(&refs(&p)), 5);
    }
}
