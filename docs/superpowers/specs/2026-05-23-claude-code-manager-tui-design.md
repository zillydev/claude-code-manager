# claude-code-manager — TUI for Claude Code sessions

**Date:** 2026-05-23
**Author:** Zaid (with Claude)
**Status:** Draft, awaiting implementation

## Goal

A terminal UI to browse and read Claude Code sessions stored under `~/.claude/projects/`. v1 scope: list sessions across all projects, open one, render its transcript, filter the list, expand/collapse tool calls, live-tail an active session.

## Non-goals (v1)

- Editing or replaying sessions
- Cross-session search of message content
- Token-cost roll-ups, archival, deletion
- Resume / attach to a live `claude` process
- Subagent transcript drill-in (subagents shown as collapsed one-liners only)

## Source data

Reference for what we read from disk:

- **`~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl`** — the transcript. Encoded-cwd replaces `/` with `-` (e.g. `/Users/x/foo` → `-Users-x-foo`). Each line is one JSON event; types observed: `user`, `assistant`, `system`, `attachment`, `summary`, `ai-title`, `custom-title`, `agent-name`, `last-prompt`, `permission-mode`, `file-history-snapshot`. Forward-compat: unknown types fall through to `Unknown(Value)`.
- **`~/.claude/projects/<encoded-cwd>/<sessionId>/subagents/agent-<id>.jsonl`** — only used in v1 to count subagent turns for the collapsed one-liner. Full content not rendered.
- **`~/.claude/sessions/<pid>.json`** — live PID registry. Used in v1 only to mark a session "live" in passing; not required.

We do **not** read `~/.claude.json`, `file-history/`, `tasks/`, `shell-snapshots/`, `daemon/` in v1.

## Architecture

Three layers, single binary:

```
ui ── consumes ── session ── uses ── store
```

- **`store`** — filesystem layer. Knows nothing about events. Lists project directories, opens jsonl readers, file watcher.
- **`session`** — domain model. Parses jsonl lines into typed events, builds session summaries (title, mtime, token totals), resolves the current path through the conversation tree.
- **`ui`** — ratatui application. Two pages (list, transcript), each with its own state. Knows nothing about disk paths — receives `Session` and `Vec<Session>` values.

## Crate layout

```
claude-code-manager/
├── Cargo.toml
└── src/
    ├── main.rs             ← terminal setup, panic hook, arg parsing
    ├── app.rs              ← App struct, event loop, page routing
    ├── store/
    │   ├── mod.rs          ← public: discover(), open_session()
    │   ├── paths.rs        ← encode/decode `-Users-x-foo` ↔ cwd
    │   └── watcher.rs      ← notify-based jsonl tail
    ├── session/
    │   ├── mod.rs          ← Session, SessionSummary
    │   ├── event.rs        ← Event enum + sub-types
    │   ├── parse.rs        ← serde JSON line parsing
    │   ├── tree.rs         ← parentUuid graph, current-path resolution
    │   └── summary.rs      ← title/mtime/usage extraction
    └── ui/
        ├── mod.rs          ← theme + shared widgets
        ├── theme.rs        ← colors per role
        ├── list_view.rs    ← sessions page
        └── transcript_view.rs ← transcript page
```

## Data model

