# Clean Noisy Messages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render slash-command echoes and background task-notifications as clean signal — `/cmd` and a collapsible `task · status · summary` — in the harvest outline and the webui, instead of raw tag-soup.

**Architecture:** A core `session::display` module parses both message kinds. The harvest outline one-lines them; `render.rs` emits a new `RenderedTurn::TaskNotification` variant for task-notifications, which the webui renders as a collapsible block (find reveals it on match, like tool calls). Command echoes stay hidden in the webui (current behavior) and become `/cmd` in the outline.

**Tech Stack:** Rust (`serde`, `chrono`), React 19 + TypeScript.

## Global Constraints

- No new dependencies.
- Command echoes stay hidden in the webui (unchanged); only the outline shows them as `/cmd`. Task-notifications are cleaned in both.
- The new `TaskNotification` turn is view-only (no edit/fork).
- Must not regress find: the task-notification summary is always findable; a match in the (collapsed) result reveals it, reusing the existing `revealedToolKeys` mechanism.
- Must not regress the `Turn` memoization (find-perf): the new block renders inside the already-memoized `Turn`.
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Create** `crates/core/src/session/display.rs` — `extract_tag` (moved here), `command_label`, `TaskNote`, `parse_task_notification`, `one_line` + tests.
- **Modify** `crates/core/src/session/mod.rs` — `pub mod display;`.
- **Modify** `crates/core/src/session/summary.rs` — use `display::extract_tag` (drop the local copy).
- **Modify** `crates/core/src/session/render.rs` — `RenderedTurn::TaskNotification` variant; emit it for task-notification user events.
- **Modify** `crates/core/src/session/surgery.rs` — outline preview uses `display::one_line`.
- **Modify** `crates/webui/frontend/src/types.ts` — `task_notification` variant + `SegField`.
- **Modify** `crates/webui/frontend/src/components/Transcript.tsx` — `Turn` renders the collapsible task-notification.
- **Modify** `crates/webui/frontend/src/lib/segments.ts` — `exactMatches` case.
- **Modify** `crates/webui/frontend/src/hooks/useFind.ts` — reveal on `task-result` match.

---

## Task 1: Core — `session::display` parsing

**Files:**
- Create: `crates/core/src/session/display.rs`
- Modify: `crates/core/src/session/mod.rs`, `crates/core/src/session/summary.rs`
- Test: inline in `display.rs`

**Interfaces:**
- Produces: `display::command_label(&str) -> Option<String>`; `display::TaskNote { status, summary, result }`; `display::parse_task_notification(&str) -> Option<TaskNote>`; `display::one_line(&str) -> Option<String>`; `pub(crate) display::extract_tag(&str, &str) -> Option<&str>`.

- [ ] **Step 1: Create `display.rs` with implementation + tests**

Create `crates/core/src/session/display.rs`:

```rust
//! Cleaning of injected "noise" user messages for display: slash-command echoes
//! and background task-notifications. Extract the signal, drop the boilerplate.

/// Inner text of the first `<tag>…</tag>` in `s`, if present.
pub(crate) fn extract_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = s.find(&open)? + open.len();
    let j = s[i..].find(&close)? + i;
    Some(&s[i..j])
}

/// A slash-command echo (`<command-name>/x</command-name> <command-args>…`) as
/// `/x args`. `None` if `text` is not a command echo.
pub fn command_label(text: &str) -> Option<String> {
    if !text.contains("<command-name>") {
        return None;
    }
    let name = extract_tag(text, "command-name").map(str::trim)?;
    let args = extract_tag(text, "command-args").map(str::trim);
    Some(match args {
        Some(a) if !a.is_empty() => format!("{} {}", name, a),
        _ => name.to_string(),
    })
}

/// Parsed background task-notification (signal only).
pub struct TaskNote {
    pub status: String,
    pub summary: String,
    pub result: String,
}

/// Parse a `<task-notification>…</task-notification>` message. `None` if not one.
pub fn parse_task_notification(text: &str) -> Option<TaskNote> {
    if !text.trim_start().starts_with("<task-notification>") {
        return None;
    }
    let tag = |t: &str| extract_tag(text, t).unwrap_or("").trim().to_string();
    Some(TaskNote { status: tag("status"), summary: tag("summary"), result: tag("result") })
}

/// Compact one-line form for previews: command echo → `/cmd args`;
/// task-notification → `task · <status> · <summary>`; else `None` (keep original).
pub fn one_line(text: &str) -> Option<String> {
    if let Some(cmd) = command_label(text) {
        return Some(cmd);
    }
    parse_task_notification(text).map(|tn| format!("task · {} · {}", tn.status, tn.summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_label_extracts_name_and_args() {
        let echo = "<command-name>/compact</command-name>\n  <command-message>compact</command-message>\n  <command-args>foo bar</command-args>";
        assert_eq!(command_label(echo).as_deref(), Some("/compact foo bar"));
        let no_args = "<command-name>/resume</command-name>\n<command-args></command-args>";
        assert_eq!(command_label(no_args).as_deref(), Some("/resume"));
        assert_eq!(command_label("just a normal message"), None);
    }

    #[test]
    fn parse_task_notification_extracts_signal() {
        let tn = "<task-notification>\n<task-id>abc</task-id>\n<tool-use-id>t</tool-use-id>\n<output-file>/tmp/x</output-file>\n<status>completed</status>\n<summary>Agent did a thing</summary>\n<result>the full result body</result>\n</task-notification>";
        let n = parse_task_notification(tn).expect("parses");
        assert_eq!(n.status, "completed");
        assert_eq!(n.summary, "Agent did a thing");
        assert_eq!(n.result, "the full result body");
        assert!(parse_task_notification("hello").is_none());
    }

    #[test]
    fn one_line_compacts_both_kinds() {
        assert_eq!(
            one_line("<command-name>/compact</command-name>").as_deref(),
            Some("/compact")
        );
        let tn = "<task-notification>\n<status>completed</status>\n<summary>did X</summary>\n<result>r</result>\n</task-notification>";
        assert_eq!(one_line(tn).as_deref(), Some("task · completed · did X"));
        assert_eq!(one_line("a normal message"), None);
    }
}
```

