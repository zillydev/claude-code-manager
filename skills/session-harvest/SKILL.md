---
name: session-harvest
description: Use when the user wants to mine a PAST Claude Code session for reusable artifacts — "what can be reused from session X", "make a skill/kb/script from messages <id>..<id>", "distill/harvest session X", "summarize what we did in session X". Read-only: it never rewrites a session (that's session-surgery). Produces skills, kb entries (runbooks live in the kb), one-off summaries, and scripts for deterministic flows.
---

# Session Harvest

You are mining a finished Claude Code session for knowledge worth keeping, and turning it into durable artifacts: a **skill**, a **kb entry** (the user's runbooks live in the kb), a **summary**, or a **script** for a deterministic flow.

**Read-only guarantee:** this skill only READS sessions (through the `claude-code-manager` binary's `surgery extract`) and WRITES new artifact files. It never rewrites or touches a session file — that's session-surgery's job, not this one. Confirm every file write (destination + one-line intent) before writing.

## Two modes (infer from the request)

- **Targeted** — the user names an artifact type and a span (`from`/`to` message ids) or a whole session: "make a skill from messages A..B", "summarize session X". Produce exactly that.
- **Discovery** — the user gives a session and asks what's reusable: "what can be reused from session X?". Return a **ranked candidate list**; the user picks which to generate.

## Locate the binary (once)

```bash
# Resolve the binary: an explicit override, else PATH, else a local build.
BIN="${CLAUDE_CODE_MANAGER_BIN:-$(command -v claude-code-manager || true)}"
if [ ! -x "$BIN" ] && [ -n "$CLAUDE_CODE_MANAGER_REPO" ]; then
  for c in release debug; do
    [ -x "$CLAUDE_CODE_MANAGER_REPO/target/$c/claude-code-manager" ] \
      && BIN="$CLAUDE_CODE_MANAGER_REPO/target/$c/claude-code-manager" && break
  done
fi
[ -x "$BIN" ] || {
  echo "claude-code-manager not found. Install it with:"
  echo "  cargo install --path crates/cli        # from a clone of the repo"
  echo "or point CLAUDE_CODE_MANAGER_BIN at the binary."
  exit 1
}
```

## Navigation: outline-first → drill down → fan out

A session can be huge. Never read it all raw. Navigate in passes:

1. **Outline.** Get the compact, id-annotated map (one line per turn — ids + tool-call one-liners, no outputs):
   ```bash
   "$BIN" surgery extract --session "$SESSION" --outline --all-segments
   ```
   This is the "table of contents": it shows the arc and, crucially, the **tool-call sequence** (the "what was done"), with each turn's message uuid. A long session is a few hundred lines here — read it fully.

   The outline covers **every conversation tree** in the file — a compacted session shows `── segment k/N · pre-compaction|active ──` dividers between its trees, so pre-compaction work is harvestable too. A `--from/--to` range must stay within one tree (the binary errors if it spans a compaction boundary).

   Each outline turn header is `#<n> <role> <short-id>` where `<short-id>` is the first 8 chars of the message uuid — copy that into `--from/--to`, which accept any uuid prefix (a full uuid works too). A turn tagged ` · compaction summary` is the "continued from a previous conversation" recap — the densest, most useful turn to read first in a compacted session.

2. **Identify & classify spans.** From the outline, mark contiguous turn ranges that carry reusable value and classify each (rubric below). Record each candidate's `from`..`to` message uuids from the outline.

   **Check whether the distillation already exists first.** Claude Code often auto-writes a project `memory/` file *during* the session, and the user may already keep a kb entry on the topic. Before proposing a **kb** or **summary**, scan the session's project memory dir — `~/.claude/projects/<encoded-cwd>/memory/` — and the user's knowledge base for an entry whose topic overlaps. If one exists, the candidate becomes **"promote/update that"** (sync it into the durable kb), NOT create-new — surface the existing path so you don't silently fork or duplicate the user's own notes.

3. **Drill down** only into the span(s) you will turn into an artifact:
   ```bash
   "$BIN" surgery extract --session "$SESSION" --from "$FROM" --to "$TO" --all-segments
   ```
   (full text, tool outputs capped, every turn uuid-annotated.)

4. **Fan out for scale.** If the session is large AND yields several candidates, dispatch one subagent per candidate span: each runs the drill-down `extract --from --to` for its window and returns a structured draft, so your own context only holds the outline + aggregated results. Small sessions: do it inline.

## Classification rubric

**First, rank by reuse — not by drama.** The best candidate is the one the user will reach for *again*, which is usually the **mundane recurring operational mechanics** (how to connect to / tail logs from / deploy / inspect a service) — NOT the impressive one-off diagnostic that happened to crack this particular incident. Before listing a span, ask: *will the user do this again next month?* A clever technique used once is a story, not a skill. Weigh `reuse frequency × time saved` and lead the candidate list with the highest-reuse item.

For each surviving span, pick the type:

- **deterministic, no judgment** (a fixed command sequence that worked) → **script**
- **durable reference knowledge** — gotchas, why-it-works, a flow's shape, how to access/operate a system → **kb**
- **reusable process WITH judgment** — a repeatable way of working that needs decisions → **skill**
- **one-time recap** of this session's outcome → **summary**

Tie-breakers:

- **kb vs skill** — if the value is in *knowing the thing exists* (a fact, a gotcha, an access recipe) → **kb**; if it's in *deciding how to apply it* (judgment, trade-offs, when-to-use) → **skill**.
- **runbook vs case study** — a *recurring* operational procedure → a **kb runbook** ("do this when X"); a *rare / one-off* incident worth remembering but unlikely to recur → at most a **case-study kb** (what happened + why), **never** a triage runbook. Don't write a "when this breaks" playbook for something that breaks once.

A span that is none of these yields no candidate. Don't manufacture artifacts — and don't promote a clever-but-rare technique just because it was hard-won.

## Discovery output

Return a ranked list. For each candidate:

```
N. [type] <proposed title>
   span: <from-uuid> .. <to-uuid>   (seg <k>/<N> · <pre-compaction|active>, turns #x–#y)
   why: <one line — what makes this reusable>
```

Then ask which to generate (e.g. "make 2 and 4"). Generate each chosen one via the matching production path below.

## Producing each artifact type

**skill** — Do NOT hand-roll the format. Invoke the `superpowers:writing-skills` skill and author the skill from the drilled-down span. Decide destination by judgment and CONFIRM it: `~/.claude/skills/<name>/` for a general/cross-project skill, or `<repo>/skills/<name>/` for a claude-code-manager-specific one.

**kb** — Write the entry into the user's knowledge base. If they have a `kb` skill (or equivalent wiki convention), invoke it so the entry lands in the right place, matches the house style, and gets cross-referenced; otherwise ask where notes live. Runbooks are kb entries.

**summary** — Write a dense, handoff-quality recap and RENDER it in chat (don't save unless asked, then save to a path the user gives). Cover, concretely: goal; decisions made and WHY; current state; concrete artifacts verbatim (file paths, names, commands+outcomes, ids, config keys+values, URLs, errors, versions); open threads; immediate next step. Drop verbose tool output and pleasantries. (Same discipline as session-surgery's summary.)

**script** — ONLY for a genuinely deterministic flow (a fixed sequence of commands that ran, no branching/judgment). If the flow needed decisions, recommend a **skill** instead. Write the script (shell/python as fits), make it idempotent where reasonable, propose a path from what it does (repo `scripts/`, `~/.claude`, …) and CONFIRM before writing.

## Confirm before writing & report

Before any file write: show the destination path + a one-line description and get a yes. After: report what was created and where (and, for discovery, the remaining candidate list so the user can ask for more).

## Errors from the binary

- **"from/to id ... is not on the active path"** — the uuid isn't on the current path (abandoned branch, metadata line, or typo). Re-read the outline and re-pick.
- **"provide both --from and --to, or neither"** — pass both ids for a range, or neither for the whole session.
- **"session '...' not found"** — wrong id/path; confirm with the user (or pass the full `.jsonl` path).