```rust
// session/event.rs
pub enum Event {
    User { uuid: Uuid, parent: Option<Uuid>, ts: DateTime<Utc>, content: UserContent, is_meta: bool, is_sidechain: bool },
    Assistant { uuid: Uuid, parent: Option<Uuid>, ts: DateTime<Utc>, model: String, content: Vec<Block>, usage: Option<Usage>, is_sidechain: bool },
    System { uuid: Uuid, parent: Option<Uuid>, ts: DateTime<Utc>, subtype: String, content: String },
    Attachment { uuid: Uuid, parent: Option<Uuid>, ts: DateTime<Utc>, hook_name: Option<String>, content: String },
    Summary { uuid: Uuid, parent: Option<Uuid>, ts: DateTime<Utc>, content: String },
    AiTitle(String),
    CustomTitle(String),
    AgentName(String),
    LastPrompt { leaf_uuid: Uuid },
    PermissionMode(String),
    FileHistorySnapshot { message_id: Uuid },
    Unknown(serde_json::Value),
}

pub enum UserContent {
    Text(String),
    ToolResults(Vec<ToolResult>),
}

pub enum Block {
    Text(String),
    Thinking(String),
    ToolUse { id: String, name: String, input: serde_json::Value },
}

pub struct ToolResult { pub tool_use_id: String, pub content: String, pub is_error: bool }
pub struct Usage { pub input: u64, pub output: u64, pub cache_creation: u64, pub cache_read: u64 }

// session/mod.rs
pub struct Session {
    pub project_path: PathBuf,
    pub project_name: String,
    pub id: Uuid,
    pub file_path: PathBuf,
    pub events: Vec<Event>,
    pub leaf: Option<Uuid>,
    pub title: Option<String>,
    pub mtime: SystemTime,
    pub bytes_read: u64,  // for tail resumption
}

pub struct SessionSummary {
    pub id: Uuid,
    pub file_path: PathBuf,
    pub project_name: String,
    pub title: String,         // already resolved (custom > ai > first user msg > "(untitled)")
    pub mtime: SystemTime,
    pub total_tokens: u64,
}
```

## Parsing

- Read jsonl line-by-line via `BufReader::lines()`.
- Each line parsed with `serde_json::from_str::<Event>(&line)`.
- Use `#[serde(tag = "type", rename_all = "kebab-case")]` plus a `#[serde(other)]` arm for forward compat.
- Parse errors per line: increment a counter, push `Event::Unknown(raw_value)` if the value was valid JSON, otherwise drop the line. Never abort the file.

For the list page we parse the full file once but only retain summary fields. For the transcript page we keep the full `Vec<Event>` in memory. Re-parse on `r` (refresh).

## Tree resolution

Most sessions are linear. Forks happen on edit/rewind. To render the "current" conversation:

1. Find the leaf UUID: latest `LastPrompt` event's `leaf_uuid`, falling back to the UUID of the last `User`/`Assistant` event in file order if no `LastPrompt`.
2. Build `HashMap<Uuid, &Event>` from all events that have a UUID.
3. Walk backward via `parent` until None, collecting events.
4. Reverse → the rendered path.

Non-rendered events (`AiTitle`, `LastPrompt`, etc.) still update session metadata but aren't part of the path.

## Tool-call pairing

