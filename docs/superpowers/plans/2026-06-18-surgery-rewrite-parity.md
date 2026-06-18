# Surgery Rewrite Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `surgery::rewrite` accept uuid **prefixes** (via the shared `resolve_id`) and give a clear off-active-path error, matching `surgery::extract`, then refresh `session-surgery/SKILL.md`.

**Architecture:** Change `rewrite`'s `from`/`to` from `Uuid` to `&str`; it already `read_raw`s, so resolve each prefix to a uuid up front, reject ids that aren't on the active path (rewrite is active-path-only), then run the existing `snap_range` + commit pipeline. Update the CLI handler, the webui `delete_message`, and the 7 existing rewrite tests to the `&str` signature.

**Tech Stack:** Rust.

## Global Constraints

- `rewrite` is active-path-only — its off-path error does NOT suggest `--all-segments` (no such flag for rewrite).
- Reuse `resolve_id` unchanged (uuid prefix → unique uuid; full uuid works).
- A full uuid string is a valid prefix of itself, so existing callers passing full uuids keep working.
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Modify** `crates/core/src/session/surgery.rs` — `rewrite` signature + prefix resolution + off-path error; update 7 rewrite tests; add 2 tests.
- **Modify** `crates/tui/src/main.rs` — `Rewrite` handler drops `Uuid::parse_str`.
- **Modify** `crates/webui/src/commands.rs` — `delete_message` drops `Uuid::parse_str`.
- **Modify** `skills/session-surgery/SKILL.md` — note prefix ids / elided preview / errors.

---

## Task 1: `rewrite` resolves prefixes + off-path error (+ all callers & tests)

**Files:**
- Modify: `crates/core/src/session/surgery.rs`, `crates/tui/src/main.rs`, `crates/webui/src/commands.rs`

**Interfaces:**
- Produces: `rewrite(root: &Path, path: &Path, from: &str, to: &str, mode: RewriteMode, summary: Option<&str>) -> Result<RewriteOutcome>`.
- Consumes: `resolve_id(&[RawLine], &str) -> Result<Uuid>` (already in `surgery.rs`).

- [ ] **Step 1: Change `rewrite` signature + resolve prefixes + off-path check**

In `crates/core/src/session/surgery.rs`, change the `rewrite` head. Replace:

```rust
pub fn rewrite(
    root: &Path,
    path: &Path,
    from: Uuid,
    to: Uuid,
    mode: RewriteMode,
    summary: Option<&str>,
) -> Result<RewriteOutcome> {
    let lines = read_raw(path)?;
    let active = active_path(&lines);
    let (a, b) = snap_range(&active, from, to)?;
```

with:

```rust
pub fn rewrite(
    root: &Path,
    path: &Path,
    from: &str,
    to: &str,
    mode: RewriteMode,
    summary: Option<&str>,
) -> Result<RewriteOutcome> {
    let lines = read_raw(path)?;
    let from_id = resolve_id(&lines, from)?;
    let to_id = resolve_id(&lines, to)?;
    let active = active_path(&lines);
    for (label, id) in [("from", from_id), ("to", to_id)] {
        if !active.iter().any(|l| l.uuid == Some(id)) {
            return Err(anyhow!(
                "{} id {} is not on the active path — surgery rewrites the active conversation only",
                label,
                id
            ));
        }
    }
    let (a, b) = snap_range(&active, from_id, to_id)?;
```

(The rest of the function body is unchanged — it uses `a`/`b`, not `from`/`to`.)

- [ ] **Step 2: Update the CLI `Rewrite` handler**

In `crates/tui/src/main.rs`, replace the `Rewrite` arm's id handling. Change:

```rust
            SurgeryOp::Rewrite { session, from, to, mode, summary_file } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let a = Uuid::parse_str(from).context("invalid --from uuid")?;
                let b = Uuid::parse_str(to).context("invalid --to uuid")?;
                let m = match mode.as_str() {
```

to:

```rust
            SurgeryOp::Rewrite { session, from, to, mode, summary_file } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let m = match mode.as_str() {
```

and change the `rewrite` call:

```rust
                let out = surgery::rewrite(&root, &path, a, b, m, summary_text.as_deref())?;
```

to:

