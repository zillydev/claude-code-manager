# Sharpen Harvest Tooling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `surgery extract` easier to drive from `session-harvest` — short copyable ids + uuid-prefix `--from/--to`, a `· compaction summary` tag in the outline, and segment-aware range errors.

**Architecture:** All in `crates/core/src/session/surgery.rs` + the `Extract` CLI in `crates/tui/src/main.rs`. `extract` takes `from/to` as `Option<&str>` and resolves a uuid prefix to a unique full uuid (`resolve_id`); cross-tree / off-active-path ranges produce segment-aware errors. `render_outline` shows an 8-char short id and tags `isCompactSummary` turns.

**Tech Stack:** Rust (`uuid`, `serde_json`, `anyhow`, `clap`).

## Global Constraints

- All changes in `surgery.rs` + the `Extract` CLI handler; `extract` is called only there and in surgery's tests (`webui` uses `surgery::rewrite`). No webui change.
- 8-char short id = the uuid's first hex group (`uuid.to_string()[..8]`); effectively unique within a session.
- `resolve_id` matches case-insensitively; a full 36-char uuid is a valid prefix of itself.
- Full (non-outline) extract keeps full uuids in its `[role uuid]` annotation (detail/citation view).
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Modify** `crates/core/src/session/surgery.rs` — add `resolve_id` + `short8`; change `extract` signature to `Option<&str>` + prefix resolution + segment-aware errors + active-path hint; `render_outline` short id + summary tag; update/add tests.
- **Modify** `crates/tui/src/main.rs` — `Extract` handler passes `from.as_deref()/to.as_deref()` (no `Uuid::parse_str`).
- **Modify** `skills/session-harvest/SKILL.md` — note short ids / prefix ranges / the summary tag.

---

## Task 1: Prefix range targeting + segment-aware errors

**Files:**
- Modify: `crates/core/src/session/surgery.rs` (add `resolve_id`/`short8`, rewrite `extract`, update existing extract test calls, add tests)
- Modify: `crates/tui/src/main.rs` (handler)

**Interfaces:**
- Produces: `extract(path: &Path, from: Option<&str>, to: Option<&str>, outline: bool, all_segments: bool) -> Result<String>`; private `resolve_id(lines: &[RawLine], prefix: &str) -> Result<Uuid>`, `short8(u: Uuid) -> String`.

- [ ] **Step 1: Add `resolve_id` + `short8` helpers**

In `crates/core/src/session/surgery.rs`, add just above `pub fn extract`:

```rust
/// The 8-char short id (first hex group) of a uuid — what the outline shows and
/// what `--from/--to` prefixes are typically copied from.
fn short8(u: Uuid) -> String {
    u.to_string()[..8].to_string()
}

/// Resolve a uuid prefix (case-insensitive) to the single matching message uuid
/// in `lines`. A full 36-char uuid is a prefix of itself. Errors if the prefix
/// matches none or more than one message.
fn resolve_id(lines: &[RawLine], prefix: &str) -> Result<Uuid> {
    let p = prefix.to_ascii_lowercase();
    let mut hits: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    for l in lines {
        if let Some(u) = l.uuid {
            if u.to_string().starts_with(&p) {
                hits.insert(u);
            }
        }
    }
    match hits.len() {
        1 => Ok(*hits.iter().next().unwrap()),
        0 => Err(anyhow!("no message id matches '{}'", prefix)),
        n => Err(anyhow!(
            "ambiguous id prefix '{}' matches {} messages — use more characters",
            prefix,
            n
        )),
    }
}
```

- [ ] **Step 2: Rewrite `extract` to resolve prefixes + segment-aware errors**

Replace the whole `pub fn extract` block (the current `Option<Uuid>` version) with:

