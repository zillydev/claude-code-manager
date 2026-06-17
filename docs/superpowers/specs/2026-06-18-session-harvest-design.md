# Session Harvest — design

**Date:** 2026-06-18
**Status:** Approved

## Problem

Past Claude Code sessions contain a lot of reusable knowledge — a debugging
recipe, a deterministic command sequence, a hard-won gotcha, a whole feature's
worth of decisions — that currently dies in the transcript. The user wants a
skill to **mine a past session and turn what's reusable into durable artifacts**:
skills, kb entries (their runbooks live in the kb), one-off summaries, and
scripts for deterministic flows.

Two usage shapes:
- **Targeted:** "make a skill from messages `<id>`..`<id>`" / "summarize session
  X" → produce exactly that artifact from that span.
- **Discovery:** "what can be reused from session X?" → return a ranked list of
  candidate artifacts; the user picks which to generate.

## Relationship to session-surgery

This is a **new sibling skill** (`session-harvest`), not an extension of
session-surgery. Surgery **compresses a session so it can be resumed** (it
rewrites history → a new session file). Harvest **reads a session and emits new
reusable artifacts** — it never rewrites or touches a session file. They share
one thing: the `claude-manager` binary's `surgery extract` reader (which already
turn-snaps and caps tool output). Harvest extends that reader; it uses no part of
the `rewrite` path.

**Safety:** harvest is read-only with respect to sessions. The only writes are
*new* artifact files (skills / kb / scripts) or rendered text (summaries), and
every file write is confirmed with the user first (destination + intent).

## Artifact taxonomy & destinations

| Type | What | Destination | How produced |
|------|------|-------------|--------------|
| **skill** | Reusable process/workflow with judgment | `~/.claude/skills/<name>/` (general) or `claude-manager/skills/<name>/` (project-specific) — chosen by judgment, **confirmed** | Delegate to superpowers `writing-skills` (don't hand-roll the format) |
| **kb** | Durable reference knowledge — gotchas, flows, invariants, playbooks/**runbooks** | Obsidian wiki `~/my/Obsidian/02 - Areas/Emergent/knowledge/<topic>.md` | Delegate to the `kb` skill (checks for existing entry, matches style, cross-references) |
| **summary** | Dense recap of what the session accomplished | Rendered in chat; **offer to save** to a path the user gives (no surprise file) | Inline, using surgery's handoff-summary discipline |
| **script** | Automation of a genuinely **deterministic** flow (no judgment/branches) | **Propose** a path from what it does (repo `scripts/`, `~/.claude`, …) and **confirm** | Inline; if the flow needs judgment, recommend a skill instead |

"Runbook" is intentionally **not** a separate type — the user authors runbooks as
kb entries.

## Classification rubric (drives discovery)

For each candidate span, pick the artifact type:

- **deterministic, no judgment** (a fixed command sequence that worked) → **script**
- **durable reference knowledge** — gotchas, why-it-works, flow/shape, triage
  playbook, runbook → **kb**
- **reusable process *with* judgment** — a repeatable way of working that needs
  decisions → **skill**
- **one-time recap** of this session's outcome → **summary**

A span that is none of these (idle chatter, a dead-end, pure noise) yields no
candidate.

## Navigation: outline-first → drill down, with subagent fan-out

Sessions are large; the skill must understand the whole arc without flooding
context. Strategy:

1. **Outline pass.** Read a compact, id-annotated *map* of the whole session
   (new `extract --outline`): one line per turn —
   `#<idx>  [<role> <msg-uuid> · <ts>]  "<short text preview>"  ▸Tool(brief input) …`
   — with **no tool outputs and no full prose**. A ~2700-line session is ~100–300
   turns → a few hundred lines, fits in one context. The "what was done" signal
   (the tool-call sequence) is preserved, which is exactly what scripts/kb
   candidates key on.

2. **Identify & classify spans.** From the outline, mark contiguous turn ranges
   that carry reusable value and classify each per the rubric. This is enough for
   the **discovery** list (type · proposed title · `from`..`to` msg-ids ·
   1-line rationale). For **targeted** mode, the outline just resolves/confirms
   the requested range.

3. **Deep-read on demand.** Only for the span(s) being turned into an artifact,
   run the full `extract --from <id> --to <id>` (full text, tool-output capped,
   id-annotated) and write the artifact from that.

4. **Subagent fan-out for scale.** When a session is large *and* yields many
   candidates, dispatch a subagent per candidate span: each deep-reads its window
   via `extract --from --to` and returns a structured candidate/draft; the main
   agent only ever holds the outline + aggregated results. Small sessions are
   handled inline (no fan-out).

## Tooling changes (`claude-manager` binary)

All in the existing `surgery extract` reader; the `rewrite` path is untouched.

1. **Optional range.** `--from`/`--to` become optional; omitting both means the
   **whole active path** (first → last message on the current conversation path).
   This gives whole-session reading without the skill having to discover the
   endpoint uuids itself.
2. **Id annotation.** Each emitted turn is prefixed with its **message uuid** (and
   role/timestamp), so the skill can cite candidate ranges and the user/skill can
   target them by id. Additive to the existing output.
3. **`--outline` mode.** A compact variant: per-turn header line only — role,
   uuid, timestamp, a truncated text preview, and tool-call one-liners
   (`▸Name(short input)`); tool **results** and full assistant prose suppressed.

Active-path scope matches `extract`'s existing behavior. (Pre-compaction
orphaned trees are out of scope for v1 — harvest operates on the active path the
user sees.)

## Skill structure (the SKILL.md)

`claude-manager/skills/session-harvest/SKILL.md`, mirroring session-surgery's
shape:

- **Trigger / description** — fires on "what can be reused from session X",
  "make a skill/kb/script from messages A..B", "distill/harvest session X",
  "create a summary of this session".
- **Locate the binary** (same snippet as session-surgery).
- **Parse inputs** — session id/path; optional `from`/`to`; artifact type +
  intent; mode (targeted vs discovery) inferred from whether a type+range was given.
- **Navigate** — outline first; drill down; fan out when large (the B+C strategy).
- **Per-artifact production** — the delegation rules above (writing-skills, kb)
  and the quality bars (handoff-summary discipline; deterministic-only scripts).
- **Confirm before writing** — show destination + a one-line description; write
  only on confirmation.
- **Report** — what was created, where, and (for discovery) the candidate list.

## Non-goals

- Rewriting/compacting sessions (that's session-surgery).
- Harvesting pre-compaction orphaned trees (v1 = active path only).
- Auto-generating artifacts without the user picking (discovery lists, user
  chooses).
- A fixed "runbook" artifact type (runbooks are kb entries).

## Testing

- **Core unit tests** for the `extract` changes:
  - whole-active-path default (no `--from/--to`) returns all turns;
  - output includes each turn's message uuid;
  - `--outline` emits one header line per turn with tool-call one-liners and
    **no** tool-result/full-prose bodies.
- **Skill** is a markdown workflow (no unit tests). Verified by exercising it on a
  real session: discovery on one of the user's sessions → a sensible candidate
  list with correct id ranges; targeted generation of a skill, a kb entry, and a
  script, each landing in the right place after confirmation.
