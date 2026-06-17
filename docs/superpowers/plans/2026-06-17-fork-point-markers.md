# Inline "forked from here" Markers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show an inline "⑂ N forked from here" marker in a session's transcript at the message where child sessions branched off (explicit forks and detected copies), with clickable chips that open each child.

**Architecture:** The backend exposes the parent-message uuid each derived session branched at (`forked_from_message` from `forkedFrom.messageUuid`; `copied_from_message` = the copy's head uuid). The frontend `App` builds, for the open session, a `Map<branchMessageUuid, ForkRef[]>` from the sessions list and passes it to `Transcript`, which renders a `ForkMarker` (mirroring the compaction-divider style) after any turn whose `fork_uuid`/`uuid` is a branch point.

**Tech Stack:** Rust (`uuid`, serde), React 19 + TypeScript.

## Global Constraints

- No new dependencies.
- Additive only: new `SessionSummary` fields + a new frontend component + props; no change to fork/copy creation, sidebar badges, or lineage banner.
- Field names snake_case to match existing serialization (`forked_from`, `copied_from`).
- Frontend marker must NOT regress the `Turn` memoization (the find-perf fix) — render the marker as a sibling element in the loop, never inside `Turn`.
- A branch point that matches no rendered turn is omitted inline (still shown by banner + sidebar).
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Modify** `crates/core/src/session/event.rs` — add `Event::forked_from_message()` accessor.
- **Modify** `crates/core/src/session/summary.rs` — add `forked_from_message` + `copied_from_message` fields; `summarize` sets `forked_from_message`.
- **Modify** `crates/core/src/store/mod.rs` — `discover` sets `copied_from_message` = descendant head; extend the existing lineage test.
- **Modify** `crates/webui/frontend/src/types.ts` — two fields on `SessionSummary` + a `ForkRef` interface.
- **Create** `crates/webui/frontend/src/components/ForkMarker.tsx` — the inline marker.
- **Modify** `crates/webui/frontend/src/components/Transcript.tsx` — props + render marker in the turn loop.
- **Modify** `crates/webui/frontend/src/App.tsx` — derive `branchPoints`, pass to `Transcript`.

---

## Task 1: Core — `forked_from_message` accessor + summary fields

**Files:**
- Modify: `crates/core/src/session/event.rs` (add accessor after `forked_from_session`, ~line 348)
- Modify: `crates/core/src/session/summary.rs` (struct fields + `summarize`)
- Test: inline in `summary.rs`

**Interfaces:**
- Produces: `Event::forked_from_message(&self) -> Option<&str>`; `SessionSummary.forked_from_message: Option<String>`, `SessionSummary.copied_from_message: Option<String>`.

- [ ] **Step 1: Add the `forked_from_message` accessor**

In `crates/core/src/session/event.rs`, immediately after the `forked_from_session` method (the closing `}` at ~line 348, before the `impl` block's final `}`), add:

```rust
    /// The source MESSAGE uuid this line was forked at (`forkedFrom.messageUuid`),
    /// if any. This is the branch point in the source session.
    pub fn forked_from_message(&self) -> Option<&str> {
        match self {
            Event::User(e) => e.forked_from.as_ref().and_then(|f| f.message_uuid.as_deref()),
            Event::Assistant(e) => e.forked_from.as_ref().and_then(|f| f.message_uuid.as_deref()),
            _ => None,
        }
    }
```

- [ ] **Step 2: Add the two `SessionSummary` fields**

In `crates/core/src/session/summary.rs`, add after `copied_overlap` (the field block added by the lineage feature, ~line 30):

```rust
    /// The parent MESSAGE uuid this session forked at (`forkedFrom.messageUuid`).
    /// The branch point for an explicit fork; `None` otherwise.
    pub forked_from_message: Option<String>,
    /// The parent MESSAGE uuid a detected copy branched at — the copy's head
    /// (shared root) uuid, which is a message in `copied_from`. Set by
    /// `store::discover`; `None` otherwise.
    pub copied_from_message: Option<String>,
```

- [ ] **Step 3: Write the failing summary test**

In the `#[cfg(test)] mod tests` of `summary.rs`, add:

```rust
    #[test]
    fn forked_from_message_read_from_provenance() {
        let mut u = match user("hi", false) {
            Event::User(u) => u,
            _ => unreachable!(),
        };
        u.forked_from = Some(ForkedFrom {
            session_id: "src-session".into(),
            message_uuid: Some("msg-7".into()),
        });
        let s = summarize(
            &[Event::User(u)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(s.forked_from.as_deref(), Some("src-session"));
        assert_eq!(s.forked_from_message.as_deref(), Some("msg-7"));

        // No provenance => both None.
        let plain = summarize(
            &[user("hi", false)],
            uuid::Uuid::new_v4(),
            PathBuf::from("/x.jsonl"),
            "proj".into(),
            UNIX_EPOCH,
        );
        assert_eq!(plain.forked_from_message, None);
    }
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test -p claude-manager-core forked_from_message_read_from_provenance`
Expected: FAIL to COMPILE — `forked_from_message` field/initializer not set in `summarize` yet (and the struct field unused). This confirms the test targets new behavior.

- [ ] **Step 5: Capture `forked_from_message` in `summarize`**

In `crates/core/src/session/summary.rs`, add the accumulator near the other `let mut … = None;` lines (next to `let mut forked_from`):

```rust
    let mut forked_from_message: Option<String> = None;
```

Change the forked-from capture block (the `if forked_from.is_none() { … }` at ~line 46) to also capture the message uuid:

```rust
        if forked_from.is_none() {
            if let Some(src) = ev.forked_from_session() {
                forked_from = Some(src.to_string());
                forked_from_message = ev.forked_from_message().map(|s| s.to_string());
            }
        }
```

And in the `SessionSummary { … }` constructor, add after `copied_overlap: None,`:

```rust
        forked_from_message,
        copied_from_message: None,
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p claude-manager-core summary`
Expected: PASS — `forked_from_message_read_from_provenance` plus all existing `summary::tests::*`.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/session/event.rs crates/core/src/session/summary.rs
git commit -m "feat(core): expose forked_from_message branch point on SessionSummary

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Core — `discover` sets `copied_from_message`

**Files:**
- Modify: `crates/core/src/store/mod.rs` (lineage stamping loop + extend the existing lineage test)

**Interfaces:**
- Consumes: `SessionSummary.copied_from_message` (Task 1); `lineage::SessionUuids.head` (existing).

- [ ] **Step 1: Extend the discover test to assert `copied_from_message`**

In `crates/core/src/store/mod.rs`, in the existing test `discover_sets_copied_from_via_shared_uuid`, add after the existing `copied_overlap` assertion:

```rust
        // B's branch point in A = B's head (its first message = the shared root).
        assert_eq!(b.copied_from_message.as_deref(), Some(shared), "B's branch point is its head");
        assert_eq!(a.copied_from_message, None);
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p claude-manager-core discover_sets_copied_from_via_shared_uuid`
Expected: FAIL — `copied_from_message` is `None` (discover doesn't set it yet).

- [ ] **Step 3: Set `copied_from_message` in the discover stamping loop**

In `crates/core/src/store/mod.rs`, replace the lineage-stamping loop (the one added by the previous feature):

```rust
        // Resolve copy-lineage within this directory and stamp the summaries.
        let links = lineage::copy_lineage(&dir_uuids);
        for summary in dir_summaries.iter_mut() {
            if let Some((ancestor_id, overlap)) = links.get(&summary.id) {
                summary.copied_from = Some(ancestor_id.to_string());
                summary.copied_overlap = Some(*overlap);
            }
        }
        sessions.extend(dir_summaries);
```

with the index-based version that also records the branch point (the descendant's head):

```rust
        // Resolve copy-lineage within this directory and stamp the summaries.
        // `dir_summaries[i]` and `dir_uuids[i]` are the same session (pushed
        // together), so the descendant's head — its branch point in the ancestor
        // — is `dir_uuids[i].head`.
        let links = lineage::copy_lineage(&dir_uuids);
        for (i, summary) in dir_summaries.iter_mut().enumerate() {
            if let Some((ancestor_id, overlap)) = links.get(&summary.id) {
                summary.copied_from = Some(ancestor_id.to_string());
                summary.copied_overlap = Some(*overlap);
                summary.copied_from_message = dir_uuids[i].head.map(|h| h.to_string());
            }
        }
        sessions.extend(dir_summaries);
```

- [ ] **Step 4: Run the test + full core suite**

Run: `cargo test -p claude-manager-core`
Expected: PASS — `discover_sets_copied_from_via_shared_uuid` (with the new assertions) and all existing core tests.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/store/mod.rs
git commit -m "feat(core): discover records copied_from_message (copy branch point)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Frontend — inline ForkMarker

**Files:**
- Modify: `crates/webui/frontend/src/types.ts`
- Create: `crates/webui/frontend/src/components/ForkMarker.tsx`
- Modify: `crates/webui/frontend/src/components/Transcript.tsx`
- Modify: `crates/webui/frontend/src/App.tsx`

**Interfaces:**
- Consumes: `SessionSummary.forked_from`, `forked_from_message`, `copied_from`, `copied_from_message` (Tasks 1-2).

- [ ] **Step 1: Add the TS fields + `ForkRef` type**

In `crates/webui/frontend/src/types.ts`, add to `SessionSummary` (after `copied_overlap`):

```ts
  /// The parent message uuid this session forked at (explicit fork branch point).
  forked_from_message: string | null;
  /// The parent message uuid a detected copy branched at (its shared-root head).
  copied_from_message: string | null;
```

And add an exported interface (after the `SessionSummary` interface):

```ts
/// A child session that branched from a given message of the open session.
export interface ForkRef {
  session: SessionSummary;
  kind: "forked" | "copied";
}
```

- [ ] **Step 2: Create the ForkMarker component**

Create `crates/webui/frontend/src/components/ForkMarker.tsx`:

```tsx
import type { ForkRef, SessionSummary } from "../types";

export default function ForkMarker({
  refs,
  onOpen,
}: {
  refs: ForkRef[];
  onOpen: (s: SessionSummary) => void;
}) {
  return (
    <div className="flex items-center gap-3 my-3">
      <div className="flex-1 border-t border-[var(--color-border)]" />
      <div className="flex flex-wrap items-center justify-center gap-2">
        <span className="text-[10px] uppercase tracking-wider text-[var(--color-text-dim)] whitespace-nowrap">
          ⑂ {refs.length} forked from here
        </span>
        {refs.map(({ session, kind }) => (
          <button
            key={session.id}
            onClick={() => onOpen(session)}
            title={`Open ${session.title}`}
            className="max-w-[16rem] truncate text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
          >
            {session.title}
            {kind === "copied" ? " (copy)" : ""}
          </button>
        ))}
      </div>
      <div className="flex-1 border-t border-[var(--color-border)]" />
    </div>
  );
}
```

- [ ] **Step 3: Wire ForkMarker into Transcript**

In `crates/webui/frontend/src/components/Transcript.tsx`:

(a) Add `Fragment` to the React import (line 1):

```tsx
import { useState, useRef, useEffect, useCallback, memo, Fragment } from "react";
```

(b) Add `ForkRef` and `SessionSummary` to the type import (the `import type { … } from "../types";` block, ~lines 4-9):

```tsx
import type {
  RenderedSession,
  RenderedTurn,
  RenderedBlock,
  SessionStats,
  ForkRef,
  SessionSummary,
} from "../types";
```

(c) Add the ForkMarker import (after the `FindBar` import, ~line 13):

```tsx
import ForkMarker from "./ForkMarker";
```

(d) Extend `Props` (the `interface Props { session; onBack }`, ~lines 18-21):

```tsx
interface Props {
  session: RenderedSession;
  onBack: () => void;
  branchPoints?: Map<string, ForkRef[]>;
  onOpenSession?: (s: SessionSummary) => void;
}
```

(e) Destructure the new props in the component signature (`export default function Transcript({ session, onBack }: Props)`):

```tsx
export default function Transcript({ session, onBack, branchPoints, onOpenSession }: Props) {
```

(f) Replace the `visibleTurns.map(…)` block (lines ~168-191) so each iteration also renders a marker after the turn:

```tsx
        {visibleTurns.map((turn, i) => {
          const editUuid =
            (turn.type === "user" || turn.type === "assistant") && turn.editable
              ? turn.edit_uuid
              : null;
          const editing = editUuid !== null && editingUuid === editUuid;
          const forkUuid =
            turn.type === "user" || turn.type === "assistant" ? turn.fork_uuid : null;
          const ownUuid =
            turn.type === "user" || turn.type === "assistant" ? turn.uuid : null;
          let marks: ForkRef[] | undefined;
          if (branchPoints) {
            if (forkUuid) marks = branchPoints.get(forkUuid);
            if (!marks && ownUuid) marks = branchPoints.get(ownUuid);
          }
          return (
            <Fragment key={i}>
              <Turn
                turn={turn}
                turnIndex={i}
                editUuid={editUuid}
                revealedToolKeys={find.revealedToolKeys}
                editing={editing}
                draft={editing ? draft : ""}
                onStartEdit={handleStartEdit}
                onChangeDraft={setDraft}
                onCancelEdit={handleCancelEdit}
                onSaveEdit={handleSaveEdit}
                onFork={handleFork}
                onDelete={handleDelete}
              />
              {marks && onOpenSession && (
                <ForkMarker refs={marks} onOpen={onOpenSession} />
              )}
            </Fragment>
          );
        })}
```

(Note: the `key` moves from `<Turn key={i}>` to `<Fragment key={i}>`.)

- [ ] **Step 4: Derive branchPoints in App and pass them**

In `crates/webui/frontend/src/App.tsx`:

(a) Add the `ForkRef` type import — change the type-less component imports area; add near the top with the other imports:

```tsx
import type { ForkRef } from "./types";
```

(b) In the `transcript ?` IIFE (added for the lineage banner), compute `branchPoints` and pass the two new props to `Transcript`. Replace the IIFE body so it reads:

```tsx
          (() => {
            const openSummary = sessions.find((s) => s.id === transcript.id) ?? null;
            const copiedFrom = openSummary?.copied_from
              ? sessions.find((s) => s.id === openSummary.copied_from) ?? null
              : null;
            const copies = sessions.filter((s) => s.copied_from === transcript.id);
            const branchPoints = new Map<string, ForkRef[]>();
            const addBranch = (msg: string, ref: ForkRef) => {
              const arr = branchPoints.get(msg);
              if (arr) arr.push(ref);
              else branchPoints.set(msg, [ref]);
            };
            for (const s of sessions) {
              if (s.forked_from === transcript.id && s.forked_from_message) {
                addBranch(s.forked_from_message, { session: s, kind: "forked" });
              }
              if (s.copied_from === transcript.id && s.copied_from_message) {
                addBranch(s.copied_from_message, { session: s, kind: "copied" });
              }
            }
            return (
              <>
                <LineageBanner
                  copiedFrom={copiedFrom}
                  copies={copies}
                  overlap={openSummary?.copied_overlap ?? null}
                  onOpen={(s) => open(s.file_path, s.project_label)}
                />
                <div className="flex-1 overflow-hidden">
                  <Transcript
                    session={transcript}
                    onBack={close}
                    branchPoints={branchPoints}
                    onOpenSession={(s) => open(s.file_path, s.project_label)}
                  />
                </div>
              </>
            );
          })()
```

- [ ] **Step 5: Type-check, test, build**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test && npm run build`
Expected: `tsc` clean; 14 tests PASS; `vite build` succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/types.ts crates/webui/frontend/src/components/ForkMarker.tsx crates/webui/frontend/src/components/Transcript.tsx crates/webui/frontend/src/App.tsx
git commit -m "feat(webui): inline forked-from-here marker linking child sessions

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS (core incl. the new summary test + extended discover test; webui; TUI), 0 failures.

- [ ] **Step 2: Frontend gate**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test && npm run build`
Expected: `tsc` clean; 14 tests PASS; build succeeds.

- [ ] **Step 3: Manual verification (needs the running app — hand to the user; do not launch it)**

Provide these for the user to run after `make webui`:
1. Open a session that has a child forked from a specific message (one of the user's real "fork from here" sessions). Expect an inline **"⑂ 1 forked from here"** line just after that turn, with a chip that opens the child.
2. If multiple children forked from the same message, expect one marker listing all of them.
3. Open `81375432` — if the copy's head matches a rendered turn, a marker with a "(copy)" chip to `91052458` appears there; otherwise the copy stays surfaced via the banner/sidebar only (acceptable per spec).
4. Confirm find (⌘F) typing is still smooth (marker must not regress `Turn` memoization).

- [ ] **Step 4: Report results**

Report exact `cargo test` and frontend counts + the user's manual outcomes. Do not claim manual checks pass without confirmation. Do not merge — the user directs all merges.

---

## Notes for the implementer

- The marker is a **sibling** of `<Turn>` in the loop (inside a `<Fragment>`), never a prop/child of `Turn` — this preserves the find-perf memoization.
- Match `fork_uuid` first, then `uuid`. A branch point matching no rendered turn is simply not shown inline (the relationship remains in the lineage banner + sidebar).
- `copied_from_message` is the descendant's head (its first message). For a compaction-rooted copy this head may be a system `compact_boundary` (not a rendered user/assistant turn), in which case the inline copy-marker is correctly omitted.
- Do not change fork/copy creation, the sidebar badges, or the lineage banner.