- [ ] **Step 2: Declare the module + reuse `extract_tag` in summary.rs**

In `crates/core/src/session/mod.rs`, add (keep alphabetical-ish with the others):

```rust
pub mod display;
```

In `crates/core/src/session/summary.rs`, delete its local `extract_tag` function and import the shared one. Add near the top `use super::display::extract_tag;`, and remove the `fn extract_tag<'a>(...)` definition (the `strip_command_markup` function keeps calling `extract_tag` — now the imported one).

- [ ] **Step 3: Run the tests**

Run: `cargo test -p claude-manager-core display && cargo test -p claude-manager-core summary`
Expected: PASS — the 3 new `display::tests::*` and all existing `summary::tests::*` (summary still compiles using the shared `extract_tag`).

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/session/display.rs crates/core/src/session/mod.rs crates/core/src/session/summary.rs
git commit -m "feat(core): session::display — parse command echoes & task-notifications

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Core — `RenderedTurn::TaskNotification`

**Files:**
- Modify: `crates/core/src/session/render.rs`
- Test: inline in `render.rs`

**Interfaces:**
- Consumes: `display::parse_task_notification` (Task 1).
- Produces: `RenderedTurn::TaskNotification { timestamp, uuid, status, summary, result }`.

- [ ] **Step 1: Add the variant**

In `crates/core/src/session/render.rs`, add to the `RenderedTurn` enum (after the `Summary` variant, before `Compaction`):

```rust
    #[serde(rename = "task_notification")]
    TaskNotification {
        timestamp: Option<DateTime<Utc>>,
        uuid: String,
        status: String,
        summary: String,
        result: String,
    },
```

- [ ] **Step 2: Write the failing test**

