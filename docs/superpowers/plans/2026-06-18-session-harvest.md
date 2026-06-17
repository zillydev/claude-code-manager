# Session Harvest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A read-only `session-harvest` skill that mines a past Claude Code session for reusable artifacts (skills, kb entries, summaries, scripts), backed by an extended `surgery extract` reader (whole-session default, uuid-annotated, plus a compact `--outline` map).

**Architecture:** Two parts. (1) Core/CLI: extend `surgery::extract` to take an optional range (defaulting to the whole active path), annotate each emitted turn with its message uuid, and add an `--outline` mode that prints a one-line-per-turn map (ids + tool-call one-liners, no outputs). (2) A new `claude-manager/skills/session-harvest/SKILL.md` workflow that navigates outline-first → drills down → fans out to subagents for large sessions, and produces artifacts by delegating to the `writing-skills` and `kb` skills (and inline for summaries/scripts), confirming every file write.

**Tech Stack:** Rust (`uuid`, `serde_json`, `anyhow`, `clap`), Markdown skill authoring.

## Global Constraints

- No new dependencies.
- Read-only on sessions: harvest never rewrites/touches a session file; the only writes are NEW artifact files (skills/kb/scripts) or rendered text (summaries), each confirmed with the user before writing.
- Reuse the binary's `surgery extract` reader; never hand-parse jsonl in the skill; never use the `rewrite` path.
- Artifact destinations: skill → `~/.claude/skills/` or `claude-manager/skills/` (confirm); kb → via `kb` skill (Obsidian); summary → render in chat + offer to save; script → propose path + confirm. "Runbook" is a kb entry, not a separate type.
- `--outline` / whole-session reads operate on the **active path** only (pre-compaction trees out of scope for v1).
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Modify** `crates/core/src/session/surgery.rs` — id-annotate `render_range_text`; add `render_outline`; change `extract` signature (optional range + `outline` flag); update its tests.
- **Modify** `crates/tui/src/main.rs` — `Extract` subcommand: optional `--from`/`--to`, new `--outline` flag; updated handler.
- **Create** `claude-manager/skills/session-harvest/SKILL.md` — the skill workflow.

---

## Task 1: Core + CLI — extend `surgery extract`

**Files:**
- Modify: `crates/core/src/session/surgery.rs`
- Modify: `crates/tui/src/main.rs`
- Test: inline `#[cfg(test)]` in `surgery.rs`

**Interfaces:**
- Produces: `surgery::extract(path: &Path, from: Option<Uuid>, to: Option<Uuid>, outline: bool) -> Result<String>` (replaces the 3-arg version); `render_outline(active, a_start, b_end) -> String` (private).

- [ ] **Step 1: Id-annotate the full extract**

In `crates/core/src/session/surgery.rs`, in `render_range_text`, replace the role header line:

```rust
        out.push_str(&format!("[{}]\n", role));
```

with one that includes the line's message uuid:

```rust
        let uuid = l.uuid.map(|u| u.to_string()).unwrap_or_default();
        out.push_str(&format!("[{} {}]\n", role, uuid));
```

- [ ] **Step 2: Add `render_outline`**

In `surgery.rs`, add after `render_range_text` (before `pub fn extract`):

```rust
/// Render a compact one-line-per-turn MAP of the inclusive range, for navigating
/// a whole session cheaply: turn index, role, the turn's first message uuid, a
/// short text preview, and tool-call one-liners. Tool RESULTS and full prose are
/// omitted — this is the "table of contents", not the content.
fn render_outline(active: &[&RawLine], a_start: usize, b_end: usize) -> String {
    let mut out = String::new();
    for (idx, (s, e)) in turn_spans(active).iter().enumerate() {
        if *s < a_start || *e > b_end {
            continue; // only turns fully within the range
        }
        let head = active[*s];
        let role = head
            .value
            .pointer("/message/role")
            .and_then(|v| v.as_str())
            .unwrap_or(head.kind.as_str());
        let uuid = head.uuid.map(|u| u.to_string()).unwrap_or_default();
        let mut preview = String::new();
        let mut tools: Vec<String> = Vec::new();
        for l in &active[*s..=*e] {
            match l.value.pointer("/message/content") {
                Some(Value::String(t)) => {
                    if preview.is_empty() {
                        preview = t.clone();
                    }
                }
                Some(Value::Array(blocks)) => {
                    for b in blocks {
                        match b.get("type").and_then(|v| v.as_str()) {
                            Some("text") => {
                                if preview.is_empty() {
                                    if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                        preview = t.to_string();
                                    }
                                }
                            }
                            Some("tool_use") => {
                                let name =
                                    b.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                                let input =
                                    b.get("input").map(|v| v.to_string()).unwrap_or_default();
                                tools.push(format!("▸{}({})", name, cap(&input, 60)));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        let preview = cap(&preview.replace('\n', " "), 80);
        out.push_str(&format!("#{} [{} {}] \"{}\"", idx, role, uuid, preview));
        if !tools.is_empty() {
            out.push(' ');
            out.push_str(&tools.join(" "));
        }
        out.push('\n');
    }
    out
}
```

