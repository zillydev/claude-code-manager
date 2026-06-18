# Bring `surgery rewrite` to parity with `extract` — design

**Date:** 2026-06-18
**Status:** Approved

## Problem

The harvest-tooling sharpening upgraded the shared `surgery::extract` reader —
`--from/--to` accept uuid **prefixes** (resolved via `resolve_id`), the preview
elides skill prompts and annotates uuids, and range errors are clearer. Because
`session-surgery`'s preview step uses `extract`, it inherited all of that for
free.

But `surgery::rewrite` — the actual surgery *operation* — was left behind. The
CLI `Rewrite` handler still does `Uuid::parse_str(from)`/`Uuid::parse_str(to)`
and `rewrite()` takes `Uuid`, so:

- `surgery rewrite --from <prefix>` is rejected even though `extract --from
  <prefix>` (the preview) accepts it — you can preview a range with a short id
  but then can't rewrite it with the same id. An inconsistency between the
  preview and the operation.
- Off-path / typo ids give a bare parse error or `snap_range`'s message, not the
  resolver's clear `no message id matches` / `ambiguous prefix` errors.

`session-surgery/SKILL.md` is also stale — it still tells the user to copy two
full ids and doesn't mention prefixes, the elided preview, or the clearer errors.

## Design

### Code: `rewrite` resolves prefixes (parity with `extract`)

- **`surgery::rewrite(root, path, from: &str, to: &str, mode, summary_text)`** —
  change `from`/`to` from `Uuid` to `&str` (mirrors `extract`). It already reads
  the file (`read_raw`); resolve each via the shared
  `resolve_id(&lines, prefix)` (uuid prefix → unique uuid; full uuid still
  works), then run the existing `active_path` + `snap_range` + commit pipeline on
  the resolved uuids.
- **Off-active-path error (rewrite-appropriate):** after resolving, if a `from`/
  `to` uuid is not on the active path, error
  `"<label> id <uuid> is not on the active path — surgery rewrites the active conversation only"`.
  (This is the rewrite analog of `extract`'s pre-compaction hint; rewrite has no
  `--all-segments`, so it does NOT suggest that flag — it can only rewrite the
  active tree.) Unknown / ambiguous prefixes surface `resolve_id`'s own errors.

### Callers

- **CLI `Rewrite` handler** (`crates/tui/src/main.rs`): drop the two
  `Uuid::parse_str` lines; pass `from`/`to` (`&String` → `&str`) straight to
  `rewrite`.
- **webui `delete_message`** (`crates/webui/src/commands.rs`): drop the local
  `Uuid::parse_str(&message_uuid)`; pass `&message_uuid` for both `from` and `to`
  (a full uuid resolves to itself; an unknown id now yields `resolve_id`'s
  message instead of a parse error).
- **Tests:** the 7 existing `rewrite(...)` calls in `surgery.rs` switch their id
  arguments from a parsed `Uuid` to a `&str` literal (the full uuid string still
  works as a prefix of itself).

### Doc: `session-surgery/SKILL.md`

Update the skill to note: `--from/--to` (on both `extract` and `rewrite`) accept
a uuid **prefix** — copy the short id the webui's copy-id button / the `extract`
preview shows; the `extract` preview elides embedded skill prompts and annotates
each turn's uuid; and the clearer `no message id matches` / `ambiguous prefix` /
`not on the active path` errors.

## Non-goals

- `--all-segments` / multi-tree rewriting (rewrite is active-path-only by design).
- Changing `rewrite`'s output or the rewrite pipeline itself — only id resolution
  and the off-path error message change.
- Any change to `resolve_id`'s semantics (reused as-is).

## Testing

- **`rewrite` accepts a uuid prefix:** a fixture whose turns have DISTINCT leading
  hex (so a short prefix is unambiguous); `rewrite(root, path, "<8-char prefix>",
  …, Prune, None)` resolves and prunes the right turn (messages_before >
  messages_after). A full uuid string still works.
- **Off-active-path id:** a two-tree fixture; `rewrite` with a pre-compaction id
  (full uuid) errors with `is not on the active path — surgery rewrites the
  active conversation only`.
- **Unknown / ambiguous prefix:** surfaces `resolve_id`'s `no message id matches`
  / `ambiguous id prefix` (covered by the existing `resolve_id` tests; one
  rewrite-level assertion that an unknown id errors).
- Existing 7 rewrite tests pass with `&str` arguments.
- **Manual:** `surgery extract --session <id> --from <8hex> --to <8hex>` (preview)
  then `surgery rewrite --session <id> --from <8hex> --to <8hex> --mode prune`
  both accept the same short id and succeed.
