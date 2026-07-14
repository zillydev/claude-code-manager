# Close terminal session + live-indicator — design

## Goal

Two small additions to the embedded terminal:

1. **Close the claude session** — a control to kill the running `claude`
   process for a session on demand.
2. **Live indicator** — show on the Terminal tab button whether that session
   currently has a live claude terminal.

Frontend-only; no backend changes.

## Key idea: reuse the existing exit path

"Closing" a claude session is already a modeled state — the process exiting.
The backend `terminal_close(session_id)` command (already implemented and
registered, but so far uncalled from the UI) kills the PTY; when the child dies,
`spawn_terminal` emits `terminal-exit`, and `TerminalView` already renders the
`[process exited]` + **Restart** state for it. So "close" just triggers that
transition deliberately instead of waiting for claude to quit — no unmount, no
new "closed" state, no re-spawn bookkeeping. The xterm view stays mounted showing
the exited state; Restart brings claude back.

## Scope decisions

- **Per-window.** The indicator reflects *this* window's live terminal for the
  current session. A session open in another window still surfaces as "busy" when
  opening here (unchanged). A global cross-window indicator is a non-goal.
- **Dot = live only.** On self-exit or after Close, the dot clears; the pane's
  existing Restart button handles bringing it back.
- **No eager-mount changes.** The existing mount/spawn lifecycle is left as-is
  (explicitly out of scope).

## Changes

Four files; no backend, no new hook/lib.

### `TerminalView.tsx`
Add one prop `onStatus?: (s: "live" | "exited" | "busy") => void`. Call it at the
existing lifecycle points:
- after `terminal_open` resolves: `"live"` (status ok) or `"busy"` (owned
  elsewhere);
- on the `terminal-exit` listener: `"exited"`;
- in `restart()` on success: `"live"`.

### `TerminalPane.tsx`
Thread an `onStatus?: (sessionId, status) => void` prop through to each mounted
`TerminalView` (wrap so the sessionId is bound).

### `App.tsx`
- Hold `terminalStatus: Record<string, "live" | "exited" | "busy">`, updated by
  the `onStatus` callback passed to `TerminalPane`.
- `closeTerminal()` → `invoke("terminal_close", { sessionId: target.sessionId })`.
  The resulting `terminal-exit` drives the status back to `"exited"` and clears
  the dot.
- Pass `terminalLive={!!target && terminalStatus[target.sessionId] === "live"}`
  and `onClose={closeTerminal}` to **both** `TabToggle` placements (Manager
  header + Terminal header).

### `TabToggle.tsx`
Add `terminalLive?: boolean` and `onClose?: () => void`. On the **Terminal**
button:
- render a small **dot** when `terminalLive`;
- render a hover-revealed **✕** (a sibling button, not nested; `stopPropagation`
  so it closes rather than switching tabs) when `terminalLive && onClose`.

## Testing

The change is UI wiring (a status map + a kill invoke), so verification is
primarily manual in the running app:
- Start a terminal → dot appears on the Terminal button (both headers).
- Click ✕ → the `claude` process is gone (PTY killed), pane shows
  `[process exited]` + Restart, dot clears.
- Restart → claude returns, dot reappears.
- Let claude exit on its own → dot clears without a Close click.
- Open the same session in a second window → "busy", no dot here.

## Out of scope

- Backend changes (none needed).
- Unmounting/removing the terminal view on close; a blank post-close pane.
- Cross-window / global open indicator.
- Any change to the eager mount-vs-spawn lifecycle.
