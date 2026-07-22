# Harvest Across All Segments (v2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `session-harvest` mine every conversation tree in a session (pre-compaction trees + active), via a `surgery extract --all-segments` flag that is OFF by default (so session-surgery is unaffected).

**Architecture:** Add `raw::segments` (raw-line mirror of `tree::segments`). Extend `surgery::extract` with an `all_segments: bool`: when set, it iterates segments — whole-session output gets `── segment k/N · pre-compaction|active ──` dividers, and a `--from/--to` range resolves within the segment containing those uuids. Thread the flag through the TUI CLI and update the `session-harvest` SKILL.md to pass it.

**Tech Stack:** Rust (`uuid`, `serde_json`, `anyhow`, `clap`), Markdown.

## Global Constraints

- No new dependencies.
- `--all-segments` defaults OFF; session-surgery never passes it and its active-path behavior is preserved unchanged.
- A `--from/--to` range never crosses a compaction boundary — if `from` and `to` are in different trees, error.
- Reuse the existing `render_range_text` / `render_outline` per segment; only segment iteration + dividers are new.
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Modify** `crates/core/src/session/raw.rs` — add `pub fn segments` + tests.
- **Modify** `crates/core/src/session/surgery.rs` — `extract` gains `all_segments`; add `render_seg`/`resolve_range` helpers + the all-segments branch; update existing extract tests' calls; add all-segments tests.
- **Modify** `crates/tui/src/main.rs` — `Extract` subcommand gains `--all-segments`; handler passes it.
- **Modify** `claude-code-manager/skills/session-harvest/SKILL.md` — outline/drill-down pass `--all-segments`; tag candidates by segment; drop the v1 active-path-only note.

---

## Task 1: `raw::segments` — raw-line tree grouping (TDD)

**Files:**
- Modify: `crates/core/src/session/raw.rs`
- Test: inline `#[cfg(test)]` in `raw.rs`

**Interfaces:**
- Produces: `pub fn segments(lines: &[RawLine]) -> Vec<Vec<&RawLine>>` — one path per conversation tree, non-active first (file order) and the active tree last; single-tree sessions return one segment equal to `active_path`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/core/src/session/raw.rs` test module (it already has `line(uuid, parent, kind)`, `write_tmp`, and `read_raw`):

```rust
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
        // Active (global leaf d) segment is last.
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p claude-code-manager-core segments_`
Expected: FAIL to compile — `segments` not defined in `raw`.

- [ ] **Step 3: Implement `raw::segments`**

In `crates/core/src/session/raw.rs`, add after `active_path` (before the `#[cfg(test)]` module). `HashMap` is already imported (used by `path_to`):

```rust
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
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p claude-code-manager-core segments_`
Expected: PASS — `segments_single_tree_equals_active_path`, `segments_two_roots_active_last`, `segments_dangling_parent_starts_new_tree`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/session/raw.rs
git commit -m "feat(core): raw::segments — group raw lines into conversation trees

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `extract --all-segments` + CLI

**Files:**
- Modify: `crates/core/src/session/surgery.rs`
- Modify: `crates/tui/src/main.rs`
- Test: inline `#[cfg(test)]` in `surgery.rs`

**Interfaces:**
- Consumes: `raw::segments` (Task 1), existing `snap_range`, `render_range_text`, `render_outline`.
- Produces: `surgery::extract(path, from: Option<Uuid>, to: Option<Uuid>, outline: bool, all_segments: bool) -> Result<String>`.

- [ ] **Step 1: Replace `extract` with the all-segments-aware version + helpers**

In `crates/core/src/session/surgery.rs`, first add `segments` to the `raw` import (it currently imports `active_path, first_cwd, read_raw, RawLine`):

```rust
use super::raw::{active_path, first_cwd, read_raw, segments, RawLine};
```

Then replace the whole `pub fn extract` block with these three items:

