# Clean noisy messages (command echoes & task-notifications) — design

**Date:** 2026-06-18
**Status:** Approved

## Problem

Two kinds of injected user messages render as ugly tag-soup:

- **Command echoes** — `<command-name>/compact</command-name> <command-message>…</command-message> <command-args>…</command-args>`.
- **Task notifications** — `<task-notification><task-id>…</task-id><tool-use-id>…</tool-use-id><output-file>…</output-file><status>completed</status><summary>…</summary><note>…</note><result>…possibly huge…</result></task-notification>`.

They appear raw in the **harvest outline** and (task-notifications) in the
**webui transcript**. Goal: render the signal, drop the boilerplate.

Reconciliation from the code: `render.rs` already **skips** command echoes in the
webui (`text.starts_with("<command-")`), so command echoes are an **outline-only**
problem. Task-notifications are NOT skipped, so they're ugly in **both**.

## Design

### 1. Core display helper (`session::display`)

New module `crates/core/src/session/display.rs`:

- `pub fn command_label(text: &str) -> Option<String>` — if `text` is a command
  echo (contains `<command-name>`), return `/<name> <args>` (e.g. `/compact`,
  `/resume`); else `None`. Reuses the tag-extraction already in `summary.rs`.
- `pub struct TaskNote { pub status: String, pub summary: String, pub result: String }`
- `pub fn parse_task_notification(text: &str) -> Option<TaskNote>` — if `text`
  trimmed starts with `<task-notification>`, extract `<status>`, `<summary>`,
  `<result>` (each may be empty); else `None`. The noisy `<task-id>`,
  `<tool-use-id>`, `<output-file>`, `<note>` are dropped.
- `pub fn one_line(text: &str) -> Option<String>` — a convenience for previews:
  command echo → `/<name> <args>`; task-notification → `task · <status> · <summary>`;
  else `None` (caller keeps the original text).

`summary.rs`'s existing `extract_tag` is moved here as `pub(crate)` and reused
(its private `strip_command_markup` can call `command_label`); no behavior change
to title generation.

### 2. Harvest outline (`surgery::render_outline`)

When building a turn's preview, if `display::one_line(preview)` is `Some`, use it
instead of the raw text. So `#4` shows `/compact` and `#9` shows
`task · completed · Agent "…" came to rest`, not tag-soup. Command echoes and
task-notifications both clean up in the outline.

### 3. Webui — collapsible task-notification

- **`render.rs`**: in the user-event arm, after the existing command-echo skip,
  if `display::parse_task_notification(&text)` is `Some(tn)`, push a **new
  `RenderedTurn::TaskNotification { timestamp, uuid, status, summary, result }`**
  variant (view-only — no edit/fork fields) instead of `RenderedTurn::User`.
  Command echoes remain skipped (unchanged).
- **`types.ts`**: add the `task_notification` variant
  (`{ type: "task_notification"; timestamp; uuid; status; summary; result }`).
- **`Transcript.tsx`**: render it as a **collapsible block** mirroring the
  tool-call pattern — a one-line header `▸ task · <status> · <summary>` (collapsed
  by default), expanding to show `result`. The summary carries
  `data-seg-id` (field `task-summary`) and, when expanded, the result carries
  `data-seg-id` (field `task-result`) so find highlights paint.
- **Find** (`lib/segments.ts` + `useFind`): `exactMatches` gains a
  `task_notification` case contributing the summary (`task-summary`) and result
  (`task-result`) to the match set; a match in a collapsed task-notification
  reveals it, reusing the existing `revealedToolKeys` reveal mechanism (the key is
  the turn's `uuid`). The block's collapsed/expanded state honors that reveal,
  exactly like collapsed tool calls today.

### 4. Memoization

The new `TaskNotification` block is rendered by a memoized component (or inside
the already-memoized `Turn`), so it does not regress the find-keystroke
memoization. Its props (status/summary/result/uuid) are stable per session.

## Non-goals

- Showing command echoes in the webui (they stay hidden — current behavior; the
  outline shows them as `/cmd`).
- Cleaning other tag types (`<local-command-stdout>`, `<system-reminder>`,
  attachments) — only command echoes and task-notifications for now.
- Editing/forking a task-notification turn (view-only).

## Testing

- **Core unit tests** (`display.rs`):
  - `command_label`: `<command-name>/compact</command-name>…` → `Some("/compact")`
    (with args → `/cmd args`); non-command text → `None`.
  - `parse_task_notification`: a real task-notification → `Some(TaskNote{status:
    "completed", summary: "…", result: "…"})`; non-task text → `None`.
  - `one_line`: command echo and task-notification each map to their compact form;
    a plain user message → `None`.
- **render.rs test**: a user event whose content is a task-notification renders as
  `RenderedTurn::TaskNotification` with the parsed status/summary/result (not a raw
  `User` turn); a command-echo user event is still skipped; a normal user message
  is still `User`.
- **Outline**: extend a surgery test so a task-notification turn's outline line
  reads `task · …` and a command echo reads `/…` (no raw tags).
- **Frontend**: `tsc` clean; existing tests stay green; add a small test for the
  task-notification branch of `exactMatches` if extracted.
- **Manual**: open a session with task-notifications → collapsible blocks with the
  summary header; expand shows the result; ⌘F finds text in the summary and (on
  match) reveals + highlights it in the result. Harvest outline shows `/cmd` and
  `task · …` lines.
