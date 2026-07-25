---
name: session-surgery
description: Use when the user wants to shrink/compact a Claude Code session's context by summarizing or pruning a range of messages between two message ids (copied from the claude-code-manager webui's per-message "copy id" button). Produces a NEW resumable session via the claude-code-manager binary; never modifies the original. Trigger phrases include "summarize messages between <id> and <id>", "prune/cut this part of the session", "reduce context in session X", "compact this stretch".
---

# Session Surgery

You are rewriting the history of a Claude Code session so it uses less context, then handing the user a `claude --resume <id>` for a new, smaller session that picks up where the original left off.

**Two hard guarantees, both enforced by the tooling — never violate them:**
1. **Copy-on-edit.** Every operation writes a brand-new session file (new id). The original session file is read-only and is never modified. If anything goes wrong, the user loses nothing.
2. **You never hand-edit the jsonl.** All structural work goes through the `claude-code-manager` binary's `surgery` subcommands, which reuse the tested write pipeline (flatten, regenerate uuids, relink the parent chain, repair tool_use/tool_result pairing). Editing the jsonl by hand would corrupt those invariants.

## Background you need (mental model)

A Claude Code session is a `.jsonl` transcript at `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` — one JSON object per line, linked into a tree by `parentUuid`. On `claude --resume <id>`, Claude Code reconstructs the entire conversation **from that file**. This has been verified empirically: whatever is in the transcript becomes the resumed model's memory of the conversation. That is exactly why this works — we produce a new transcript with a denser middle, and the resumed agent treats the summary as real history.

**Context in real sessions is dominated by tool outputs** (file reads, bash/search dumps), not prose. A long exploration phase, a giant accidental dump, or a resolved debugging thread can be hundreds of KB. Surgery lets the user collapse *exactly that span* while keeping everything else verbatim — targeted, manual compaction (unlike Claude Code's untargeted auto-compaction).

## Inputs (parse from the user's request)

- **session** — a session id (e.g. `8cab6894-...`) or a full `.jsonl` path. The user typically copies the id from the webui detail panel's **session** row or from a `claude --resume <id>` line. If they don't give one and it's not obvious, ask.
- **from** and **to** — two **message ids** (uuids), copied with the webui's per-message **copy id** hover button. They bound the range to operate on. The range is **inclusive** and **snaps to whole-turn boundaries** automatically (picking an assistant message's id pulls in its tool calls and their results, so a turn is never half-cut). `from` must come at or before `to` on the conversation. Both `--from/--to` (on `extract` and `rewrite`) accept a uuid **prefix** — a short leading fragment is fine, the full uuid works too; the binary errors if a prefix is ambiguous. The `extract` preview annotates each turn's uuid and elides embedded skill prompts (the `Base directory for this skill:` dumps), so the span is easy to read.
- **operation** — `summarize` (replace the range with one dense summary message) or `prune` (delete the range entirely). If the user didn't say which, infer from intent and confirm: summarize when the span has durable value worth keeping in condensed form; prune when it's pure noise.

