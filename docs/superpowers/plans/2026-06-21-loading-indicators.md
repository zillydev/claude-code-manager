# Loading Indicators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every async webui action inline, per-action loading feedback (spinner + disabled state) so nothing feels unresponsive.

**Architecture:** One reusable `<Spinner>` atom (pure CSS, `currentColor`-tinted). Each call site adds small local busy state and renders the atom on its own control. No global progress bar, no toasts, no backend changes.

**Tech Stack:** React 19 + TypeScript + Tailwind v4 (`animate-spin` built in). Vitest (pure-function tests only).

## Global Constraints

- **Frontend-only.** No Rust/`cargo` changes — every gap is a rendering gap.
- **Inline style only** — per-control spinner + `disabled`; no global mechanism, no toasts.
- **Spinner tints to context** via `border-current` (inherits text color).
- **Out of scope:** success/error toasts. Existing `console.error` / inline `error` / `CommitResultBanner` stay as-is.
- **Verification per task:** `npx tsc --noEmit` clean; existing `npx vitest run` stays green (20 tests); manual in-app check. No new test infra (project has no jsdom / @testing-library; the stateful changes mirror existing patterns and are verified by tsc + manual).

---

### Task 1: The `Spinner` atom

**Files:**
- Create: `crates/webui/frontend/src/components/Spinner.tsx`

**Interfaces:**
- Produces: `export default function Spinner({ size?: "sm" | "md"; className?: string }): JSX.Element`

- [ ] **Step 1: Create the component**

```tsx
// crates/webui/frontend/src/components/Spinner.tsx
export default function Spinner({
  size = "sm",
  className = "",
}: {
  size?: "sm" | "md";
  className?: string;
}) {
  const dims = size === "sm" ? "h-3.5 w-3.5" : "h-6 w-6";
  return (
    <span
      role="status"
      aria-label="Loading"
      className={`inline-block animate-spin rounded-full border-2 border-current border-t-transparent ${dims} ${className}`}
    />
  );
}
```

- [ ] **Step 2: Typecheck**

Run: `cd crates/webui/frontend && npx tsc --noEmit`
Expected: no errors (file unused yet is fine — it's a default export, no unused-var error).

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/components/Spinner.tsx
git commit -m "feat(webui): add reusable Spinner atom"
```

---

### Task 2: Editor op spinners (fork / edit-save / delete-message)

The user's flagged pain: `useEditor` sets a busy flag that is never rendered. Add a `pending` target to the hook and show a spinner on the exact button clicked.

**Files:**
- Modify: `crates/webui/frontend/src/hooks/useEditor.ts`
- Modify: `crates/webui/frontend/src/components/Transcript.tsx`

**Interfaces:**
- Produces (`useEditor`): return value gains `pending: { kind: "fork" | "edit" | "delete"; uuid: string } | null` (and keeps `busy: boolean` as a derived convenience). Exports `type EditorPending`.
- Consumes (`Transcript`): `editor.pending`; `Spinner` from Task 1.

- [ ] **Step 1: Add `pending` to `useEditor`**

Replace the whole body of `crates/webui/frontend/src/hooks/useEditor.ts` with:

```tsx
import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CommitResult } from "../types";

export type EditorPending = { kind: "fork" | "edit" | "delete"; uuid: string } | null;

