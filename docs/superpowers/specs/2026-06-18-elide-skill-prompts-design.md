# Elide embedded skill prompts in extract — design

**Date:** 2026-06-18
**Status:** Approved

## Problem

The second `session-harvest` evaluation flagged this as the top remaining
friction: in drill-down (`surgery extract --from/--to`), a span that includes a
**Skill invocation** dumps the entire embedded skill prompt. Each is an
`isMeta:true` user message whose text starts with `Base directory for this
skill: <path>` followed by the full SKILL.md (~10.5 KB for brainstorming; **45**
such messages in this session). Since nearly every feature arc begins with a
brainstorm/plan/execute skill, this static plumbing dominates any drilled span
and crowds out the real work.

The webui already hides `isMeta` messages (`render.rs` skips them); the harvest
reader (`surgery::render_range_text`/`render_outline`, which read raw lines) does
not. This is harvest-reader-only.

## Design

A core helper plus elision at the two render sites in
`crates/core/src/session/surgery.rs`:

- **`display::skill_prompt_name(text: &str) -> Option<String>`** (new, in
  `crates/core/src/session/display.rs`): if `text` starts with
  `Base directory for this skill:`, return the skill's folder name — the last
  path component of the first line (e.g. `.../superpowers/6.0.2/skills/brainstorming`
  → `brainstorming`); else `None`.
- **Drill-down `render_range_text`**: when rendering message text (the
  `Value::String` content case and the `text` block case), if
  `skill_prompt_name(text)` is `Some(name)`, emit
  **`[skill prompt elided: <name>, <N> chars]`** (N = `text.chars().count()`)
  instead of the full text.
- **Outline `render_outline`** (via `display::one_line`): extend `one_line` so a
  skill prompt maps to **`skill: <name>`** (instead of a truncated cache path).
  Same detection, compact label.

The `Skill` tool_use block (`▸ Skill: {"skill":"…"}`) is already small — unchanged.

## Non-goals

- Any webui change (it already skips `isMeta` messages).
- Eliding other `isMeta` content (only skill prompts — the `Base directory for
  this skill:` prefix — are targeted).
- Correlating the prompt to the preceding `Skill` tool_use to recover the
  namespaced `superpowers:brainstorming` form; the folder name is enough.

## Testing

- **`skill_prompt_name`:** `"Base directory for this skill: /a/b/skills/brainstorming\n# …"` → `Some("brainstorming")`; a normal message → `None`.
- **`render_range_text`:** a fixture with a skill-prompt `isMeta` user message renders `[skill prompt elided: brainstorming, <N> chars]` and NOT the body; a normal user message renders verbatim.
- **`one_line` / outline:** a skill-prompt turn's outline preview reads `skill: brainstorming` (not the raw `Base directory…` path).
- **Manual:** `surgery extract --session c84ba790-… --from <skill-prompt-turn> --to <next> --all-segments` shows the one-liner, not 10 KB of SKILL text; the outline shows `skill: <name>` lines.