## Locate the binary (once, up front)

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
  echo "  cargo install --path crates/tui        # from a clone of the repo"
  echo "or point CLAUDE_CODE_MANAGER_BIN at the binary."
  exit 1
}
```

## Step 0 — Safety check (ALWAYS do first)

A live Claude Code process that currently has the target session open will re-persist its file from memory — it can recreate the original even after you produce a new session. We never touch the original, but a live process resuming the *old* id is a footgun. Check:

```bash
grep -l "\"sessionId\":\"$SESSION_ID\"" ~/.claude/sessions/*.json 2>/dev/null
```

If there's a match, tell the user: "that session is currently open in a running Claude Code process — finish/close it before resuming the new session, or the live process may keep re-persisting the old transcript." Then proceed (it's still safe to read the file).

## Choosing the operation and confirming the range

Before rewriting, **always `extract` first and look at what you're about to compress.** Don't blindly trust the two ids — confirm the span is the right one.

- **summarize** when the span reached conclusions you want to keep: an exploration that landed on a design, a debugging thread that found the root cause, a long back-and-forth whose *outcome* matters but whose *verbosity* doesn't.
- **prune** when the span has no lasting value: an abandoned dead-end approach, an accidental huge file/dir dump, redundant repetition. Nothing in it is needed going forward.
- **Keep the tail verbatim.** Avoid including the most recent messages in the range — recent context is usually what the agent most needs intact. If `to` is at/near the end of the conversation, double-check with the user.

## Operation: SUMMARIZE

### 1. Extract the range
```bash
"$BIN" surgery extract --session "$SESSION" --from "$FROM" --to "$TO"
```
This prints the messages in the (turn-snapped, inclusive) range as readable text. Tool inputs/results are capped to keep your context bounded — that's fine, you're summarizing, not transcribing. Read it.

### 2. Write a HANDOFF-QUALITY summary
This is the part that determines whether the resumed agent keeps performing well. **A thin/vague summary degrades the agent — that is the main failure mode of this skill.** Write the summary like a briefing to a competent teammate who will continue this exact work with no other context. It must let them resume seamlessly.

Cover, concretely:
- **Goal** — what this span was trying to accomplish.
- **Decisions made, and WHY** — the durable conclusions and their rationale. The "why" is what prevents the resumed agent from re-litigating settled questions.
- **Current state** — where the work actually stands now (done / in progress / blocked).
- **Concrete artifacts — preserve these verbatim, never paraphrase:** file paths, function/type/variable names, commands run and their outcomes, ids (session/PR/issue/commit/message), config keys + values, URLs, error strings, version numbers.
- **Open threads** — what's unresolved, known bugs, deferred items.
- **Immediate next step** — the very next action to take.

Style: dense and factual. Drop pleasantries, restated questions, and the verbose tool outputs (those are the bytes you're reclaiming). Keep every identifier and decision. Aim for the shortest text that loses nothing actionable — a 50-message stretch often compresses to ~15–40 lines. When unsure whether a detail matters, keep it (it's far cheaper than a confused resumed agent).

**Anti-patterns (do not do):** "We explored several options and picked one." (Which? Why?) · dropping file paths or ids · summarizing away the error message that explains the fix · pasting raw tool output back in (defeats the purpose) · editorializing.

Save it to a temp file:
```bash
SUMMARY_FILE=$(mktemp)
cat > "$SUMMARY_FILE" <<'EOF'
<your dense handoff summary here>
EOF
```

### 3. Rewrite (writes the new session)
```bash
"$BIN" surgery rewrite --session "$SESSION" --from "$FROM" --to "$TO" --mode summary --summary-file "$SUMMARY_FILE"
```
The binary inserts your summary as a marked context message in place of the range, repairs any tool-pairing across the cut, and writes a new flattened session. It prints the new id, path, resume command, and a `messages: N -> M (removed ~X bytes)` line.

### 4. Report back to the user
Give them: the new session's `claude --resume <id>` command (copyable), the savings (`messages N→M`, ~X bytes), and a one-line note of what you summarized. Remind them the original is untouched.

## Operation: PRUNE

1. Safety check (Step 0).
2. Optionally `extract` to confirm the span is genuinely disposable.
3. Rewrite:
   ```bash
   "$BIN" surgery rewrite --session "$SESSION" --from "$FROM" --to "$TO" --mode prune
   ```
4. Report the new `claude --resume <id>`, the savings line, and what you removed. Original untouched.

> Note: you cannot prune the very first user turn of a session — the result must start with a user message. If the user wants to drop the opening, summarize it instead (summarize inserts a user-role message). The binary will refuse a prune that would leave a leading assistant message.

## Handling errors from the binary

- **"... is not on the active path — surgery rewrites the active conversation only"** — the id resolved but sits in an earlier (pre-compaction) tree, not the active conversation surgery operates on. Re-copy an id from the active path (the webui, or the `extract` preview).
- **"no message id matches '...'"** — the prefix/uuid matches no message (typo, or a metadata line that isn't a turn). Re-copy from the webui.
- **"ambiguous id prefix '...' matches N messages — use more characters"** — the prefix is too short; copy a few more characters (or the full id).
- **"from id occurs after to id"** — the two ids are reversed; swap `--from` and `--to`.
- **"refusing: the result would contain no messages"** — the range covers the whole conversation; narrow it (you can't summarize-away everything).
- **"session '...' not found"** — the session id/path is wrong; confirm with the user (or pass the full `.jsonl` path).

## After surgery — suggest verifying

Encourage the user to resume the new session and sanity-check: `claude --resume <new-id>`, then ask the agent something that depends on the compressed span (e.g. "what's our current state and the next step?"). If it answers coherently, the summary did its job. If it's lost, the summary was too thin — re-run with a richer summary (the original is still there).