```rust
fn render_seg(seg: &[&RawLine], a: usize, b: usize, outline: bool) -> String {
    if outline {
        render_outline(seg, a, b)
    } else {
        render_range_text(seg, a, b)
    }
}

/// Resolve a (possibly absent) range against a single path. Both absent = the
/// whole path; both present = turn-snapped; one present = error.
fn resolve_range(path: &[&RawLine], from: Option<Uuid>, to: Option<Uuid>) -> Result<(usize, usize)> {
    match (from, to) {
        (Some(f), Some(t)) => snap_range(path, f, t),
        (None, None) => {
            if path.is_empty() {
                return Err(anyhow!("session has no messages on the active path"));
            }
            Ok((0, path.len() - 1))
        }
        _ => Err(anyhow!("provide both --from and --to, or neither")),
    }
}

/// Read a session and render the full text or a compact outline. With
/// `all_segments`, covers every conversation tree (pre-compaction + active),
/// dividing whole-session output by segment; otherwise the active path only.
/// With both `from`/`to`, renders that turn-snapped range; with neither, the
/// whole path/session; with one, errors.
pub fn extract(
    path: &Path,
    from: Option<Uuid>,
    to: Option<Uuid>,
    outline: bool,
    all_segments: bool,
) -> Result<String> {
    let lines = read_raw(path)?;

    if !all_segments {
        let active = active_path(&lines);
        let (a, b) = resolve_range(&active, from, to)?;
        return Ok(render_seg(&active, a, b, outline));
    }

    let segs = segments(&lines);
    if segs.is_empty() {
        return Err(anyhow!("session has no messages"));
    }
    match (from, to) {
        (Some(f), Some(t)) => {
            // The range must live within a single tree.
            let seg = segs
                .iter()
                .find(|s| s.iter().any(|l| l.uuid == Some(f)))
                .ok_or_else(|| anyhow!("from id {} is not in any conversation tree", f))?;
            if !seg.iter().any(|l| l.uuid == Some(t)) {
                return Err(anyhow!("from and to are in different conversation trees"));
            }
            let (a, b) = snap_range(seg, f, t)?;
            Ok(render_seg(seg, a, b, outline))
        }
        (None, None) => {
            let n = segs.len();
            let mut out = String::new();
            for (k, seg) in segs.iter().enumerate() {
                let kind = if k + 1 == n { "active" } else { "pre-compaction" };
                out.push_str(&format!("── segment {}/{} · {} ──\n", k + 1, n, kind));
                let end = seg.len().saturating_sub(1);
                out.push_str(&render_seg(seg, 0, end, outline));
            }
            Ok(out)
        }
        _ => Err(anyhow!("provide both --from and --to, or neither")),
    }
}
```

- [ ] **Step 2: Update the existing extract tests to the new arity**

In `surgery.rs` tests, the four `extract(...)` calls added previously each gain a trailing `false` (active-path mode). Update them:

- `extract_renders_range_with_roles`: `extract(&p, Some(a), Some(c), false, false)`
- `extract_whole_path_when_no_range`: `extract(&p, None, None, false, false)`
- `extract_rejects_one_sided_range`: `extract(&p, Some(a), None, false, false)`
- `extract_outline_is_compact`: `extract(&p, None, None, true, false)`

- [ ] **Step 3: Add all-segments tests + a two-tree fixture**

Add to the `surgery.rs` test module:

```rust
    fn two_tree_fixture() -> std::path::PathBuf {
        // Pre-compaction tree (a1->b2), then a fresh null-parent root (c3->d4).
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"PRE1"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"assistant","content":[{"type":"text","text":"PRE2"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000c3","parentUuid":null,"message":{"role":"user","content":"ACT1"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-0000-0000-0000000000d4","parentUuid":"00000000-0000-0000-0000-0000000000c3","message":{"role":"assistant","content":[{"type":"text","text":"ACT2"}]}}"#, "\n",
        );
        let mut p = std::env::temp_dir();
        p.push(format!("cm-surg-twotree-{}.jsonl", std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn all_segments_outline_shows_both_trees_with_dividers() {
        let p = two_tree_fixture();
        let text = extract(&p, None, None, true, true).unwrap();
        assert!(text.contains("── segment 1/2 · pre-compaction ──"));
        assert!(text.contains("── segment 2/2 · active ──"));
        assert!(text.contains("PRE1"), "pre-compaction tree's turns appear");
        assert!(text.contains("ACT1"), "active tree's turns appear");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn all_segments_range_within_precompaction_tree() {
        let p = two_tree_fixture();
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        let b = "00000000-0000-0000-0000-0000000000b2".parse().unwrap();
        let text = extract(&p, Some(a), Some(b), false, true).unwrap();
        assert!(text.contains("PRE1"));
        assert!(text.contains("PRE2"));
        assert!(!text.contains("ACT"), "range stays within the pre-compaction tree");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn all_segments_range_across_trees_errors() {
        let p = two_tree_fixture();
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        let d = "00000000-0000-0000-0000-0000000000d4".parse().unwrap();
        let err = extract(&p, Some(a), Some(d), false, true).unwrap_err().to_string();
        assert!(err.contains("different conversation trees"), "got: {err}");
        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 4: Update the TUI CLI**

In `crates/tui/src/main.rs`, add `--all-segments` to the `Extract` subcommand:

```rust
        /// Compact map: one line per turn (ids + tool-call one-liners), no outputs.
        #[arg(long)]
        outline: bool,
        /// Cover ALL conversation trees (pre-compaction + active), not just the active path.
        #[arg(long)]
        all_segments: bool,
    },