```rust
                let out = surgery::rewrite(&root, &path, from, to, m, summary_text.as_deref())?;
```

(`from`/`to` are `&String`, which coerce to `&str`. If `use uuid::Uuid;` at the top of `main.rs` is now unused, the build will warn but not fail — leave it; it's used by other code paths only if present, otherwise remove the `use uuid::Uuid;` line to silence the warning.)

- [ ] **Step 3: Update webui `delete_message`**

In `crates/webui/src/commands.rs`, replace the body of `delete_message`. Change:

```rust
    let path = PathBuf::from(&file_path);
    let id = uuid::Uuid::parse_str(&message_uuid)
        .map_err(|e| format!("invalid message uuid: {e}"))?;
    let out = rewrite(&root, &path, id, id, RewriteMode::Prune, None).map_err(|e| e.to_string())?;
    Ok(out.commit)
```

to:

```rust
    let path = PathBuf::from(&file_path);
    let out = rewrite(&root, &path, &message_uuid, &message_uuid, RewriteMode::Prune, None)
        .map_err(|e| e.to_string())?;
    Ok(out.commit)
```

(If this removal makes the `uuid` crate unused elsewhere in `commands.rs`, the build warns only — check at Step 6 and drop any now-unused `use`.)

- [ ] **Step 4: Update the 7 existing rewrite test calls to `&str`**

In `crates/core/src/session/surgery.rs` tests, each rewrite test defines its id as a parsed `Uuid` then passes it to `rewrite`. Change each `let <var> = "<uuid>".parse().unwrap();` to `let <var> = "<uuid>";` (drop `.parse().unwrap()`). The id is only ever passed to `rewrite` in these tests, so no other use breaks. (Several `b` lines are textually identical — disambiguate each Edit with its surrounding line, e.g. the comment or the `rewrite(...)` call that follows.) The tests and their id vars:

  1. `prune_drops_the_range_and_revalidates_clean` — `let b = "00000000-0000-0000-0000-0000000000b2";`
  2. `summary_message_has_timestamp` — `let b = "00000000-0000-0000-0000-0000000000b2";`
  3. `summary_inserts_one_message` — `let b = "00000000-0000-0000-0000-0000000000b2";`
  4. `summary_requires_text` — `let a = "00000000-0000-0000-0000-0000000000a1";`
  5. `prune_keeps_tool_pairs_intact_no_repairs` — `let b = "00000000-0000-0000-0000-0000000000b2";`
  6. `prune_from_head_leaving_leading_assistant_is_refused` — `let a = "00000000-0000-0000-0000-0000000000a1";`
  7. `rewrite_files_new_session_under_launch_cwd_not_active_path_cwd` — `let d4 = "00000000-0000-0000-0000-0000000000d4";`

Do NOT touch `let a = "…a1".parse().unwrap();` inside the `snap_range`-only test or any `extract` test — those are separate; only the 7 rewrite tests above change.

- [ ] **Step 5: Add the prefix + off-path tests**

Add to the `surgery.rs` test module:

```rust
    #[test]
    fn rewrite_accepts_uuid_prefix() {
        // Distinct leading hex so an 8-char prefix is unambiguous.
        let body = concat!(
            r#"{"type":"user","uuid":"aaaaaaaa-0000-0000-0000-000000000001","parentUuid":null,"message":{"role":"user","content":"A"}}"#, "\n",
            r#"{"type":"assistant","uuid":"bbbbbbbb-0000-0000-0000-000000000002","parentUuid":"aaaaaaaa-0000-0000-0000-000000000001","message":{"role":"assistant","content":[{"type":"text","text":"B"}]}}"#, "\n",
            r#"{"type":"user","uuid":"cccccccc-0000-0000-0000-000000000003","parentUuid":"bbbbbbbb-0000-0000-0000-000000000002","message":{"role":"user","content":"C"}}"#, "\n",
        );
        let p = write_tmp("rwprefix", body);
        let root = std::env::temp_dir().join(format!("cm-surg-root-rwpfx-{}", std::process::id()));
        // Prune the assistant turn by an 8-char prefix; a full uuid would also work.
        let out = rewrite(&root, &p, "bbbbbbbb", "bbbbbbbb", RewriteMode::Prune, None).unwrap();
        assert_eq!(out.messages_after, 2, "assistant turn pruned; A + C remain");
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rewrite_rejects_off_active_path_id() {
        let p = two_tree_fixture();
        let root = std::env::temp_dir().join(format!("cm-surg-root-offpath-{}", std::process::id()));
        let a = "00000000-0000-0000-0000-0000000000a1"; // in the pre-compaction tree
        let err = rewrite(&root, &p, a, a, RewriteMode::Prune, None).unwrap_err().to_string();
        assert!(err.contains("not on the active path"), "got: {err}");
        assert!(err.contains("active conversation only"), "got: {err}");
        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&root).ok();
    }
```

- [ ] **Step 6: Build all three crates + run core tests**

Run: `cargo build && cargo test -p claude-manager-core surgery`
Expected: workspace compiles (core, tui, webui); surgery tests pass — the 7 updated rewrite tests, `rewrite_accepts_uuid_prefix`, `rewrite_rejects_off_active_path_id`, and all others. Fix any now-unused `use uuid::Uuid;` warnings flagged by the build (remove the import in that file).

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/session/surgery.rs crates/tui/src/main.rs crates/webui/src/commands.rs
git commit -m "feat(core): surgery rewrite accepts uuid-prefix ids + active-path error

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Refresh `session-surgery/SKILL.md`

**Files:**
- Modify: `skills/session-surgery/SKILL.md`

- [ ] **Step 1: Read the skill and find where it describes copying the two message ids**

Run: `grep -nE 'id|uuid|--from|--to|extract|rewrite' skills/session-surgery/SKILL.md` to locate the step that tells the user to copy/pass the two message ids.

- [ ] **Step 2: Add a note about prefix ids, the elided preview, and errors**

Near that step, add (adapt wording to the surrounding house style):

```markdown
`--from/--to` (on both `extract` and `rewrite`) accept a uuid **prefix** — copy
the short id from the webui's copy-id button or the `extract` preview header;
the full uuid works too. The `extract` preview annotates each turn's uuid and
elides embedded skill prompts (the `Base directory for this skill:` dumps).
Errors are explicit: `no message id matches '<x>'`, `ambiguous id prefix '<x>'
matches N messages — use more characters`, and `… is not on the active path —
surgery rewrites the active conversation only`.
```

- [ ] **Step 3: Commit**

```bash
git add skills/session-surgery/SKILL.md
git commit -m "docs(skills): session-surgery notes prefix ids, elided preview, clearer errors

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS (core incl. the 2 new rewrite tests; webui; TUI), 0 failures.

- [ ] **Step 2: Real-session smoke (preview + rewrite share one short id)**

Build (`cargo build --release`). On a session, copy ONE 8-char short id from the outline and use it for both the preview and a prune:

```bash
BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-manager
S=c84ba790-92b6-41ea-881f-971902858ce2
SHORT=$("$BIN" surgery extract --session "$S" --outline --all-segments | grep -oE '^#[0-9]+ [a-z]+ [0-9a-f]{8}$' | awk 'NR==5{print $3}')
echo "using short id: $SHORT"
"$BIN" surgery extract --session "$S" --from "$SHORT" --to "$SHORT" | head -3        # preview accepts prefix
"$BIN" surgery rewrite --session "$S" --from "$SHORT" --to "$SHORT" --mode prune     # rewrite accepts the SAME prefix
# Cleanup: the rewrite wrote a NEW session file (copy-on-edit); delete it.
```
Expected: the preview prints the turn; the rewrite prints `new session: …` / `path: …` (or a clear `not on the active path` if that short id is pre-compaction — both are correct behavior). Delete the new session file the rewrite created (it's a throwaway smoke artifact).

- [ ] **Step 3: Report**

Report exact `cargo test` counts and the smoke output, and note/delete the throwaway session the rewrite smoke created. Do not merge — the user directs merges.

---

## Notes for the implementer

- `rewrite`'s body past `snap_range` is untouched — only id resolution + the new off-path guard are added.
- Resolution scope is all file uuids (via `resolve_id`); the active-path guard then distinguishes "off the active path" from "unknown id".
- The rewrite smoke (Task 3 Step 2) actually writes a new session file (copy-on-edit) — delete it afterward so it doesn't linger in `~/.claude/projects`.
