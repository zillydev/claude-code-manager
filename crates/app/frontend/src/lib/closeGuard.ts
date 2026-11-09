import type { LiveActivity } from "../hooks/useActivity";
import type { SessionSummary } from "../types";
import { deriveStatus, type HookPhase, type TerminalState } from "./sessionStatus";

/** `deriveStatus`'s session inputs: live activity when the poller has it, else the summary. The
 *  sidebar and the close guard both read them here, so "working" means the same thing to each. */
export function statusInputs(
  s: SessionSummary,
  act: LiveActivity | undefined,
): { lastEventAt: number | null; awaiting: boolean; terminal: TerminalState; hook: HookPhase } {
  return {
    lastEventAt: act?.at ?? s.last_event_at?.secs_since_epoch ?? null,
    awaiting: act?.awaiting ?? s.awaiting_input,
    terminal: act?.hosted ? { busy: act.busy, busyAt: act.busyAt, interruptAt: act.interruptAt, program: act.program } : null,
    hook: act?.hook ?? null,
  };
}

/** The sessions among `ids` Claude is working in — the ones a close would interrupt mid-turn. */
export function workingSessions(
  ids: string[],
  sessions: SessionSummary[],
  activity: Map<string, LiveActivity>,
  now: number,
): SessionSummary[] {
  const wanted = new Set(ids);
  return sessions.filter((s) => {
    if (!wanted.has(s.id)) return false;
    const i = statusInputs(s, activity.get(s.id));
    // lastViewed only decides the unread dot, never whether a session is running.
    return deriveStatus(i.lastEventAt, i.awaiting, 0, now, i.terminal, i.hook).running;
  });
}
