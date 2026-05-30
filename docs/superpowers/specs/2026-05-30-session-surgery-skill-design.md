# Session Surgery — skill + CLI for targeted history compaction

**Date:** 2026-05-30
**Status:** Approved, pre-implementation
**Goal:** A Claude Code skill that rewrites a session's history to reduce context — by **summarizing** or **pruning** a range of messages — producing a NEW, resumable session while leaving the original untouched.

## Motivation

Claude Code's auto-compaction is untargeted. Often you know exactly which span burned context (an exploration phase, a giant tool dump, a dead-end) and want to collapse just that, keeping everything else verbatim. With the session-editor pipeline already built (`core::edit::commit`: flatten, regenerate uuids, repair tool pairing, copy-on-edit) and per-message **copy id** buttons in the webui, this is now a thin orchestration layer.

## Settled decisions

| Decision | Choice |
|---|---|
| Operations | **summarize a range [A..B]** and **prune a range [A..B]** |
| Rewrite engine | new **`claude-manager surgery`** CLI subcommands, reusing `core::edit::commit` (model never hand-edits jsonl) |
| Summarizer | the **running Claude, inline** (reads the range, writes the summary) |
| Output | always a **new session** (copy-on-edit); original file never modified |
| Agent-performance guard | high-fidelity, handoff-quality summary; range snaps to whole-turn boundaries; clean tool-pairing |

## Two principles (hard constraints)

1. **Protect the resumed agent.** The rewritten session must let the agent continue working as if the span were genuinely part of history. This drives: (a) the summary is *handoff-quality* (decisions, current state, file paths, message/PR ids, open threads, next step — not a vague recap); (b) cuts snap to whole-turn boundaries so no turn is left half-present; (c) tool_use/tool_result pairing stays valid (the commit pipeline repairs any straggler).
2. **Copy-on-edit only.** Every operation writes a new session file via `commit`; the source session is read-only. Nothing is ever written back to the original.

## CLI surface

Introduce a `surgery` subcommand group on the existing `claude-manager` binary (alongside the current `--print`/`--fork-test` flags; this requires moving to a clap subcommand structure, keeping the bare-invocation = launch-TUI default).

### `claude-manager surgery extract --session <path|id> --from <uuidA> --to <uuidB>`
Prints the messages in the **turn-snapped, inclusive** range A..B on the active path, human-readable: `[role] text`, with tool calls shown as `▸ Tool: <args>` and tool results included but each capped to ~4000 chars (`…[N more chars]`) so the orchestrating session's context stays bounded while remaining faithful enough to summarize. Used only by the summarize flow.

### `claude-manager surgery rewrite --session <path|id> --from <uuidA> --to <uuidB> --mode <summary|prune> [--summary-file <f>]`
Builds the new raw-line list from the active path and commits it via `core::edit::commit`:
- **prune:** `lines[..A_start] + lines[B_end+1..]`
- **summary:** `lines[..A_start] + <summary message> + lines[B_end+1..]` (summary text read from `--summary-file`)

Prints: new session id, new path, `claude --resume <id>`, and a **content-size delta** — bytes removed, message count before→after. Tool-pairing across the cut is auto-repaired by `commit`.

### Range semantics
- `A`/`B` are **event uuids** (the ids the webui copy-id button yields; for an assistant turn it's the head event's uuid).
- **Turn-snapping:** `A_start` snaps to the first raw line of A's turn; `B_end` snaps to the last raw line of B's turn, *including the trailing tool_result user lines* of an assistant turn. This guarantees no turn is half-cut.
- Validation: both uuids must be on the active path and `A_start ≤ B_end`; else error out (no file written).
- `--session` accepts a full path or a bare session id (resolved as the unique `<id>.jsonl` under `~/.claude/projects/**`).

## The summary message

The replacement is a single **user-role** message (matches how we verified edited/synthesized messages flow into the resumed model's context), clearly marked so the agent reads it as established context, not a fresh instruction:

```
[Earlier session context — summarized to save tokens. Treat as established history.]

<summary written by the running Claude>
```

The SKILL.md instructs the model to write the summary like a **handoff note**: the task/goal, key decisions and why, the current state of the work, concrete artifacts (file paths, function names, session/PR/message ids, commands run and outcomes), unresolved threads, and the immediate next step. This is the lever that protects agent performance — the summary must carry forward everything the agent needs, just denser.

A user-role note can sit next to another user message after the cut; Claude Code already coalesces consecutive same-role messages, and we proved resume tolerates this.

## Skill: `session-surgery`

Lives at `skills/session-surgery/SKILL.md` in this repo (tightly coupled to the binary), symlink-able into `~/.claude/skills/`. It calls the built `claude-manager` binary (release build, or on `PATH`).

**Frontmatter:** `name: session-surgery`, `description:` triggers on intents like "summarize/compact messages between two ids", "prune messages between …", "reduce context in session X".

**Flow — summarize:**
1. Resolve the target session (id or path) and the two message ids from the user's prompt (copied from the webui).
2. **Live-session check:** if a `~/.claude/sessions/*.json` has a running PID for that session id, warn (it's safe to read, but note the session is active).
3. `surgery extract …` → read the range.
4. Write a handoff-quality summary (per the guidance above) to a temp file under `$CLAUDE_JOB_DIR/tmp` or `/tmp`.
5. `surgery rewrite --mode summary --summary-file …` → capture the new id + resume command + savings.
6. Report to the user: new session id, the exact `claude --resume <id>` command, and the content-size delta.

**Flow — prune:**
1. Resolve session + ids; live-session check.
2. `surgery rewrite --mode prune` → report new id + resume command + savings.

The skill never edits jsonl directly and never writes to the source session.

## Architecture / file structure

```
crates/core/src/session/edit.rs   (existing) — commit() reused as-is
crates/core/src/session/surgery.rs (NEW)     — range resolution + turn-snapping + extract-render + line splicing; pure, unit-tested
crates/tui/src/main.rs             (modify)   — clap subcommands: `surgery extract` / `surgery rewrite`
skills/session-surgery/SKILL.md    (NEW)      — orchestration + summary-quality guidance
```

Splicing and snapping live in `core` (testable, reuses `raw` + `edit`); the binary is a thin CLI shell; the skill is judgment + orchestration.

## Error handling
- Unknown/off-path uuid, `A` after `B`, ambiguous/unknown session id → clear stderr error, non-zero exit, **no file written**.
- `commit` failures (I/O) → surfaced; original untouched by construction.
- Empty result (pruning the entire path) → refuse with a message (a session needs ≥1 message).

## Testing
- **core unit:** turn-snapping expands a mid-turn uuid to full turn boundaries (incl. trailing tool_result lines); prune splice drops exactly the range; summary splice inserts one message; validation rejects off-path / reversed ranges; round-trip — the written session re-parses and passes `edit::validate`.
- **CLI smoke:** `extract` on a fixture prints the expected range; `rewrite --mode prune`/`--mode summary` produce a valid resumable file (re-read + active-path check); content-size delta is reported.
- **Manual:** real session → copy two ids in webui → run skill → `claude --resume` the result → confirm the agent continues coherently from the summarized history.

## Out of scope (this skill)
Tool-output stripping, keep-only/extract-to-session, multi-range batch surgery, restoring from the rewrite. Each is a possible follow-up; the CLI is structured so they slot in as additional `--mode`s or subcommands.