```rust
/// Read a session and render the full text or a compact outline. `from`/`to` are
/// uuid PREFIXES (resolved to a unique message; a full uuid works too). With
/// `all_segments`, covers every conversation tree, dividing whole-session output
/// by segment; otherwise the active path only. Both `from`/`to` = turn-snapped
/// range; neither = whole path/session; one = error.
pub fn extract(
    path: &Path,
    from: Option<&str>,
    to: Option<&str>,
    outline: bool,
    all_segments: bool,
) -> Result<String> {
    let lines = read_raw(path)?;
    let from_id = from.map(|p| resolve_id(&lines, p)).transpose()?;
    let to_id = to.map(|p| resolve_id(&lines, p)).transpose()?;

    if !all_segments {
        let active = active_path(&lines);
        // A resolved id that exists in the file but not on the active path is in
        // an earlier (pre-compaction) tree — point the caller at --all-segments.
        for (label, id) in [("from", from_id), ("to", to_id)] {
            if let Some(u) = id {
                if !active.iter().any(|l| l.uuid == Some(u)) {
                    return Err(anyhow!(
                        "{} id {} is not on the active path — it's in an earlier (pre-compaction) tree; pass --all-segments to reach it",
                        label,
                        u
                    ));
                }
            }
        }
        let (a, b) = resolve_range(&active, from_id, to_id)?;
        return Ok(render_seg(&active, a, b, outline));
    }

    let segs = segments(&lines);
    if segs.is_empty() {
        return Err(anyhow!("session has no messages"));
    }
    match (from_id, to_id) {
        (Some(f), Some(t)) => {
            let seg_of = |u: Uuid| segs.iter().position(|s| s.iter().any(|l| l.uuid == Some(u)));
            let fi = seg_of(f).ok_or_else(|| anyhow!("from id {} is not in any conversation tree", f))?;
            let ti = seg_of(t).ok_or_else(|| anyhow!("to id {} is not in any conversation tree", t))?;
            if fi != ti {
                let n = segs.len();
                return Err(anyhow!(
                    "from {} is in segment {}/{}, to {} is in segment {}/{} — a range can't cross a compaction boundary",
                    short8(f), fi + 1, n, short8(t), ti + 1, n
                ));
            }
            let (a, b) = snap_range(&segs[fi], f, t)?;
            Ok(render_seg(&segs[fi], a, b, outline))
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

- [ ] **Step 3: Update the CLI handler**

In `crates/tui/src/main.rs`, replace the `Extract` handler arm body so it passes the raw strings (drop the `Uuid::parse_str` resolution):

```rust
            SurgeryOp::Extract { session, from, to, outline, all_segments } => {
                let path = surgery::resolve_session_path(&root, session)?;
                print!(
                    "{}",
                    surgery::extract(&path, from.as_deref(), to.as_deref(), *outline, *all_segments)?
                );
            }
```

(If the `use uuid::Uuid;` import in `main.rs` is now unused, leave it if other arms use it — the `Rewrite` arm still parses uuids, so it stays.)

- [ ] **Step 4: Update existing extract test call sites to the `&str` signature**

In `surgery.rs` tests, the calls currently pass parsed `Uuid`s. Change each affected test's `let x = "…".parse().unwrap();` to a plain `&str` and pass it. Specifically:

- `extract_renders_range_with_roles`: change `let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();` → `let a = "00000000-0000-0000-0000-0000000000a1";` and `let c = "00000000-0000-0000-0000-0000000000c4".parse().unwrap();` → `let c = "00000000-0000-0000-0000-0000000000c4";`. Call stays `extract(&p, Some(a), Some(c), false, false)`.
- `extract_rejects_one_sided_range`: `let a = "00000000-0000-0000-0000-0000000000a1";` (drop `.parse().unwrap()`). Call stays `extract(&p, Some(a), None, false, false)`.
- `all_segments_range_within_precompaction_tree`: `let a = "00000000-0000-0000-0000-0000000000a1";` and `let b = "00000000-0000-0000-0000-0000000000b2";` (drop parses). Call stays `extract(&p, Some(a), Some(b), false, true)`.
- `all_segments_range_across_trees_errors`: `let a = "00000000-0000-0000-0000-0000000000a1";` and `let d = "00000000-0000-0000-0000-0000000000d4";` (drop parses). Then update its assertion (these uuids are in different trees) to the new error text:

```rust
        let err = extract(&p, Some(a), Some(d), false, true).unwrap_err().to_string();
        assert!(err.contains("segment 1/2") && err.contains("compaction boundary"), "got: {err}");