```

…and update the handler arm to pass it (replace the `print!` line and add `all_segments` to the destructure):

```rust
            SurgeryOp::Extract { session, from, to, outline, all_segments } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let a = from
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .context("invalid --from uuid")?;
                let b = to
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .context("invalid --to uuid")?;
                print!("{}", surgery::extract(&path, a, b, *outline, *all_segments)?);
            }
```

- [ ] **Step 5: Build + test**

Run: `cargo test -p claude-code-manager-core surgery && cargo build -p claude-code-manager-tui`
Expected: all surgery tests pass (4 updated + 3 new all-segments); TUI compiles with the new arity.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/session/surgery.rs crates/tui/src/main.rs
git commit -m "feat(core): surgery extract --all-segments (mine pre-compaction trees)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Update the `session-harvest` skill

**Files:**
- Modify: `claude-code-manager/skills/session-harvest/SKILL.md`

- [ ] **Step 1: Make the navigation commands pass `--all-segments`**

In `/Users/you/dev/claude-code-manager/skills/session-harvest/SKILL.md`, change the outline command in the "Navigation" section from:

```bash
   "$BIN" surgery extract --session "$SESSION" --outline
```
to:
```bash
   "$BIN" surgery extract --session "$SESSION" --outline --all-segments
```

and the drill-down command from:

```bash
   "$BIN" surgery extract --session "$SESSION" --from "$FROM" --to "$TO"
```
to:
```bash
   "$BIN" surgery extract --session "$SESSION" --from "$FROM" --to "$TO" --all-segments
```

- [ ] **Step 2: Note segments in navigation + discovery; drop the v1 limitation framing**

In the same file, in the "Navigation" section step 1 (Outline), append after the existing sentence about the table of contents:

```markdown
   The outline covers **every conversation tree** in the file — a compacted
   session shows `── segment k/N · pre-compaction|active ──` dividers between its
   trees, so pre-compaction work is harvestable too. A `--from/--to` range must
   stay within one tree (the binary errors if it spans a compaction boundary).
```

And in the "Discovery output" section, change the candidate template's `span` line to also note the segment:

```
   span: <from-uuid> .. <to-uuid>   (seg <k>/<N> · <pre-compaction|active>, turns #x–#y)
```

- [ ] **Step 3: Sanity-check + commit**

Run: `grep -c 'all-segments' /Users/you/dev/claude-code-manager/skills/session-harvest/SKILL.md`
Expected: `2` (both navigation commands updated).

```bash
git add skills/session-harvest/SKILL.md
git commit -m "docs(skills): session-harvest reads all segments (pre-compaction trees)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS — core incl. 3 new `raw::segments` tests + 3 new all-segments extract tests + the 4 updated extract tests; webui; TUI. 0 failures.

- [ ] **Step 2: Real-session smoke (proves all-segments on real data)**

Build (`cargo build --release`), then on a compacted session whose active path is short (e.g. `91052458-9e2f-4cb9-80dc-afe3b9ce904d`):

```bash
BIN=/Users/you/dev/claude-code-manager/target/release/claude-code-manager
S=91052458-9e2f-4cb9-80dc-afe3b9ce904d
echo "active-path-only outline turns:";  "$BIN" surgery extract --session "$S" --outline | wc -l
echo "all-segments outline turns:";      "$BIN" surgery extract --session "$S" --outline --all-segments | wc -l
"$BIN" surgery extract --session "$S" --outline --all-segments | grep -c '── segment'
```
Expected: the all-segments outline has MORE turns than active-path-only (it now includes the pre-compaction tree), and `grep -c '── segment'` is ≥ 1 (dividers present).

- [ ] **Step 3: Manual harvest exercise (hand to the user)**

Have the user run, in a session where the skill is available: "what can be reused from session `<a compacted session>`?" → expect candidates drawn from pre-compaction spans, each tagged with its segment.

- [ ] **Step 4: Report**

Report exact `cargo test` counts and the smoke output (active-path turns vs all-segments turns, divider count). Do not claim the manual exercise works without the user's confirmation. Do not merge — the user directs merges.

---

## Notes for the implementer

- `--all-segments` is OFF by default; session-surgery never passes it, so its active-path behavior is unchanged — the 4 updated extract tests (all passing `false`) prove this.
- A range that spans two trees is an error, not a silent merge.
- `raw::segments` is a faithful mirror of `tree::segments` (same ordering: non-active first in file order, active last); the only difference is `RawLine.parent` vs `Event::parent_uuid()` and `path_to` vs `current_path`.
- Per-segment outline turn indices restart at `#0`; that's fine — candidates are cited by uuid, not index.
