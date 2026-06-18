# Elide Embedded Skill Prompts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In `surgery extract`, collapse injected skill prompts (`Base directory for this skill: …` + full SKILL.md) so drill-down shows `[skill prompt elided: <name>, <N> chars]` and the outline shows `skill: <name>` instead of 10 KB of plumbing.

**Architecture:** A `display::skill_prompt_name` detector; `render_range_text` (drill-down) elides skill-prompt message text to a one-liner via a shared `render_message_text` helper; `display::one_line` (used by the outline) maps a skill prompt to `skill: <name>`. Harvest-reader only — the webui already hides these `isMeta` messages.

**Tech Stack:** Rust.

## Global Constraints

- Detect a skill prompt ONLY by the first line starting with `Base directory for this skill:`.
- No webui change (`render.rs` already skips `isMeta` messages).
- The `Skill` tool_use block is unchanged (already small).
- `<N>` is `text.chars().count()`.
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Modify** `crates/core/src/session/display.rs` — add `skill_prompt_name`; extend `one_line`; tests.
- **Modify** `crates/core/src/session/surgery.rs` — `render_message_text` helper; elide in `render_range_text`'s string + text-block cases; tests.

---

## Task 1: `display::skill_prompt_name` + `one_line`

**Files:**
- Modify: `crates/core/src/session/display.rs`
- Test: inline in `display.rs`

**Interfaces:**
- Produces: `display::skill_prompt_name(text: &str) -> Option<String>`; `one_line` now also maps skill prompts to `skill: <name>`.

- [ ] **Step 1: Add `skill_prompt_name` + extend `one_line`**

In `crates/core/src/session/display.rs`, add the helper (near `command_label`):

```rust
const SKILL_PROMPT_PREFIX: &str = "Base directory for this skill:";

/// If `text` is an injected skill prompt (an `isMeta` user message whose first
/// line is `Base directory for this skill: <path>`), return the skill's folder
/// name (the last path component of that line). `None` otherwise.
pub fn skill_prompt_name(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    let path = first.strip_prefix(SKILL_PROMPT_PREFIX)?.trim();
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    Some(if name.is_empty() { "skill".to_string() } else { name.to_string() })
}
```

Then extend `one_line` (currently command-echo → task-notification) to also cover skill prompts:

```rust
pub fn one_line(text: &str) -> Option<String> {
    if let Some(cmd) = command_label(text) {
        return Some(cmd);
    }
    if let Some(name) = skill_prompt_name(text) {
        return Some(format!("skill: {}", name));
    }
    parse_task_notification(text).map(|tn| format!("task · {} · {}", tn.status, tn.summary))
}
```

- [ ] **Step 2: Add tests**

In `display.rs` `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn skill_prompt_name_extracts_folder() {
        let t = "Base directory for this skill: /Users/x/.claude/plugins/superpowers/6.0.2/skills/brainstorming\n\n# Brainstorming\nbody";
        assert_eq!(skill_prompt_name(t).as_deref(), Some("brainstorming"));
        assert_eq!(skill_prompt_name("a normal user message"), None);
    }

    #[test]
    fn one_line_compacts_skill_prompt() {
        let t = "Base directory for this skill: /a/b/skills/writing-plans\n# Writing Plans";
        assert_eq!(one_line(t).as_deref(), Some("skill: writing-plans"));
    }
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p claude-manager-core display`
Expected: PASS — `skill_prompt_name_extracts_folder`, `one_line_compacts_skill_prompt`, plus the existing `display::tests::*`.

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/session/display.rs
git commit -m "feat(core): display::skill_prompt_name + one_line skill compaction

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Elide skill prompts in `render_range_text`

**Files:**
- Modify: `crates/core/src/session/surgery.rs`
- Test: inline in `surgery.rs`

**Interfaces:**
- Consumes: `display::skill_prompt_name` (Task 1).

- [ ] **Step 1: Add `render_message_text` + use it in `render_range_text`**

