# Session organization — design

## Goal

Give claude-code-manager its own layer of session organization, stored **separately
from claude's `.jsonl`** (never mutated): global colored **tags**, a local
**alias** shown alongside claude's auto-name, per-session **notes**, and a
sidebar **tag filter**. All keyed by the stable session `id`.

Explicitly **not** in scope this iteration: pin/favorite, archive/hide, folders,
a standalone per-session color (color = tag color), and any write to claude's
session files.

## Why a separate store (not the transcript)

Storing metadata in the `.jsonl` would (a) violate claude-code-manager's copy-on-edit
rule — the `custom-title` append was the sole exception and the cause of the
rename desync we're now fixing; (b) risk interleaved/corrupt writes, since the
live `claude` process is writing that same file; and (c) not fit **global** tags
(name+color), which need one shared registry, not a copy per transcript. A
separate file owned by claude-code-manager avoids all three.

## Storage

`<config-dir>/session-meta.json`, where `<config-dir>` resolves exactly as the
config file (`$CLAUDE_CODE_MANAGER_CONFIG_DIR` → `$XDG_CONFIG_HOME/claude-code-manager` →
`~/.config/claude-code-manager`) — share `config::config_dir()`. Atomic temp-write +
rename; missing/malformed → defaults (empty); merge-over-defaults on load.

### Schema

```json
{
  "version": 1,
  "tags": [{ "name": "wip", "color": "#e5534b" }],
  "sessions": {
    "<session-id>": { "alias": "auth refactor", "tags": ["wip"], "note": "blocked on X" }
  }
}
```

- **tags** — the global registry. `name` is the identity (unique, case-sensitive);
  `color` is one of a fixed palette. Reused across sessions.
- **sessions[id]** — `alias?`, `tags: string[]` (tag names), `note?`. Sessions with
  no metadata are simply absent.
- Referential note: a session's `tags` reference registry names; a name missing
  from the registry renders with a neutral fallback color (self-heals; never errors).

## Backend (`crates/webui/src/session_meta.rs`, mirrors `config.rs`)

- `struct SessionMeta { version: u32, tags: Vec<Tag>, sessions: HashMap<String, SessionEntry> }`,
  `struct Tag { name: String, color: String }`,
  `struct SessionEntry { alias: Option<String>, tags: Vec<String>, note: Option<String> }`
  — serde with field defaults; `Default` = empty.
- `path()` via the shared `config_dir()`, file `session-meta.json`.
- `load()` (read+parse; missing → written default; malformed → default, file left
  intact), `save()` (atomic).
- Commands `get_session_meta() -> SessionMeta`, `set_session_meta(meta) -> Result<(), String>`
  → save + broadcast `session-meta-changed`. Registered in `lib.rs`.
- Tests: path resolution reuse, roundtrip, missing→default, malformed→default,
  partial-merge, atomic (no `.tmp` left).

## Frontend

### `lib/sessionMeta.ts`
`SessionMeta` / `Tag` / `SessionEntry` types + `DEFAULT_META`; `TAG_PALETTE` (≈8
preset hex colors); `tagColor(meta, name)` (registry lookup → fallback);
`sessionEntry(meta, id)`. Pure merge helpers for the reducer.

### `hooks/useSessionMeta.tsx` (mirrors `useConfig`)
`SessionMetaProvider` at the app root (beside `ConfigProvider`): loads via
`get_session_meta`, listens `session-meta-changed` (echo-guarded), and exposes
`meta` plus mutators that each merge → optimistic set → `set_session_meta`:
- `setAlias(id, alias|null)`, `setNote(id, note|null)`, `setSessionTags(id, names)`
- `upsertTag(name, color)` (create/recolor in the registry), `removeTag(name)`
  (drop from registry + every session)
Empty `alias`/`note`/`tags` prune the entry so unused ids don't accumulate.

### Sidebar (join + display + filter)
The sidebar **joins** each `SessionSummary` with `meta.sessions[id]` — the backend
session list and `SessionSummary` are untouched.
- **Row title:** alias if present (prominent) with claude's auto-name dim/smaller
  beneath; no alias → claude's name as today.
- **Tags:** small colored chips (`tagColor`) on the row.
- **Note:** a `✎` indicator on rows that have one; full text in the info panel.
- **Tag filter:** a new control in the filter row (alongside project/date) — select
  tag(s); a session matches if it has **any** selected tag. Combines with the
  existing filters in the `filtered` memo.
- **Row `⋯` menu** (currently Delete-only) gains **Rename** (alias inline input →
  `setAlias`), **Tags…** (opens `TagEditor`), **Note…** (opens a small textarea
  popover), then Delete.

### `TagEditor` (popover)
Current tags as removable chips + an input that autocompletes existing tags and,
for a new name, assigns a palette color (next unused, or a small swatch picker).
Applies via `setSessionTags` / `upsertTag`.

## Testing
- Rust: `session_meta` load/save/merge/roundtrip/atomic + shared path resolution.
- Frontend: meta reducer (`setAlias`/`setSessionTags`/`upsertTag`/prune-empty),
  `tagColor` fallback, the tag-filter predicate, alias-vs-claude-name display logic.

## Out of scope / non-goals
- Writing any metadata into claude's `.jsonl`.
- Pin/favorite, archive, folders, standalone per-session color.
- Tag rename (only create/recolor/remove); global "manage tags" screen — defer.
- Backend join into `SessionSummary` (kept as a frontend overlay).