In `render.rs` tests, add (constructing a user event whose text is a task-notification; mirror the existing user-event construction in this file's tests):

```rust
    #[test]
    fn task_notification_user_event_becomes_task_turn() {
        use crate::session::event::{Message, MessageContent, UserEvent};
        let tn = "<task-notification>\n<status>completed</status>\n<summary>did X</summary>\n<result>body</result>\n</task-notification>";
        let ev = Event::User(UserEvent {
            uuid: Some("00000000-0000-0000-0000-0000000000a1".parse().unwrap()),
            parent_uuid: None,
            timestamp: Some(chrono::Utc::now()),
            is_sidechain: false,
            is_meta: None,
            message: Message { role: Some("user".into()), content: MessageContent::Text(tn.into()) },
            tool_use_result: None,
            cwd: None,
            git_branch: None,
            forked_from: None,
        });
        let turns = render_path_turns(&[&ev], true);
        assert_eq!(turns.len(), 1);
        match &turns[0] {
            RenderedTurn::TaskNotification { status, summary, result, .. } => {
                assert_eq!(status, "completed");
                assert_eq!(summary, "did X");
                assert_eq!(result, "body");
            }
            other => panic!("expected TaskNotification, got {other:?}"),
        }
    }
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p claude-manager-core task_notification_user_event_becomes_task_turn`
Expected: FAIL — the user event renders as `RenderedTurn::User`, not `TaskNotification`.

- [ ] **Step 4: Emit the variant in `render_path_turns`**

In `crates/core/src/session/render.rs`, in the `Event::User(u)` arm, right after the command-echo skip block (`if text.starts_with("<command-") || text.starts_with("<local-command-") { i += 1; continue; }`) and before `let idx = msg_index[i]...`, insert:

```rust
                if let Some(tn) = crate::session::display::parse_task_notification(&text) {
                    let uuid = ev.uuid().map(|u| u.to_string()).unwrap_or_default();
                    turns.push(RenderedTurn::TaskNotification {
                        timestamp: ev.timestamp(),
                        uuid,
                        status: tn.status,
                        summary: tn.summary,
                        result: tn.result,
                    });
                    i += 1;
                    continue;
                }
```

- [ ] **Step 5: Run to verify it passes + full core**

Run: `cargo test -p claude-manager-core`
Expected: PASS — the new test plus all existing core tests (the new variant is additive; the stats filter uses `matches!(… User | Assistant)` so task-notifications are correctly excluded from message-turn counts/stats).

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/session/render.rs
git commit -m "feat(core): render task-notifications as a TaskNotification turn

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Outline cleaning (harvest)

**Files:**
- Modify: `crates/core/src/session/surgery.rs`
- Test: inline in `surgery.rs`

**Interfaces:**
- Consumes: `display::one_line` (Task 1).

- [ ] **Step 1: Use `one_line` for the outline preview**

In `crates/core/src/session/surgery.rs` `render_outline`, the preview is built as `let prev = preview.replace('\n', " ");` then `ellipsis(prev.trim(), …)`. Replace that preview-finalizing code:

```rust
        let prev = preview.replace('\n', " ");
        let prev = ellipsis(prev.trim(), OUTLINE_PREVIEW_CAP);
```

with a version that compacts command echoes / task-notifications first:

```rust
        let cleaned = super::display::one_line(&preview).unwrap_or_else(|| preview.replace('\n', " "));
        let prev = ellipsis(cleaned.trim(), OUTLINE_PREVIEW_CAP);
```

- [ ] **Step 2: Add a test**

In `surgery.rs` tests, add a fixture + assertions:

```rust
    fn noisy_fixture() -> PathBuf {
        let body = concat!(
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000a1","parentUuid":null,"message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args></command-args>"}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-0000-0000-0000000000b2","parentUuid":"00000000-0000-0000-0000-0000000000a1","message":{"role":"user","content":"<task-notification>\n<status>completed</status>\n<summary>did the thing</summary>\n<result>body</result>\n</task-notification>"}}"#, "\n",
        );
        let mut p = std::env::temp_dir();
        p.push(format!("cm-surg-noisy-{}.jsonl", std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn outline_compacts_command_echo_and_task_notification() {
        let p = noisy_fixture();
        let text = extract(&p, None, None, true, false).unwrap();
        assert!(text.contains("/compact"), "command echo → /compact");
        assert!(text.contains("task · completed · did the thing"), "task-notif → one line");
        assert!(!text.contains("<command-name>"), "no raw command tags");
        assert!(!text.contains("<task-notification>"), "no raw task tags");
        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 3: Run + commit**

Run: `cargo test -p claude-manager-core surgery`
Expected: PASS — `outline_compacts_command_echo_and_task_notification` plus existing surgery tests.

```bash
git add crates/core/src/session/surgery.rs
git commit -m "feat(core): outline compacts command echoes & task-notifications

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Webui — collapsible task-notification + find

**Files:**
- Modify: `crates/webui/frontend/src/types.ts`, `components/Transcript.tsx`, `lib/segments.ts`, `hooks/useFind.ts`

**Interfaces:**
- Consumes: the `task_notification` `RenderedTurn` variant (Task 2).

- [ ] **Step 1: Add the TS variant + SegField**

In `crates/webui/frontend/src/types.ts`, add to the `RenderedTurn` union (after the `summary` variant):

```ts
  | {
      type: "task_notification";
      timestamp: string | null;
      uuid: string;
      status: string;
      summary: string;
      result: string;
    }
```

In `crates/webui/frontend/src/lib/segments.ts`, extend the `SegField` union with `"task-summary"` and `"task-result"`:

```ts
export type SegField =
  | "user-text"
  | "assistant-text"
  | "system"
  | "summary"
  | "tool-name"
  | "tool-input"
  | "tool-result"
  | "task-summary"
  | "task-result";
```

- [ ] **Step 2: exactMatches covers the new variant**

In `crates/webui/frontend/src/lib/segments.ts`, in `exactMatches`'s `switch (turn.type)`, add a case (alongside `system`/`summary`):

```ts
      case "task_notification":
        push(ti, null, "task-summary", turn.summary);
        push(ti, null, "task-result", turn.result);
        break;
```

- [ ] **Step 3: Reveal a collapsed task-notification on a result match**

In `crates/webui/frontend/src/hooks/useFind.ts`, extend the reveal effect. Replace:

```ts
    if (active.field === "tool-input" || active.field === "tool-result") {
      const key = `${active.turnIndex}:${active.blockIndex}`;
      setRevealedToolKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
    }
```

with:

```ts
    if (active.field === "tool-input" || active.field === "tool-result") {
      const key = `${active.turnIndex}:${active.blockIndex}`;
      setRevealedToolKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
    } else if (active.field === "task-result") {
      const key = `${active.turnIndex}:task`;
      setRevealedToolKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
    }
```

- [ ] **Step 4: Render the collapsible block in `Turn`**

In `crates/webui/frontend/src/components/Transcript.tsx`, add a `TaskNotificationBlock` component (after the `Turn` component, near `BlockRenderer`):

```tsx
const TaskNotificationBlock = memo(function TaskNotificationBlock({
  turnIndex,
  status,
  summary,
  result,
  forceExpand,
}: {
  turnIndex: number;
  status: string;
  summary: string;
  result: string;
  forceExpand: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const open = expanded || forceExpand;
  return (
    <div className="my-1 text-sm">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-baseline text-left w-full hover:bg-[var(--color-surface-hover)] rounded px-2 py-1 transition-colors text-[var(--color-text-dim)]"
      >
        <span className="w-4 shrink-0 text-center">{open ? "▾" : "▸"}</span>
        <span className="shrink-0 mr-1.5">task · {status} ·</span>
        <span data-seg-id={makeSegId(turnIndex, null, "task-summary")} className="truncate min-w-0">
          {summary}
        </span>
      </button>
      {open && result && (
        <pre
          data-seg-id={makeSegId(turnIndex, null, "task-result")}
          className="ml-4 px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 border-l-[var(--color-border)] bg-[var(--color-tool-result-bg)] text-[var(--color-text-dim)]"
          style={{ maxHeight: "400px", overflowY: "auto" }}
        >
          {result}
        </pre>
      )}
    </div>
  );
});
```

Then add a `case "task_notification"` to the `switch (turn.type)` in the `Turn` component (alongside `system`/`summary`):

```tsx
    case "task_notification":
      return (
        <TaskNotificationBlock
          turnIndex={turnIndex}
          status={turn.status}
          summary={turn.summary}
          result={turn.result}
          forceExpand={revealedToolKeys.has(`${turnIndex}:task`)}
        />
      );
```

(`Turn` already receives `revealedToolKeys` and `turnIndex`.)

- [ ] **Step 5: Type-check, test, build**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test && npm run build`
Expected: `tsc` clean (the `RenderedTurn` union is exhaustively handled in `Turn` and `exactMatches`); 14 tests pass; `vite build` succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/types.ts crates/webui/frontend/src/components/Transcript.tsx crates/webui/frontend/src/lib/segments.ts crates/webui/frontend/src/hooks/useFind.ts
git commit -m "feat(webui): collapsible task-notification turn + find integration

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS (core incl. display, render task-notif, outline-compact tests; webui; TUI), 0 failures.

- [ ] **Step 2: Frontend gate**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test && npm run build`
Expected: `tsc` clean; 14 tests pass; build succeeds.

- [ ] **Step 3: Real-session outline smoke**

Build (`cargo build --release`), then on a session with command echoes + task-notifications (e.g. `c84ba790-92b6-41ea-881f-971902858ce2`):

```bash
BIN=/Users/zillydev/my/Dev/Personal/claude-manager/target/release/claude-manager
S=c84ba790-92b6-41ea-881f-971902858ce2
"$BIN" surgery extract --session "$S" --outline --all-segments | grep -E '/compact|/resume|task · ' | head
"$BIN" surgery extract --session "$S" --outline --all-segments | grep -c '<task-notification>'
```
Expected: lines like `/compact` and `task · completed · …` appear; the raw `<task-notification>` count is `0`.

- [ ] **Step 4: Manual webui check (hand to the user; do not launch the app)**

Have the user open a session with task-notifications → each renders as a collapsed `▸ task · status · summary`; expand shows the result; ⌘F matches the summary always and (on a match in the result) reveals + highlights it.

- [ ] **Step 5: Report**

Report exact `cargo test` + frontend counts and the outline smoke output. Do not claim the webui check passes without the user's confirmation. Do not merge — the user directs merges.

---

## Notes for the implementer

- Command echoes stay hidden in the webui (the existing `render.rs` skip is untouched); only the outline shows them as `/cmd`.
- The `TaskNotification` turn is view-only — it has no edit/fork uuids and is excluded from message-turn counts by the existing `matches!(… User | Assistant)` filter.
- The result reveal reuses `revealedToolKeys` with key `${turnIndex}:task` (distinct from the tool key `${turnIndex}:${blockIndex}`).
- Keep the block inside the memoized `Turn` (don't add a new top-level non-memoized turn loop element) so the find-keystroke memoization isn't regressed.