In `crates/core/src/session/surgery.rs`, add a helper just above `render_range_text`:

```rust
/// Render a message's plain text, eliding an injected skill prompt (the full
/// SKILL.md dumped after `Base directory for this skill:`) to a one-liner so a
/// drilled span isn't drowned by static plumbing.
fn render_message_text(t: &str) -> String {
    match super::display::skill_prompt_name(t) {
        Some(name) => format!("[skill prompt elided: {}, {} chars]", name, t.chars().count()),
        None => t.to_string(),
    }
}
```

Then in `render_range_text`, route both text paths through it. Change the `Value::String` arm:

```rust
            Some(Value::String(s)) => {
                out.push_str(s);
                out.push('\n');
            }
```

to:

```rust
            Some(Value::String(s)) => {
                out.push_str(&render_message_text(s));
                out.push('\n');
            }
```

and the `text` block arm:

```rust
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                out.push_str(t);
                                out.push('\n');
                            }
                        }
```

to:

```rust
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                out.push_str(&render_message_text(t));
                                out.push('\n');
                            }
                        }
```

- [ ] **Step 2: Write the test (drill-down elision + outline label)**

Add to the `surgery.rs` test module:

```rust
    #[test]
    fn extract_elides_skill_prompt() {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill: /x/y/skills/brainstorming\n# Brainstorming\nlong body body body"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"user","content":"normal message"}}"#, "\n",
        );
        let p = write_tmp("skillprompt", body);

        // Drill-down: the skill prompt collapses; its body is gone; normal text stays.
        let full = extract(&p, None, None, false, false).unwrap();
        assert!(full.contains("[skill prompt elided: brainstorming,"), "got: {full}");
        assert!(!full.contains("# Brainstorming"), "skill body must not appear");
        assert!(full.contains("normal message"), "normal text stays verbatim");

        // Outline: the skill-prompt turn shows `skill: brainstorming`.
        let outline = extract(&p, None, None, true, false).unwrap();
        assert!(outline.contains("skill: brainstorming"), "got: {outline}");
        assert!(!outline.contains("Base directory for this skill"), "no raw path");

        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 3: Run the test + full core suite**

Run: `cargo test -p claude-manager-core surgery`
Expected: PASS — `extract_elides_skill_prompt` plus all existing surgery tests.

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/session/surgery.rs
git commit -m "feat(core): elide embedded skill prompts in extract drill-down + outline

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS (core incl. the new display + surgery tests; webui; TUI), 0 failures.

- [ ] **Step 2: Real-session smoke**

Build (`cargo build --release`), then on this session (which is full of skill invocations):

```bash
BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-manager
S=c84ba790-92b6-41ea-881f-971902858ce2
# Outline: skill prompts show as `skill: <name>`, no raw "Base directory" path
"$BIN" surgery extract --session "$S" --outline --all-segments | grep -c 'Base directory for this skill'   # expect 0
"$BIN" surgery extract --session "$S" --outline --all-segments | grep -oE 'skill: [a-z-]+' | sort -u | head
# Drill-down a whole session: no 10 KB skill bodies, only elided one-liners
"$BIN" surgery extract --session "$S" --all-segments | grep -c '\[skill prompt elided:'   # expect > 0
"$BIN" surgery extract --session "$S" --all-segments | grep -c '# Brainstorming Ideas Into Designs'   # expect 0
```
Expected: 0 raw `Base directory for this skill` lines in the outline; `skill: brainstorming` / `skill: writing-plans` / etc. appear; the full extract has `[skill prompt elided: …]` one-liners and 0 embedded SKILL.md headings.

- [ ] **Step 3: Report**

Report exact `cargo test` counts and the smoke numbers. Do not merge — the user directs merges.

---

## Notes for the implementer

- Detection is purely the `Base directory for this skill:` first-line prefix — other `isMeta` messages are untouched.
- `render_message_text` is the single elision point; both the string-content and text-block paths route through it (DRY).
- The webui is unchanged (it already skips `isMeta` user messages in `render.rs`).
