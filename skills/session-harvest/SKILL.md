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