```

(The `None, None` and outline calls — `extract(&p, None, None, …)` — need no change.)

- [ ] **Step 5: Add tests for the resolver, prefix range, and hint**

Add to the `surgery.rs` test module:

```rust
    fn idfix() -> PathBuf {
        // Three uuids with DISTINCT leading hex so prefixes are meaningful:
        // aaaaaaaa…1, bbbbbbbb…2, aaaaaaab…3 (shares "aaaaaaa" with the first).
        let body = concat!(
            r#"{"type":"user","uuid":"aaaaaaaa-0000-0000-0000-000000000001","parentUuid":null,"message":{"role":"user","content":"x"}}"#, "\n",
            r#"{"type":"user","uuid":"bbbbbbbb-0000-0000-0000-000000000002","parentUuid":"aaaaaaaa-0000-0000-0000-000000000001","message":{"role":"user","content":"y"}}"#, "\n",
            r#"{"type":"user","uuid":"aaaaaaab-0000-0000-0000-000000000003","parentUuid":"bbbbbbbb-0000-0000-0000-000000000002","message":{"role":"user","content":"z"}}"#, "\n",
        );
        write_tmp("idfix", body)
    }

    #[test]
    fn resolve_id_prefix_unique_ambiguous_none() {
        let p = idfix();
        let lines = read_raw(&p).unwrap();
        assert_eq!(
            resolve_id(&lines, "bbbbbbbb").unwrap().to_string(),
            "bbbbbbbb-0000-0000-0000-000000000002"
        );
        assert_eq!(
            resolve_id(&lines, "aaaaaaaa").unwrap().to_string(),
            "aaaaaaaa-0000-0000-0000-000000000001"
        );
        assert!(resolve_id(&lines, "aaaaaaa").is_err(), "ambiguous (aaaaaaaa + aaaaaaab)");
        assert!(resolve_id(&lines, "zzzz").is_err(), "no match");
        assert_eq!(
            resolve_id(&lines, "aaaaaaab-0000-0000-0000-000000000003").unwrap().to_string(),
            "aaaaaaab-0000-0000-0000-000000000003"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_accepts_uuid_prefix_range() {
        let p = idfix();
        let text = extract(&p, Some("aaaaaaaa"), Some("bbbbbbbb"), false, false).unwrap();
        assert!(text.contains("x") && text.contains("y"));
        assert!(!text.contains("z"), "range stops at the 'to' turn");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_hints_all_segments_for_precompaction_id() {
        let p = two_tree_fixture();
        let a = "00000000-0000-0000-0000-0000000000a1"; // in the pre-compaction tree
        let err = extract(&p, Some(a), Some(a), false, false).unwrap_err().to_string();
        assert!(err.contains("pass --all-segments"), "got: {err}");
        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 6: Build + test**

Run: `cargo test -p claude-code-manager-core surgery && cargo build -p claude-code-manager-tui`
Expected: all surgery tests pass (updated + 3 new); TUI compiles with the `&str` `extract`.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/session/surgery.rs crates/tui/src/main.rs
git commit -m "feat(core): surgery extract resolves uuid-prefix ranges + segment-aware errors

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Outline short ids + compaction-summary tag

**Files:**
- Modify: `crates/core/src/session/surgery.rs` (`render_outline` + tests)

- [ ] **Step 1: Short id + summary tag in the header**

In `render_outline`, replace the uuid line and the header push. Change:

```rust
        let uuid = head.uuid.map(|u| u.to_string()).unwrap_or_default();
```

to:

```rust
        let short = head.uuid.map(short8).unwrap_or_default();
        let is_summary =
            head.value.get("isCompactSummary").and_then(|v| v.as_bool()).unwrap_or(false);
```

and change the header push:

```rust
        out.push_str(&format!("#{} {} {}\n", shown, role, uuid));
```

to:

```rust
        let tag = if is_summary { " · compaction summary" } else { "" };
        out.push_str(&format!("#{} {} {}{}\n", shown, role, short, tag));
```

- [ ] **Step 2: Update the outline test to expect the short id**

In `extract_outline_is_compact`, the header assertion currently expects the full uuid. Change:

```rust
        assert!(text.contains("#0 user 00000000-0000-0000-0000-0000000000a1"));
```

to (the short id, terminated by newline so it can't match a longer string):

```rust
        assert!(text.contains("#0 user 00000000\n"));
```

- [ ] **Step 3: Add a compaction-summary-tag test**

Add to the `surgery.rs` test module:

```rust
    #[test]
    fn outline_tags_compaction_summary() {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"isCompactSummary":true,"message":{"role":"user","content":"This session is being continued from a previous conversation…"}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"user","content":"hello"}}"#, "\n",
        );
        let p = write_tmp("summarytag", body);
        let text = extract(&p, None, None, true, false).unwrap();
        assert!(text.contains("· compaction summary"), "isCompactSummary turn is tagged");
        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 4: Build + test**

Run: `cargo test -p claude-code-manager-core surgery`
Expected: PASS — `outline_tags_compaction_summary` + the updated `extract_outline_is_compact` + the rest.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/session/surgery.rs
git commit -m "feat(core): outline shows short ids + tags compaction-summary turns

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Skill doc touch-up

**Files:**
- Modify: `skills/session-harvest/SKILL.md`

- [ ] **Step 1: Document short ids / prefix ranges / the summary tag**

In `/Users/zillydev/my/Dev/Personal/claude-manager/skills/session-harvest/SKILL.md`, in the "Navigation" section, after the outline-command paragraph (the one describing the `── segment k/N ──` dividers), add:

```markdown
   Each outline turn header is `#<n> <role> <short-id>` where `<short-id>` is the
   first 8 chars of the message uuid — copy that into `--from/--to`, which accept
   any uuid prefix (a full uuid works too). A turn tagged ` · compaction summary`
   is the "continued from a previous conversation" recap — the densest, most
   useful turn to read first in a compacted session.
```

- [ ] **Step 2: Commit**

```bash
git add skills/session-harvest/SKILL.md
git commit -m "docs(skills): session-harvest notes short ids, prefix ranges, summary tag

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS (core surgery incl. the new resolver/prefix/hint/summary-tag tests; webui; TUI), 0 failures.

- [ ] **Step 2: Real-session smoke**

Build (`cargo build --release`), then on a compacted session (e.g. `91052458-9e2f-4cb9-80dc-afe3b9ce904d`):

```bash
BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-code-manager
S=91052458-9e2f-4cb9-80dc-afe3b9ce904d
# Outline: short ids + a compaction-summary tag
"$BIN" surgery extract --session "$S" --outline --all-segments | grep -E '· compaction summary' | head -2
"$BIN" surgery extract --session "$S" --outline --all-segments | grep -oE '^#[0-9]+ [a-z]+ [0-9a-f]{8} ' | head -3
# Drill in with an 8-char prefix copied from the outline (pick a real one):
SHORT=$("$BIN" surgery extract --session "$S" --outline --all-segments | grep -oE '[0-9a-f]{8}' | sed -n '3p')
"$BIN" surgery extract --session "$S" --from "$SHORT" --to "$SHORT" --all-segments | head -3
```
Expected: outline headers show `#N role <8hex>`; at least one ` · compaction summary` line; the 8-char-prefix drill resolves and prints that turn (no "not in any conversation tree").

- [ ] **Step 3: Report**

Report exact `cargo test` counts and the smoke output. Do not merge — the user directs merges.

---

## Notes for the implementer

- `extract` resolves prefixes against ALL file uuids (regardless of `--all-segments`), so a prefix resolves first, then the segment/active-path logic + hints apply.
- `short8` slices `uuid.to_string()[..8]` — uuids are always 36 ASCII chars, so this is safe.
- The full (non-outline) `render_range_text` keeps full uuids in `[role uuid]` — do NOT shorten those.
- The `resolve_id`/`extract`/`short8` symbols are private; the surgery test module reaches them via `use super::*` (already present — existing tests call `extract`/`snap_range` unqualified).