export function useEditor() {
  const [result, setResult] = useState<CommitResult | null>(null);
  const [pending, setPending] = useState<EditorPending>(null);
  const [error, setError] = useState<string | null>(null);

  const commitEdit = useCallback(
    async (filePath: string, editUuid: string, newText: string) => {
      setPending({ kind: "edit", uuid: editUuid });
      setError(null);
      try {
        const res = await invoke<CommitResult>("commit_session", {
          filePath,
          editUuid,
          newText,
          customTitle: null,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setPending(null);
      }
    },
    []
  );

  const fork = useCallback(
    async (filePath: string, forkUuid: string) => {
      setPending({ kind: "fork", uuid: forkUuid });
      setError(null);
      try {
        const res = await invoke<CommitResult>("fork_session", {
          filePath,
          forkUuid,
          customTitle: null,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setPending(null);
      }
    },
    []
  );

  const deleteMessage = useCallback(
    async (filePath: string, messageUuid: string) => {
      setPending({ kind: "delete", uuid: messageUuid });
      setError(null);
      try {
        const res = await invoke<CommitResult>("delete_message", {
          filePath,
          messageUuid,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setPending(null);
      }
    },
    []
  );

  const dismiss = useCallback(() => setResult(null), []);

  return { result, pending, busy: pending !== null, error, commitEdit, fork, deleteMessage, dismiss };
}
```

- [ ] **Step 2: Verify no other consumer broke**

Run: `cd crates/webui/frontend && grep -rn "\.busy" src | grep -v test`
Expected: no matches outside `useEditor.ts` (confirms removing the standalone `busy` state is safe; the derived `busy` in the return is kept anyway).

- [ ] **Step 3: Thread `pending` into the Transcript turn map**

In `crates/webui/frontend/src/components/Transcript.tsx`, add the `Spinner` import at the top (next to the other component imports):

```tsx
import Spinner from "./Spinner";
```

In the `visibleTurns.map((turn, i) => { ... })` block (around line 220), after the `ownUuid` computation and before `let marks`, add:

```tsx
          const forkPending =
            editor.pending?.kind === "fork" && editor.pending.uuid === forkUuid;
          const editPending =
            editor.pending?.kind === "edit" && editor.pending.uuid === editUuid;
          const deletePending =
            editor.pending?.kind === "delete" && editor.pending.uuid === ownUuid;
```

Then pass them to `<Turn ... />` (add after the `onDelete={handleDelete}` prop):

```tsx
                forkPending={!!forkPending}
                editPending={!!editPending}
                deletePending={!!deletePending}
```

- [ ] **Step 4: Add the props to `Turn` and forward them**

In the `Turn = memo(function Turn({ ... })` signature (around line 385) add `forkPending`, `editPending`, `deletePending` to the destructured params and to the prop type:

```tsx
  onFork,
  onDelete,
  forkPending,
  editPending,
  deletePending,
}: {
  // ...existing prop types...
  onFork: (forkUuid: string) => void;
  onDelete: (uuid: string) => void;
  forkPending: boolean;
  editPending: boolean;
  deletePending: boolean;
}) {
```

Pass to both `<TurnActions ... />` (the `user` and `assistant` cases) — add after `canMutate={turn.editable}`:

```tsx
              forkPending={forkPending}
              deletePending={deletePending}
```

Pass to both `<EditBox ... />` (user and assistant cases) — add after `onCancel={onCancelEdit}`:

```tsx
              saving={editPending}
```

- [ ] **Step 5: Render spinners in `TurnActions`**

In `function TurnActions({ ... })` (around line 539) add the two props:

```tsx
function TurnActions({
  onEdit,
  onFork,
  onDelete,
  messageId,
  canMutate,
  forkPending,
  deletePending,
}: {
  onEdit: () => void;
  onFork: () => void;
  onDelete: () => void;
  messageId: string;
  canMutate: boolean;
  forkPending: boolean;
  deletePending: boolean;
}) {
```

Add `disabled:opacity-50` to the shared `btn` class string:

```tsx
  const btn =
    "text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-50";
```

Replace the fork button with:

```tsx
      <button
        onClick={onFork}
        disabled={forkPending || deletePending}
        className={btn}
        title="Create a new session ending at this message"
      >
        {forkPending ? <Spinner /> : "fork from here"}
      </button>
```

Replace the non-confirming delete button (the `) : (` branch that renders "delete") with:

```tsx
          <button
            onClick={() => setConfirming(true)}
            disabled={deletePending}
            className={btn}
            title="Delete this message (writes a new session without it)"
          >
            {deletePending ? <Spinner /> : "delete"}
          </button>
```

(The spinner appears on the main "delete" button after the user clicks "yes" — that handler already calls `setConfirming(false)`, returning to this branch while the op runs.)

- [ ] **Step 6: Render the spinner in `EditBox`**

In `function EditBox({ ... })` (around line 620) add a `saving` prop:

```tsx
function EditBox({
  draft,
  onChange,
  onSave,
  onCancel,
  saving,
}: {
  draft: string;
  onChange: (s: string) => void;
  onSave: () => void;
  onCancel: () => void;
  saving: boolean;
}) {
```

Replace the Save button with:

```tsx
        <button
          onClick={onSave}
          disabled={saving}
          className="text-xs px-2 py-1 rounded border border-[var(--color-accent)] text-[var(--color-accent)] disabled:opacity-50 inline-flex items-center gap-1.5"
        >
          {saving && <Spinner />}
          Save as new session
        </button>
```

- [ ] **Step 7: Typecheck**

Run: `cd crates/webui/frontend && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add crates/webui/frontend/src/hooks/useEditor.ts crates/webui/frontend/src/components/Transcript.tsx
git commit -m "feat(webui): spinner on fork/edit-save/delete-message while in flight"
```

---

### Task 3: Sidebar spinners (delete / rename / new session)

**Files:**
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `Spinner` from Task 1. `onDelete` / `onRename` props are awaited (already async in `App.tsx`); widen their types to allow a returned Promise.

- [ ] **Step 1: Import Spinner and add busy state**

Add the import at the top of `Sidebar.tsx`:

```tsx
import Spinner from "./Spinner";
```

Widen the prop types in the `Props` interface:

```tsx
  onDelete: (s: SessionSummary) => void | Promise<void>;
  onRename: (s: SessionSummary, newTitle: string) => void | Promise<void>;
```

Add three state hooks next to the existing ones (near `confirmingId`):

```tsx
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
```

- [ ] **Step 2: New-session spinner**

Replace `createSession` with a version that tracks `creating`:

```tsx
  const createSession = async (cwd: string) => {
    const c = cwd.trim();
    if (!c) return;
    setCreating(true);
    try {
      await onNewSession(c);
      setNewOpen(false);
      setNewPath("");
      setNewErr(null);
    } catch (e) {
      setNewErr(String(e));
    } finally {
      setCreating(false);
    }
  };
```

In the "+ New" popover, update the "New in …" button (the `selectedCwd &&` block) to disable while creating:

```tsx
                <button
                  onClick={() => createSession(selectedCwd)}
                  disabled={creating}
                  className="w-full text-left text-xs px-2 py-1 rounded bg-[var(--color-bg)] border border-[var(--color-border)] hover:border-[var(--color-accent)] truncate disabled:opacity-50"
                  title={selectedCwd}
                >
                  New in {shortenPath(selectedCwd)}
                </button>
```

And the "Create" button:

```tsx
                <button
                  onClick={() => createSession(newPath)}
                  disabled={!newPath.trim() || creating}
                  className="text-xs px-2 py-1 rounded bg-[var(--color-accent)] text-white disabled:opacity-40 inline-flex items-center gap-1.5"
                >
                  {creating && <Spinner />}
                  Create
                </button>
```

- [ ] **Step 3: Delete spinner (per row)**

Update the confirm-delete "✓" button so it sets `deletingId` and awaits `onDelete`:

```tsx
                      <button
                        onClick={async (e) => {
                          e.stopPropagation();
                          setConfirmingId(null);
                          setDeletingId(s.id);
                          try {
                            await onDelete(s);
                          } finally {
                            setDeletingId(null);
                          }
                        }}
                        className="text-[10px] px-1.5 py-0.5 rounded text-[var(--color-error)] border border-[var(--color-error)] hover:bg-[var(--color-error)] hover:text-[var(--color-bg)] transition-colors"
                        title="Confirm delete"
                      >
                        ✓
                      </button>
```

In the non-confirming affordances, make the row's action cluster stay visible while deleting and show a spinner on the "×" button. Change the wrapper `<div>` className (the `opacity-0 group-hover:opacity-100` one) to:

```tsx
                <div className={`flex items-center shrink-0 pr-1.5 transition-opacity ${deletingId === s.id ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}>
```

And the "×" delete button:

```tsx
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          setConfirmingId(s.id);
                        }}
                        disabled={deletingId === s.id}
                        className="text-[10px] px-1 py-0.5 rounded text-[var(--color-text-dim)] hover:text-[var(--color-error)] transition-colors disabled:opacity-100"
                        title="Delete session"
                      >
                        {deletingId === s.id ? <Spinner /> : "×"}
                      </button>
```

- [ ] **Step 4: Rename spinner (per row)**

Make the inline-rename input's `onKeyDown` await the rename and show a spinner. Replace the editing branch's `<div>`/`<input>` with:

```tsx
                <div className="flex-1 px-3 py-2 min-w-0 flex items-center gap-2">
                  <input
                    autoFocus
                    value={draftTitle}
                    disabled={renamingId === s.id}
                    onChange={(e) => setDraftTitle(e.target.value)}
                    onKeyDown={async (e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        const t = draftTitle.trim();
                        if (t && t !== s.title) {
                          setRenamingId(s.id);
                          try {
                            await onRename(s, t);
                          } finally {
                            setRenamingId(null);
                          }
                        }
                        setEditingId(null);
                      } else if (e.key === "Escape") {
                        e.preventDefault();
                        setEditingId(null);
                      }
                    }}
                    onBlur={() => setEditingId(null)}
                    className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-sm px-1.5 py-1 rounded border border-[var(--color-accent)] outline-none disabled:opacity-60"
                  />
                  {renamingId === s.id && <Spinner />}
                </div>
```

- [ ] **Step 5: Typecheck**

Run: `cd crates/webui/frontend && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): spinners for sidebar delete / rename / new session"
```

---

### Task 4: Terminal "Starting claude…" overlay

**Files:**
- Modify: `crates/webui/frontend/src/components/TerminalView.tsx`

**Interfaces:**
- Consumes: `Spinner` from Task 1. Uses the existing `terminal-output` / `terminal-exit` listeners and `restart()`.

- [ ] **Step 1: Import Spinner and add `started` state**

Add the import:

```tsx
import Spinner from "./Spinner";
```

Next to `const [exited, setExited] = useState(false);` add:

```tsx
  const [started, setStarted] = useState(false);
```

- [ ] **Step 2: Mark started on first output**

In the `terminal-output` listener, set `started` true when a matching byte arrives:

```tsx
        await w.listen<{ sessionId: string; data: string }>("terminal-output", (e) => {
          if (e.payload.sessionId === sessionId) {
            term.write(b64ToBytes(e.payload.data));
            setStarted(true);
          }
        }),
```

- [ ] **Step 3: Reset on restart**

In `restart()`, reset `started` so the overlay reappears for the relaunched process:

```tsx
  const restart = () => {
    const t = termRef.current;
    if (!t) return;
    t.clear();
    setStarted(false);
    invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols })
      .then(() => setExited(false))
      .catch(() => {});
  };
```

- [ ] **Step 4: Render the overlay**

Replace the returned JSX's outer block so the overlay sits above the host (between the host div and the `{exited && ...}` Restart button):

```tsx
  return (
    <div className="relative h-full w-full" style={{ display: active ? "block" : "none" }}>
      <div ref={hostRef} className="h-full w-full" />
      {!started && !exited && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-[var(--color-text-dim)] pointer-events-none">
          <Spinner size="md" />
          <span className="text-sm">Starting claude…</span>
        </div>
      )}
      {exited && (
        <button
          onClick={restart}
          className="absolute top-2 right-2 px-3 py-1 rounded bg-[var(--color-accent)] text-white text-xs"
        >
          Restart
        </button>
      )}
    </div>
  );
```

- [ ] **Step 5: Typecheck**

Run: `cd crates/webui/frontend && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/components/TerminalView.tsx
git commit -m "feat(webui): Starting claude… overlay on the terminal until first output"
```

---

### Task 5: Consistency polish (open session / sidebar loading / searching)

**Files:**
- Modify: `crates/webui/frontend/src/App.tsx`
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `Spinner` from Task 1.

- [ ] **Step 1: Manager "Loading session…" → spinner**

In `App.tsx` add the import:

```tsx
import Spinner from "./components/Spinner";
```

Replace the `txLoading` branch:

```tsx
        {txLoading ? (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-[var(--color-text-dim)]">
            <Spinner size="md" />
            <span>Loading session…</span>
          </div>
        ) : transcript && transcript.id === target?.sessionId ? (
```

- [ ] **Step 2: Sidebar empty-list "Loading…" → spinner**

In `Sidebar.tsx` (import already added in Task 3) replace the empty-list loading branch:

```tsx
        {loading && sessions.length === 0 ? (
          <div className="p-4 flex items-center gap-2 text-[var(--color-text-dim)] text-sm">
            <Spinner /> Loading…
          </div>
        ) : sorted.length === 0 ? (
```

- [ ] **Step 3: Footer "· searching…" → spinner**

Replace the footer searching suffix:

```tsx
        {sorted.length}/{sessions.length} sessions
        {searching && (
          <span className="inline-flex items-center gap-1 ml-1">
            · <Spinner /> searching…
          </span>
        )}
```

- [ ] **Step 4: Typecheck + full frontend suite**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run`
Expected: tsc clean; 20 tests pass.

- [ ] **Step 5: Production build sanity**

Run: `cd crates/webui/frontend && npx vite build`
Expected: "✓ built".

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/App.tsx crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): unify open/loading/searching states on the Spinner atom"
```

---

## Self-Review

**Spec coverage:**
- Spinner atom → Task 1. ✓
- Fork/edit/delete-message → Task 2. ✓
- Delete session → Task 3 (step 3). ✓
- Rename session → Task 3 (step 4). ✓
- New session → Task 3 (step 2). ✓
- Terminal starting → Task 4. ✓
- Consistency polish (open / loading / searching) → Task 5. ✓
- Out-of-scope toasts → untouched. ✓

**Test deviation (deliberate):** the spec floated a `useEditor` pending unit test. The project has no jsdom / @testing-library and all existing tests are pure-function; the `pending` change is a direct mirror of the prior `busy` set/clear. Adding hook-test infrastructure for one trivial assertion is not worth it (YAGNI). Verification is `tsc` + existing suite green + manual in-app pass.

**Type consistency:** `editor.pending` shape `{ kind, uuid }` is consistent across `useEditor` (Task 2 step 1), the Transcript map (step 3), and the `forkPending`/`editPending`/`deletePending` booleans. `EditBox` prop is named `saving` consistently (Task 2 steps 4 & 6). `Spinner` props `size`/`className` match every call site.