- [ ] **Step 3: Change the `extract` signature (optional range + outline)**

Replace the whole `pub fn extract` (currently lines ~172-177):

```rust
/// Read a session and render either the full text of a range or a compact
/// outline. With no `from`/`to`, renders the whole active path; with both, the
/// turn-snapped range. Passing only one of `from`/`to` is an error.
pub fn extract(
    path: &Path,
    from: Option<Uuid>,
    to: Option<Uuid>,
    outline: bool,
) -> Result<String> {
    let lines = read_raw(path)?;
    let active = active_path(&lines);
    let (a, b) = match (from, to) {
        (Some(f), Some(t)) => snap_range(&active, f, t)?,
        (None, None) => {
            if active.is_empty() {
                return Err(anyhow!("session has no messages on the active path"));
            }
            (0, active.len() - 1)
        }
        _ => return Err(anyhow!("provide both --from and --to, or neither")),
    };
    Ok(if outline {
        render_outline(&active, a, b)
    } else {
        render_range_text(&active, a, b)
    })
}
```

- [ ] **Step 4: Update the existing extract test + add new ones**

In `surgery.rs` tests, update `extract_renders_range_with_roles` to the new signature and id-annotated header:

```rust
    #[test]
    fn extract_renders_range_with_roles() {
        let p = fixture();
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        let c = "00000000-0000-0000-0000-0000000000c4".parse().unwrap();
        let text = extract(&p, Some(a), Some(c), false).unwrap();
        assert!(text.contains("[user 00000000-0000-0000-0000-0000000000a1]"));
        assert!(text.contains("A"));
        assert!(text.contains("▸ Bash"));
        assert!(text.contains("→ result: out"));
        assert!(text.contains("C"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_whole_path_when_no_range() {
        let p = fixture();
        let text = extract(&p, None, None, false).unwrap();
        // First and last turns of the fixture are present.
        assert!(text.contains("A"));
        assert!(text.contains("D"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_rejects_one_sided_range() {
        let p = fixture();
        let a = "00000000-0000-0000-0000-0000000000a1".parse().unwrap();
        assert!(extract(&p, Some(a), None, false).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_outline_is_compact() {
        let p = fixture();
        let text = extract(&p, None, None, true).unwrap();
        // One line per turn, tool-call one-liner present, NO results / no full body.
        assert!(text.contains("#0 [user 00000000-0000-0000-0000-0000000000a1]"));
        assert!(text.contains("▸Bash"));
        assert!(!text.contains("→ result"), "outline omits tool results");
        // The last turn (user "D") shows as its own header line.
        assert!(text.contains("\"D\""));
        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 5: Update the surgery extract test in `fork_test`/CLI — TUI handler**

In `crates/tui/src/main.rs`, update the `Extract` subcommand definition (make `from`/`to` optional, add `--outline`):

```rust
    /// Print the messages in a range (turn-snapped) — or the whole session — for
    /// summarizing/harvesting. With --outline, print a compact one-line-per-turn map.
    Extract {
        /// Session file path or bare session id.
        #[arg(long)]
        session: String,
        /// Start message id (uuid). Omit (with --to) to read the whole session.
        #[arg(long)]
        from: Option<String>,
        /// End message id (uuid). Omit (with --from) to read the whole session.
        #[arg(long)]
        to: Option<String>,
        /// Compact map: one line per turn (ids + tool-call one-liners), no outputs.
        #[arg(long)]
        outline: bool,
    },
