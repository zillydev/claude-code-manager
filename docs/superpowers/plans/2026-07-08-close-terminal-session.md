# Close Terminal Session + Live Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a control to close (kill) a session's running `claude` terminal, and a live-indicator dot on the Terminal tab button.

**Architecture:** Frontend-only. `TerminalView` reports its PTY status up via a new `onStatus` callback; `TerminalPane` threads it through; `App` holds a `terminalStatus` map and a `closeTerminal` handler; `TabToggle` renders a green dot when the current session is live plus a hover-✕ that invokes the existing (uncalled) `terminal_close` command. Closing reuses the existing `terminal-exit` → `[process exited]` + Restart path — no new "closed" state.

**Tech Stack:** React 19 + TypeScript, Tauri 2 (`@tauri-apps/api` `invoke`), xterm.js, Tailwind v4, Vite, Vitest.

## Global Constraints

- No backend/Rust changes — `terminal_close(session_id)` already exists and is registered.
- No changes to the mount-vs-spawn lifecycle (explicitly out of scope).
- Indicator is per-window; dot shows only for a `"live"` status (this window's PTY). `"busy"` (owned elsewhere) and `"exited"` show no dot.
- Status union is exactly `"live" | "exited" | "busy"` everywhere.
- No component-test harness exists in this project (tests are pure-function Vitest only). These tasks are UI wiring: each is verified by `npx tsc --noEmit` clean, `npx vitest run` staying green (69 tests), and `npx vite build` succeeding; end-to-end behavior is verified manually in Task 4.
- Run all frontend commands from `crates/webui/frontend`.
- End commit messages with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

### Task 1: `TabToggle` — live dot + hover-✕

**Files:**
- Modify: `crates/webui/frontend/src/components/TabToggle.tsx` (whole file)

**Interfaces:**
- Produces: `TabToggle` now accepts two optional props — `terminalLive?: boolean` and `onClose?: () => void`. Existing callers that omit them are unaffected (dot/✕ simply don't render).

- [ ] **Step 1: Replace the component**

Replace the entire file with:

```tsx
/** The Manager ↔ Terminal switch. Lives in the session header (right cluster on
 *  Manager, slim header on Terminal) rather than its own bar, to save a row.
 *  The Terminal button carries a live-terminal dot and a hover-✕ to close it. */
export default function TabToggle({
  activeTab,
  onSelect,
  terminalLive = false,
  onClose,
}: {
  activeTab: "manager" | "terminal";
  onSelect: (t: "manager" | "terminal") => void;
  /** Show a green dot on the Terminal button (this window has a live PTY for the
   *  current session). */
  terminalLive?: boolean;
  /** Close (kill) the current session's terminal; enables the hover-✕. */
  onClose?: () => void;
}) {
  const btn = (active: boolean) =>
    "px-3 py-1 text-sm rounded " +
    (active
      ? "bg-[var(--color-accent)] text-white"
      : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]");
  const showClose = terminalLive && !!onClose;
  return (
    <div className="flex gap-1">
      <button className={btn(activeTab === "manager")} onClick={() => onSelect("manager")}>
        Manager
      </button>
      {/* group: the ✕ reveals on hover over the Terminal button. */}
      <div className="group relative flex items-center">
        <button
          className={btn(activeTab === "terminal") + (showClose ? " pr-6" : "")}
          onClick={() => onSelect("terminal")}
        >
          <span className="inline-flex items-center gap-1.5">
            {terminalLive && (
              <span
                className="inline-block w-1.5 h-1.5 rounded-full bg-green-500"
                title="A claude terminal is running for this session"
              />
            )}
            Terminal
          </span>
        </button>
        {showClose && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onClose!();
            }}
            title="Close terminal session (stops claude)"
            className={
              "absolute right-1.5 opacity-0 group-hover:opacity-100 text-xs leading-none px-0.5 " +
              (activeTab === "terminal"
                ? "text-white/80 hover:text-white"
                : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]")
            }
          >
            ✕
          </button>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Typecheck + build + tests**

Run: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; `Tests 69 passed`; build succeeds. (Callers don't pass the new props yet, so nothing renders differently.)

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/components/TabToggle.tsx
git commit -m "feat(webui): TabToggle live-terminal dot + hover-close (props unused yet)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `TerminalView` — report status via `onStatus`

**Files:**
- Modify: `crates/webui/frontend/src/components/TerminalView.tsx`

**Interfaces:**
- Produces: `TerminalView` accepts `onStatus?: (s: "live" | "exited" | "busy") => void`, invoked after `terminal_open` resolves (`"live"` / `"busy"`), on `terminal-exit` (`"exited"`), and on successful Restart (`"live"`).
- Consumes: the existing `terminal_open` result `{ status: string; owner: string | null }` and the `terminal-exit` event.

- [ ] **Step 1: Add the prop + a stable ref**

In the props destructure (currently `sessionId, filePath, active, cwd`), add `onStatus`:

```tsx
export default function TerminalView({
  sessionId,
  filePath,
  active,
  cwd,
  onStatus,
}: {
  sessionId: string;
  filePath: string;
  active: boolean;
  cwd?: string;
  onStatus?: (s: "live" | "exited" | "busy") => void;
}) {
```

Then, next to the existing `monoRef`/`blockedRef` refs (just after `blockedRef.current = blocked !== null;`), add a ref so listeners/callbacks read the latest without re-subscribing:

```tsx
  const onStatusRef = useRef(onStatus);
  onStatusRef.current = onStatus;
```

- [ ] **Step 2: Report `"exited"` on the exit listener**

In the `"terminal-exit"` listener body, add the report after `setExited(true);`:

```tsx
        await w.listen<{ sessionId: string }>("terminal-exit", (e) => {
          if (e.payload.sessionId === sessionId) {
            term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
            setExited(true);
            onStatusRef.current?.("exited");
          }
        }),
```

- [ ] **Step 3: Report `"live"`/`"busy"` after `terminal_open`**

In the activation effect, replace the `.then` on `terminal_open`:

```tsx
          .then((res) => {
            if (res.status === "busy") {
              setBlocked({ owner: res.owner });
              onStatusRef.current?.("busy");
            } else {
              onStatusRef.current?.("live");
            }
          })
```

- [ ] **Step 4: Report `"live"` on Restart**

Update `restart()`'s success handler:

```tsx
  const restart = () => {
    const t = termRef.current;
    if (!t) return;
    t.clear();
    setStarted(false);
    setBlocked(null);
    invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols })
      .then(() => {
        setExited(false);
        onStatusRef.current?.("live");
      })
      .catch(() => {});
  };
```

- [ ] **Step 5: Typecheck + build + tests**

Run: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; `Tests 69 passed`; build succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/components/TerminalView.tsx
git commit -m "feat(webui): TerminalView reports PTY status (live/exited/busy) via onStatus

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `TerminalPane` — thread `onStatus` through

**Files:**
- Modify: `crates/webui/frontend/src/components/TerminalPane.tsx`

**Interfaces:**
- Consumes: `TerminalView`'s `onStatus` (Task 2).
- Produces: `TerminalPane` accepts `onStatus?: (sessionId: string, s: "live" | "exited" | "busy") => void` and forwards it per-session.

- [ ] **Step 1: Add the prop**

Add to the props type (after `tabActive: boolean;`):

```tsx
  onStatus,
}: {
  sessionId: string | null;
  filePath: string | null;
  cwd?: string;
  tabActive: boolean;
  onStatus?: (sessionId: string, s: "live" | "exited" | "busy") => void;
}) {
```

(Add `onStatus` to the destructured params list alongside `sessionId, filePath, cwd, tabActive`.)

- [ ] **Step 2: Forward it to each `TerminalView`**

In the `Object.entries(opened).map(...)` render, add the bound callback:

```tsx
        <TerminalView
          key={sid}
          sessionId={sid}
          filePath={v.filePath}
          cwd={v.cwd}
          active={tabActive && sid === sessionId}
          onStatus={onStatus ? (s) => onStatus(sid, s) : undefined}
        />
```

- [ ] **Step 3: Typecheck + build + tests**

Run: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; `Tests 69 passed`; build succeeds. (App doesn't pass `onStatus` yet — still a no-op.)

- [ ] **Step 4: Commit**

```bash
git add crates/webui/frontend/src/components/TerminalPane.tsx
git commit -m "feat(webui): TerminalPane forwards per-session terminal status

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `App` — wire status map, close handler, and TabToggle props

**Files:**
- Modify: `crates/webui/frontend/src/App.tsx`

**Interfaces:**
- Consumes: `TerminalPane`'s `onStatus` (Task 3), `TabToggle`'s `terminalLive`/`onClose` (Task 1), and the backend `terminal_close` command.

- [ ] **Step 1: Add state + handlers**

Near the other `useState` hooks in `App` (e.g. just after the `activeTab` state at line ~30), add:

```tsx
  // Per-session terminal liveness for THIS window: drives the Terminal tab dot
  // and enables the close-✕. Fed by TerminalView via TerminalPane's onStatus.
  const [terminalStatus, setTerminalStatus] = useState<
    Record<string, "live" | "exited" | "busy">
  >({});
```

Then add the two handlers alongside the other `useCallback`s (e.g. after `openSession`):

```tsx
  const onTerminalStatus = useCallback(
    (sid: string, status: "live" | "exited" | "busy") =>
      setTerminalStatus((m) => (m[sid] === status ? m : { ...m, [sid]: status })),
    [],
  );

  // Close = kill the PTY; the resulting terminal-exit drives status back to
  // "exited" (clearing the dot) and the pane into its [process exited]+Restart state.
  const closeTerminal = useCallback(() => {
    if (target) invoke("terminal_close", { sessionId: target.sessionId }).catch(() => {});
  }, [target]);
```

- [ ] **Step 2: Derive current-session liveness**

Just before the `return (`, add:

```tsx
  const terminalLive = !!target && terminalStatus[target.sessionId] === "live";
```

- [ ] **Step 3: Pass props to both `TabToggle` placements**

Manager-header `TabToggle` (currently `tabs={<TabToggle activeTab={activeTab} onSelect={setActiveTab} />}`):

```tsx
                    tabs={
                      <TabToggle
                        activeTab={activeTab}
                        onSelect={setActiveTab}
                        terminalLive={terminalLive}
                        onClose={closeTerminal}
                      />
                    }
```

Terminal-header `TabToggle`:

```tsx
                  <TabToggle
                    activeTab={activeTab}
                    onSelect={setActiveTab}
                    terminalLive={terminalLive}
                    onClose={closeTerminal}
                  />
```

- [ ] **Step 4: Pass `onStatus` to `TerminalPane`**

```tsx
                <TerminalPane
                  sessionId={target?.sessionId ?? null}
                  filePath={target?.filePath ?? null}
                  cwd={target?.cwd}
                  tabActive={activeTab === "terminal"}
                  onStatus={onTerminalStatus}
                />
```

- [ ] **Step 5: Typecheck + build + tests**

Run: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; `Tests 69 passed`; build succeeds.

- [ ] **Step 6: Manual verification in the running app**

The user runs the dev app (do NOT launch it yourself). Ask the user to confirm:
1. Open a session, switch to the Terminal tab → **green dot** appears on the Terminal button (visible from both the Manager and Terminal headers).
2. Hover the Terminal button → **✕** appears; click it → the `claude` process stops, the pane shows `[process exited]` + **Restart**, and the dot clears. (No tab switch happens on ✕ click.)
3. Click **Restart** → claude returns, dot reappears.
4. In another session's terminal, type `/exit` (or let claude quit) → dot clears on its own.
5. Open the same session in a second window → it shows "busy" there; no dot in the second window.

- [ ] **Step 7: Commit**

```bash
git add crates/webui/frontend/src/App.tsx
git commit -m "feat(webui): close terminal session from the tab button + live dot

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Update FEATURES.md

**Files:**
- Modify: `FEATURES.md` (the embedded-terminal section)

**Interfaces:** none.

- [ ] **Step 1: Find the terminal line**

Run: `grep -n -i "terminal" FEATURES.md`
Locate the embedded-terminal bullet (the one describing the in-app `claude` terminal tab).

- [ ] **Step 2: Append a concise clause**

Add to that bullet (adapt wording to the existing line): a green **live dot** on the Terminal tab button when a session's `claude` is running, and a hover-**✕** to close (kill) it — reusing the process-exit → Restart path.

- [ ] **Step 3: Commit**

```bash
git add FEATURES.md
git commit -m "docs: FEATURES.md — close terminal session + live dot

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Close the claude session → Task 4 (`closeTerminal` → `terminal_close`) + Task 1 (✕). ✓
- Live indicator on the tab button → Task 1 (dot) + Tasks 2–4 (status plumbing). ✓
- Reuse exit path (no new closed state) → Task 2 reports `"exited"`; pane unchanged. ✓
- Per-window / dot=live-only / busy handling → Task 2 (`"busy"`), Task 4 (`terminalLive` = `"live"`). ✓
- No backend changes → confirmed; only `invoke("terminal_close")`. ✓
- FEATURES.md currency (project convention) → Task 5. ✓

**Placeholder scan:** none — every step has concrete code/commands.

**Type consistency:** the status union `"live" | "exited" | "busy"` is identical across `TerminalView.onStatus`, `TerminalPane.onStatus`, `App.terminalStatus`/`onTerminalStatus`; `terminalLive: boolean` and `onClose: () => void` match between `App` and `TabToggle`.
