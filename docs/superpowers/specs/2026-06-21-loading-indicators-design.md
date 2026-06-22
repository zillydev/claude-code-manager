# Loading Indicators Design

**Date:** 2026-06-21
**Status:** Approved
**TODOS.md item:** UI overhaul → "loading indicators on everything"

## Goal

Give every async action in the webui visible, inline feedback while it runs, so
nothing feels unresponsive. Style is **inline, per-action** (not a global
progress bar): each control shows its own state — a button disables and shows a
small spinner; pane-level loads show a centered spinner.

## Motivation

Several operations today run with no visual feedback between click and result:

- **Fork / edit-save / delete-message** — `useEditor` computes a `busy` flag but
  it is never rendered. Only the result banner appears, *after* completion, so a
  click on "fork from here" looks like nothing happened.
- **Delete a session** — `await invoke("delete_session")` with no UI feedback.
- **Rename a session** — `await invoke("rename_session")` with no UI feedback.
- **New session** — clicking "+ New" runs `new_session` with no feedback.
- **Terminal starting** — the pane is blank black until `claude` paints its
  first frame; no "starting" indicator.

Two operations already have *minimal* text feedback and only need to adopt the
shared atom for consistency:

- **Open session** → "Loading session…" text (App.tsx).
- **Cross-transcript search** → "· searching…" text (Sidebar).

## The atom: `components/Spinner.tsx`

A single reusable spinner used by every indicator below.

```tsx
export default function Spinner({
  size = "sm",
  className = "",
}: {
  size?: "sm" | "md";
  className?: string;
}) {
  const px = size === "sm" ? "h-3.5 w-3.5 border-2" : "h-6 w-6 border-2";
  return (
    <span
      role="status"
      aria-label="Loading"
      className={`inline-block animate-spin rounded-full border-current border-t-transparent ${px} ${className}`}
    />
  );
}
```

- Inherits `currentColor` (via `border-current`) so it tints to its context
  (white on accent buttons, dim text elsewhere).
- `size="sm"` for inside buttons / inline; `size="md"` for pane-center.
- No external dependency; pure CSS animation (`animate-spin` is built into
  Tailwind).

## Wiring, per site

### 1. Editor ops (fork / edit-save / delete-message)

`hooks/useEditor.ts` already has a single `busy` flag (only one op runs at a
time). Extend it to also expose **which target** is in flight, so the exact
button the user clicked shows the spinner.

- Add state `const [pending, setPending] = useState<{ kind: "fork" | "edit" | "delete"; uuid: string } | null>(null)`.
- Each method sets `setPending({ kind, uuid })` on entry and `setPending(null)`
  in its `finally`. (`uuid` = `editUuid` / `forkUuid` / `messageUuid`.)
- Return `pending` alongside the existing `{ result, busy, error, ... }`.

In `components/Transcript.tsx`:

- Pass `pending` down to the per-turn action buttons.
- The **"fork from here"** button renders `<Spinner size="sm" />` in place of its
  label and sets `disabled` when `pending?.kind === "fork" && pending.uuid === turn.fork_uuid`.
- The **edit "Save"** button shows the spinner when
  `pending?.kind === "edit" && pending.uuid === editUuid`.
- The **delete-message** button shows the spinner when
  `pending?.kind === "delete" && pending.uuid === uuid`.
- While `editor.busy` is true, the other editor action buttons in the same turn
  are `disabled` (prevents a second concurrent op; the hook serializes anyway).

### 2. Delete a session (sidebar row)

`components/Sidebar.tsx` row trash button. The delete handler lives in
`App.tsx` (`onDelete`), so thread a busy signal through:

- Add `const [deletingId, setDeletingId] = useState<string | null>(null)` in
  `Sidebar`.
- The row's `onDelete` wrapper sets `deletingId = s.id`, `await`s the prop
  `onDelete(s)`, then clears it in `finally`.
- That row's trash icon → `<Spinner size="sm" />` + `disabled` while
  `deletingId === s.id`.

`App.tsx`'s `onDelete` prop stays `async` (it already is); Sidebar awaits it.

### 3. Rename a session (sidebar inline rename)

`components/Sidebar.tsx` rename commit. Add a local `renaming` boolean around the
`await onRename(s, newTitle)` call; show `<Spinner size="sm" />` next to the
rename input (or replace the confirm affordance) while it is true, and disable
the input. Clear in `finally`.

### 4. New session ("+ New" popover)

`components/Sidebar.tsx` create buttons (`createSession`). Add a local
`creating` boolean around the `await onNewSession(cwd)` call. While true: both
create buttons (paste-path and "New in <dir>") are `disabled` and the invoked one
shows `<Spinner size="sm" />`. Clear in `finally`. (After success the popover
closes and the app switches to the Terminal tab, whose existing "new session"
placeholder covers the wait for the first message.)

### 5. Terminal starting (overlay)

`components/TerminalView.tsx`. The PTY is spawned on first activation; until
`claude` emits its first byte the pane is blank. Add a "starting" overlay:

- Add `const [started, setStarted] = useState(false)`.
- In the `terminal-output` listener, on the first matching event call
  `setStarted(true)` (idempotent). Also set it on `terminal-exit`.
- Reset `started` to `false` when `terminal_open` is (re)invoked (initial open
  and Restart), so the overlay reappears for a fresh/relaunched process.
- Render, layered over the host, while `active && !started && !exited`:
  a centered `<Spinner size="md" />` + "Starting claude…". Absolutely positioned,
  pointer-events-none, so it never blocks the terminal underneath.

### 6. Consistency polish

- `App.tsx` Manager: replace the "Loading session…" text node with a centered
  `<Spinner size="md" />` + "Loading session…" label.
- `Sidebar.tsx`: replace the bare "Loading…" (empty-list state) and the
  "· searching…" suffix with the `<Spinner size="sm" />` atom inline.
- `Sidebar.tsx` refresh button already spins via a CSS class; leave it (it works),
  or optionally swap to the atom — low priority.

## Structure

- One new file: `components/Spinner.tsx` (the atom).
- All other changes are local busy state (`useState`) added at each call site,
  plus the `pending` field on `useEditor`. No global loading mechanism, no new
  context/provider — matches the chosen inline style.

## Error / completion feedback — out of scope

Success and error toasts are **not** part of this work. Delete/rename failures
keep their current `console.error`; the editor's existing inline `error` text and
`CommitResultBanner` are unchanged. A toast system can be a separate TODO item.

## Testing

Most of this is visual and verified by running the app. Pin the one piece of pure
logic worth a test:

- **`useEditor` pending tracking** — a vitest asserting that `fork` / `commitEdit`
  / `deleteMessage` set `pending` to the right `{ kind, uuid }` during the call
  and clear it to `null` afterward (mock `invoke`). This guards the per-button
  spinner targeting.

Otherwise: `tsc --noEmit` clean, the existing vitest suite stays green, the
`cargo` suite is unaffected (no backend changes), and a manual pass in-app
confirms each indicator appears and clears.

## Non-goals

- No backend (Rust) changes — every gap is a frontend rendering gap.
- No global progress bar / toast layer.
- No change to *when* operations run, only to how their in-flight state is shown.
