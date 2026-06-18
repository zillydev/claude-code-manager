# Sharpen harvest tooling — design

**Date:** 2026-06-18
**Status:** Approved

## Problem

Dogfooding `session-harvest` (a fresh subagent summarizing this session) surfaced
three friction points in the `surgery extract` reader the skill drives:

1. **Long uuids are error-prone to copy into `--from/--to`.** The agent
   repeatedly mis-transcribed 36-char uuids from the outline and hit "is not in
   any conversation tree". (The outline uuids themselves are correct — verified
   363/363 — the problem is copy fidelity.)
2. **Compaction-summary turns aren't flagged.** The `isCompactSummary` "This
   session is being continued…" turn is the densest, most valuable read in a
   compacted session, but renders as an ordinary `#N user` line.
3. **Range errors are uninformative.** "from id X is not in any conversation
   tree" / "from and to are in different conversation trees" don't say which
   segment, or that a pre-compaction id needs `--all-segments`.

All three are in `crates/core/src/session/surgery.rs` (`render_outline`,
`extract`) and the `Extract` CLI in `crates/tui/src/main.rs`. `extract` is called
only by that CLI handler and surgery's own tests, so the signature change is
contained. (`webui` uses `surgery::rewrite`, not `extract`.)

## Design

### ① Short ids + uuid-prefix range targeting

- **Outline header shows a short 8-char id** — the uuid's first hex group, e.g.
  `#0 user bf2baa15` instead of the full `bf2baa15-1937-4581-b9ce-6433e80847f8`.
  Easier to copy, less clutter. (8 hex is effectively unique within a session.)
- **`--from/--to` accept any uuid prefix.** A new
  `resolve_id(lines, prefix) -> Result<Uuid>` matches the prefix
  (case-insensitive) against every line uuid's string form:
  - exactly one match → that uuid;
  - none → `"no message id matches '<prefix>'"`;
  - more than one → `"ambiguous id prefix '<prefix>' matches N messages — use more characters"`.
  A full 36-char uuid still works (it is a prefix of itself).
- **Signature change:** `extract(path, from: Option<&str>, to: Option<&str>,
  outline: bool, all_segments: bool)`. `extract` resolves the prefixes to
  `Option<Uuid>` up front (against all file uuids, regardless of
  `all_segments`), then runs the existing range/segment logic on the resolved
  uuids. The CLI handler stops pre-parsing to `Uuid` and passes the raw
  `--from/--to` strings (`from.as_deref()`).
- The full (non-outline) extract keeps full uuids in its `[role uuid]` line
  annotations — it's the detail view for precise citation.

### ② Tag compaction-summary turns in the outline

In `render_outline`, after a turn passes the skip checks, if its head line has
`isCompactSummary == true` (read from `head.value.get("isCompactSummary")`),
append ` · compaction summary` to the header line:
`#1 user 5bc84a16 · compaction summary`.

### ③ Segment-aware range errors

- **Cross-tree range (all-segments):** when `from` and `to` resolve into
  different segments, report both segment positions:
  `"from <f8> is in segment 2/4, to <t8> is in segment 4/4 — a range can't cross a compaction boundary"`
  (`<f8>`/`<t8>` are the 8-char short ids).
- **Pre-compaction id without `--all-segments`:** in the active-path branch, if a
  resolved `from`/`to` uuid is not on the active path, error with a hint:
  `"<label> id <x> is not on the active path — it's in an earlier (pre-compaction) tree; pass --all-segments to reach it"`
  (instead of the bare snap_range error). This is reachable because `resolve_id`
  finds the uuid anywhere in the file, so we can tell "exists but off-path" from
  "doesn't exist".

### Skill doc touch-up

Update `skills/session-harvest/SKILL.md` to note: the outline shows short ids;
`--from/--to` accept a uuid prefix (copy the short id from the outline); turns
tagged ` · compaction summary` are the best first read in a compacted session.

## Non-goals

- A per-segment `#N` ordinal as a range target (it resets per segment under
  `--all-segments`; uuid-prefix is globally unambiguous — chosen instead).
- Changing the full (non-outline) extract's uuid annotation to short ids.
- Any webui change (this is harvest/outline tooling only).

## Testing

- **`resolve_id`:** unique prefix → uuid; full uuid → uuid; unknown prefix →
  error; ambiguous prefix (two uuids sharing a leading prefix) → error naming the
  count.
- **Outline:** header shows the 8-char id (not the full uuid); an
  `isCompactSummary` turn's header carries ` · compaction summary`.
- **Range targeting:** `extract` with an 8-char `--from`/`--to` prefix resolves
  and renders the right range; a full uuid still works.
- **Errors:** a cross-tree `--from/--to` (all-segments) errors naming both
  segments; an off-active-path id without `--all-segments` errors with the
  `pass --all-segments` hint.
- Existing extract tests updated to the `&str` signature (pass the short id or
  full uuid string instead of a parsed `Uuid`).
- **Manual:** `surgery extract --session <id> --outline --all-segments` shows
  short ids + the compaction-summary tag; drilling with an 8-char `--from/--to`
  works.
