export type SessionStatus = { running: boolean; unread: boolean };

/**
 * Claude's own account of a session's state, relayed by the opt-in hooks:
 * `attention` means a question / permission / plan card is on screen — the one
 * state neither the session file nor the terminal can prove.
 */
export type HookPhase = {
  phase: "running" | "attention" | "awaiting";
  since: number;
} | null;

/**
 * What ccm's own terminal knows about a session, or null when ccm isn't running
 * it (then only the session file's mtime is available to guess from).
 */
export type TerminalState = {
  /** Claude authored a real burst of output recently — it is working.
   *  Repaints caused by the user's own keys or a resize don't count. */
  busy: boolean;
  /** Epoch s it was last busy; once quiet, when claude started waiting on you. */
  busyAt: number;
  /** Epoch s of the user's last Esc / Ctrl-C in this terminal (0 if none). */
  interruptAt: number;
} | null;

/**
 * A mid-turn session counts as "running" until its last write goes this stale.
 * Generous so a long thinking pause (no file writes) doesn't drop the dot — a
 * completed turn (`awaitingInput`) ends "running" immediately regardless.
 *
 * Only a guess at "the process died mid-turn", though: a long tool call (a
 * build, a CI poll) writes nothing for minutes and is still very much running.
 * For sessions ccm runs itself, `busy` answers that directly — see below.
 */
export const RUNNING_STALE_SECS = 180;

/**
 * Derive a session's sidebar status. `running` and `unread` are mutually
 * exclusive: a session is either mid-turn (claude working) or waiting on you.
 * Keying on turn-completeness — not just write recency — is what stops a
 * thinking pause from flipping a live session to "unread".
 *
 * @param lastEventAt   last real-event time (epoch s), or null if no turns yet
 * @param awaitingInput last turn is a completed assistant turn (claude is done)
 * @param lastViewed    epoch s the user last opened this session (0 if never)
 * @param now           current epoch s
 * @param terminal      ccm's own terminal for this session, when it runs it.
 *                      Claude's TUI talks constantly while working and goes
 *                      quiet when it waits, so `busy` answers "is it working?"
 *                      outright and the staleness backstop doesn't apply: a
 *                      20-minute tool call stays running, and a session parked
 *                      on a question stops at once. (A live PTY alone only
 *                      proves the process exists — that was the bug.)
 */
/**
 * Whether the sidebar paints the unread dot. The open session is only "read"
 * while you can actually see it — in a background window or on another Space it
 * still owes you the dot.
 */
export function showsUnread(
  status: SessionStatus,
  isOpen: boolean,
  windowFocused: boolean,
): boolean {
  return status.unread && !(isOpen && windowFocused);
}

export function deriveStatus(
  lastEventAt: number | null,
  awaitingInput: boolean,
  lastViewed: number,
  now: number,
  terminal: TerminalState = null,
  hook: HookPhase = null,
): SessionStatus {
  if (lastEventAt == null) return { running: false, unread: false };
  // An Esc / Ctrl-C in ccm's own terminal ends the turn as surely as the file's
  // interrupt marker — and Esc before the first token writes no marker and
  // fires no hook, so the keystroke is the only witness.
  if (terminal && terminal.interruptAt > lastEventAt && !awaitingInput) {
    lastEventAt = terminal.interruptAt;
    awaitingInput = true;
  }
  // Claude's own account outranks inference unless the file refutes it: a
  // running phase is contradicted only by a finished turn or an interrupt
  // (both `awaitingInput`), never by the tool lines a turn writes.
  if (hook && (hook.since >= lastEventAt || (hook.phase === "running" && !awaitingInput))) {
    return {
      // Unhosted: no PTY to prove liveness, so decay like the mtime guess —
      // from the last sign of life, hook or file line.
      running:
        hook.phase === "running" &&
        (terminal ? true : now - Math.max(hook.since, lastEventAt) < RUNNING_STALE_SECS),
      unread: hook.phase !== "running" && hook.since > lastViewed,
    };
  }
  // A quiet terminal with no finished turn in the file means claude is parked
  // on something it hasn't written down — a question, a permission prompt, a
  // plan. Claude Code records a question only once it is answered, so the file
  // still ends on YOUR prompt: date this from when the terminal went quiet, or
  // it would compare against something you have by definition already seen.
  const waitingSince =
    terminal && !terminal.busy && !awaitingInput ? terminal.busyAt : null;
  return {
    // Mid-turn → claude is working (through thinking pauses and long tools).
    running:
      !awaitingInput &&
      (terminal ? terminal.busy : now - lastEventAt < RUNNING_STALE_SECS),
    // Your turn, and you haven't seen it yet (never while mid-turn).
    unread:
      (awaitingInput && lastEventAt > lastViewed) ||
      (waitingSince != null && waitingSince > lastViewed),
  };
}