Before rendering, scan once for all `UserContent::ToolResults` and index by `tool_use_id` → `&ToolResult`. When rendering an assistant `Block::ToolUse`, look up its pair. The tool_result `user` row that owns the matched result is then **skipped** during rendering (it's only shown as the expanded form of its `ToolUse`).

## UI

### List page

Table widget with cursor row highlighted, columns:
`Project | Title | Updated (relative) | Tokens`.

- Title fallback chain: `custom-title` > `ai-title` > first `User` event whose `is_meta == false` AND `content == UserContent::Text(_)` (trimmed to ~60 chars, with newlines collapsed to spaces) > `(untitled)`.
- Updated rendered as `2m`, `3h`, `1d`, `12d`.
- Tokens = sum of `input + output + cache_creation` across assistant rows. Skip `cache_read` (avoids double-counting).
- `/` opens an inline filter input at the bottom; filters by substring against `project + " " + title` (case-insensitive). `Esc` clears + closes the input.
- `r` refresh (re-discover from disk).
- `q` quit, `Enter` open.

### Transcript page

Stateless render of `Vec<Line>` from `(session, expanded: HashSet<String>, show_thinking: bool)`.

For each event in the resolved path:

- **User text** → `[HH:MM] user\n  <wrapped text>`.
- **Assistant text** → `[HH:MM] assistant (model)\n  <wrapped text>`.
- **Thinking** → if `show_thinking` is on, render with dimmed style. Otherwise nothing.
- **ToolUse** → single line `  ▸ Tool: <short args>`. The cursor can land on this line; pressing `Enter`/`Space` adds its `id` to `expanded` and the next render shows the result indented underneath.
- **System (local_command)** → `[HH:MM] system: /<cmd>`.
- **Attachment** → render only if the hook is not the SessionStart skill-injection hook (we filter that out as noise).
- **Subagent** (assistant with `is_sidechain=true`) — v1 only renders the *spawn* assistant turn normally; the actual subagent thread is summarised as `▸ Agent (<type>): <description> · <N turns>` on a single line, not expandable in v1. Subagent file is read just for the turn count.

Cursor model: a `Vec<RenderItem>` where each `RenderItem` is `{ start_y: u16, kind: Turn | ToolCall { id } | Subagent }`. `j/k` moves the cursor between items; `↑↓` is per-line scroll. PgUp/PgDn jump by viewport height. `Enter` expands a `ToolCall`. `g/G` jump to first/last item. `t` toggles `show_thinking`. `Esc` returns to list, dropping any file watcher.

### Theme

Role colors: `user` cyan, `assistant` default, `system` dim yellow, `attachment` dim, `thinking` dim italic, `tool_use` magenta, `tool_result` dim. Cursor line: inverted style.

## Live tail

When a transcript is open, spawn a `notify::RecommendedWatcher` on the file. On modify event:

1. Reopen the file at offset `session.bytes_read`.
2. Read to EOF, line-by-line.
3. Parse each new line, append to `session.events`, update `bytes_read`.
4. Recompute the leaf (last `LastPrompt`/last uuid).
5. Mark UI dirty; debounced (80 ms) so a burst of writes coalesces into one redraw.

The watcher channel feeds the main event loop alongside `crossterm::event::poll`. On `Esc`/`q`, the watcher is dropped before transitioning pages.

## Error handling

- **Discovery errors** (unreadable project dir, permission denied) — push to `Vec<Warning>`, skip the entry.
- **Parse errors** — per-line; counted but never fatal.
- **Render code** never panics — pure functions on validated data.
- **Terminal restore** — `Drop` guard restores the terminal on panic. `main` sets a panic hook that prints to stderr after restore.
- **v1** exposes warnings only via stderr after exit. A `w` warnings page is a v1.1 follow-up.

## Dependencies

| Crate           | Why                                |
|-----------------|------------------------------------|
| `ratatui`       | TUI framework                      |
| `crossterm`     | Cross-platform terminal backend    |
| `serde`         | Derive                             |
| `serde_json`    | jsonl parsing                      |
| `chrono`        | Timestamps + relative time         |
| `notify`        | File watcher for live tail         |
| `uuid`          | Uuid type                          |
| `dirs`          | Locate `~/.claude`                 |
| `clap`          | Minimal arg parsing (`--root`)     |
| `anyhow`        | Error plumbing                     |
| `insta` (dev)   | Snapshot tests                     |

## CLI

```
claude-code-manager [--root PATH]
```

`--root` overrides `~/.claude` for tests / inspecting another user's tree. Defaults to `dirs::home_dir()?.join(".claude")`.

## Testing strategy

1. **Unit** — `paths::decode/encode` round-trips; `parse::Event` deserializes each known type from a fixture; unknown type → `Event::Unknown`.
2. **Tree** — fixture jsonl with two branches; verify resolved path matches the leaf chain.
3. **Snapshot** — `insta` snapshots of `Vec<Line>` rendered from a small fixture session (1 user turn, 1 tool call, 1 expanded tool call).
4. **Smoke** — open against the real `~/.claude/projects/` and verify the current session lists and renders.

## Out-of-scope follow-ups

- Cross-session full-text search
- Cost dashboard (per-day, per-project)
- Resume picker that wraps `claude --resume <id>`
- Subagent drill-in (recursive transcript view)
- Sidebar showing related TaskCreate todos for the session
- Compaction-aware view (collapsing pre-compaction turns under a `Summary` event)
- File-history viewer (diff what Claude touched)