```

…and update the handler arm:

```rust
            SurgeryOp::Extract { session, from, to, outline } => {
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
                print!("{}", surgery::extract(&path, a, b, *outline)?);
            }
```

- [ ] **Step 6: Build + test**

Run: `cargo test -p claude-manager-core surgery && cargo build -p claude-manager-tui`
Expected: all surgery tests pass (updated + 3 new); TUI binary compiles with the new `extract` signature.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/session/surgery.rs crates/tui/src/main.rs
git commit -m "feat(core): surgery extract gains whole-session, uuid annotation, --outline

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: The `session-harvest` skill

**Files:**
- Create: `claude-manager/skills/session-harvest/SKILL.md`

This is a Markdown workflow (no unit tests). Its deliverable is the skill doc itself; verification is the real-session exercise in Task 3.

- [ ] **Step 1: Write the skill**

Create `crates/../skills/session-harvest/SKILL.md` at the repo path `/Users/zillydev/my/Dev/Personal/claude-manager/skills/session-harvest/SKILL.md` with this content:

````markdown
---
name: session-harvest
description: Use when the user wants to mine a PAST Claude Code session for reusable artifacts — "what can be reused from session X", "make a skill/kb/script from messages <id>..<id>", "distill/harvest session X", "summarize what we did in session X". Read-only: it never rewrites a session (that's session-surgery). Produces skills, kb entries (runbooks live in the kb), one-off summaries, and scripts for deterministic flows.
---

# Session Harvest

You are mining a finished Claude Code session for knowledge worth keeping, and turning it into durable artifacts: a **skill**, a **kb entry** (the user's runbooks live in the kb), a **summary**, or a **script** for a deterministic flow.

**Read-only guarantee:** this skill only READS sessions (through the `claude-manager` binary's `surgery extract`) and WRITES new artifact files. It never rewrites or touches a session file — that's session-surgery's job, not this one. Confirm every file write (destination + one-line intent) before writing.

## Two modes (infer from the request)

- **Targeted** — the user names an artifact type and a span (`from`/`to` message ids) or a whole session: "make a skill from messages A..B", "summarize session X". Produce exactly that.
- **Discovery** — the user gives a session and asks what's reusable: "what can be reused from session X?". Return a **ranked candidate list**; the user picks which to generate.

## Locate the binary (once)

```bash
BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-manager
[ -x "$BIN" ] || BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/debug/claude-manager
[ -x "$BIN" ] || { (cd /Users/zillydev/my/Dev/Personal/claude-manager && cargo build --release); BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-manager; }
```

## Navigation: outline-first → drill down → fan out

A session can be huge. Never read it all raw. Navigate in passes:

1. **Outline.** Get the compact, id-annotated map (one line per turn — ids + tool-call one-liners, no outputs):
   ```bash
   "$BIN" surgery extract --session "$SESSION" --outline
   ```
   This is the "table of contents": it shows the arc and, crucially, the **tool-call sequence** (the "what was done"), with each turn's message uuid. A long session is a few hundred lines here — read it fully.

2. **Identify & classify spans.** From the outline, mark contiguous turn ranges that carry reusable value and classify each (rubric below). Record each candidate's `from`..`to` message uuids from the outline.

3. **Drill down** only into the span(s) you will turn into an artifact:
   ```bash
   "$BIN" surgery extract --session "$SESSION" --from "$FROM" --to "$TO"
   ```
   (full text, tool outputs capped, every turn uuid-annotated.)

4. **Fan out for scale.** If the session is large AND yields several candidates, dispatch one subagent per candidate span: each runs the drill-down `extract --from --to` for its window and returns a structured draft, so your own context only holds the outline + aggregated results. Small sessions: do it inline.

## Classification rubric

For each candidate span, pick the type:

- **deterministic, no judgment** (a fixed command sequence that worked) → **script**
- **durable reference knowledge** — gotchas, why-it-works, a flow's shape, a triage playbook, a runbook → **kb**
- **reusable process WITH judgment** — a repeatable way of working that needs decisions → **skill**
- **one-time recap** of this session's outcome → **summary**

A span that is none of these yields no candidate. Don't manufacture artifacts.

## Discovery output

Return a ranked list. For each candidate:

```
N. [type] <proposed title>
   span: <from-uuid> .. <to-uuid>   (turns #x–#y)
   why: <one line — what makes this reusable>
```

Then ask which to generate (e.g. "make 2 and 4"). Generate each chosen one via the matching production path below.

## Producing each artifact type

**skill** — Do NOT hand-roll the format. Invoke the `superpowers:writing-skills` skill and author the skill from the drilled-down span. Decide destination by judgment and CONFIRM it: `~/.claude/skills/<name>/` for a general/cross-project skill, or `/Users/zillydev/my/Dev/Personal/claude-manager/skills/<name>/` for a claude-manager-specific one.

**kb** — Invoke the `kb` skill to write the entry into the user's Obsidian wiki (`/Users/zillydev/my/Obsidian/02 - Areas/Emergent/knowledge/<topic>.md`). The kb skill checks for an existing entry to update, matches the house style, and cross-references. Runbooks are kb entries.

**summary** — Write a dense, handoff-quality recap and RENDER it in chat (don't save unless asked, then save to a path the user gives). Cover, concretely: goal; decisions made and WHY; current state; concrete artifacts verbatim (file paths, names, commands+outcomes, ids, config keys+values, URLs, errors, versions); open threads; immediate next step. Drop verbose tool output and pleasantries. (Same discipline as session-surgery's summary.)

**script** — ONLY for a genuinely deterministic flow (a fixed sequence of commands that ran, no branching/judgment). If the flow needed decisions, recommend a **skill** instead. Write the script (shell/python as fits), make it idempotent where reasonable, propose a path from what it does (repo `scripts/`, `~/.claude`, …) and CONFIRM before writing.

## Confirm before writing & report

Before any file write: show the destination path + a one-line description and get a yes. After: report what was created and where (and, for discovery, the remaining candidate list so the user can ask for more).

## Errors from the binary

- **"from/to id ... is not on the active path"** — the uuid isn't on the current path (abandoned branch, metadata line, or typo). Re-read the outline and re-pick.
- **"provide both --from and --to, or neither"** — pass both ids for a range, or neither for the whole session.
- **"session '...' not found"** — wrong id/path; confirm with the user (or pass the full `.jsonl` path).
````

- [ ] **Step 2: Sanity-check the skill file**

Run: `head -3 /Users/zillydev/my/Dev/Personal/claude-manager/skills/session-harvest/SKILL.md`
Expected: shows the YAML frontmatter (`---`, `name: session-harvest`, `description: ...`).

- [ ] **Step 3: Commit**

```bash
git add skills/session-harvest/SKILL.md
git commit -m "feat(skills): add session-harvest skill (mine sessions for reusable artifacts)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS (core incl. the 4 surgery extract tests; webui; TUI), 0 failures.

- [ ] **Step 2: Real-session CLI smoke (proves the reader works on real data)**

Build once (`cargo build --release`), then against a real session (e.g. the user's `91052458-9e2f-4cb9-80dc-afe3b9ce904d`):

```bash
BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-manager
"$BIN" surgery extract --session 91052458-9e2f-4cb9-80dc-afe3b9ce904d --outline | head -20
"$BIN" surgery extract --session 91052458-9e2f-4cb9-80dc-afe3b9ce904d --outline | wc -l
```
Expected: the outline prints one `#<idx> [<role> <uuid>] "<preview>" ▸Tool(...)` line per turn; the line count is far smaller than the file's line count (a compact map); no tool-result bodies appear.

- [ ] **Step 3: Manual harvest exercise (hand to the user)**

Have the user (in a session where the skill is available) try: "what can be reused from session `<id>`?" → expect a ranked candidate list with correct id ranges; then "make the summary" / "make a skill from #N" → expect the right artifact, destination confirmed before writing.

- [ ] **Step 4: Report**

Report exact `cargo test` counts and the CLI smoke output (outline line-count vs file line-count). Do not claim the manual harvest works without the user's confirmation. Do not merge — the user directs merges.

---

## Notes for the implementer

- `extract`'s signature change is breaking; the only callers are the TUI handler and the `extract_renders_range_with_roles` test — both updated in Task 1. (webui uses `surgery::rewrite`, not `extract`.)
- The outline operates on the active path; turns partially outside a given range are skipped (whole-session passes include all turns).
- The skill must DELEGATE skill-authoring to `writing-skills` and kb-writing to `kb` — don't reimplement those. Summaries and scripts are produced inline.
- Confirm every file write. Harvest writes only new artifacts; it never edits a session.
