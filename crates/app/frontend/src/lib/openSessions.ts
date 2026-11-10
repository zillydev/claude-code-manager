import type { LiveActivity, PendingSession } from "../hooks/useActivity";
import type { SessionSummary } from "../types";
import { statusInputs } from "./closeGuard";
import { sessionUpdatedAt } from "./sessions";
import { deriveStatus } from "./sessionStatus";

/** A session whose `claude` runs in one of the app's terminals; `mine` when it runs in window `here`, `pending` before its first message. */
export type OpenSession = { session: SessionSummary; working: boolean; owner: string | null; mine: boolean; pending: boolean };

/** Placeholders for hosted sessions the session list lacks, each kept until its summary arrives: the poll that
 *  clears `pending` can land before the refetched list does. */
export function heldPending(
  prev: ReadonlyMap<string, PendingSession>,
  sessions: SessionSummary[],
  activity: Map<string, LiveActivity>,
): Map<string, PendingSession> {
  const known = new Set(sessions.map((s) => s.id));
  const next = new Map<string, PendingSession>();
  for (const [id, act] of activity) {
    if (!act.hosted || known.has(id)) continue;
    const p = act.pending ?? prev.get(id);
    if (p) next.set(id, p);
  }
  return next;
}

function pendingSummary(id: string, p: PendingSession, at: number): SessionSummary {
  return {
    id,
    file_path: p.filePath,
    project_label: p.projectLabel,
    cwd: p.cwd,
    git_branch: null,
    title: "New session",
    mtime: { secs_since_epoch: at, nanos_since_epoch: 0 },
    last_event_at: null,
    awaiting_input: true,
    total_tokens: 0,
    forked_from: null,
    copied_from: null,
    copied_overlap: null,
    forked_from_message: null,
    copied_from_message: null,
  };
}

/** Every session with a live terminal: window `here`'s first, then within each group those Claude is working in, then the most recently updated. */
export function openSessions(
  sessions: SessionSummary[],
  activity: Map<string, LiveActivity>,
  now: number,
  here: string,
  pending: ReadonlyMap<string, PendingSession> = heldPending(new Map(), sessions, activity),
): OpenSession[] {
  const rows: (OpenSession & { updated: number })[] = [];
  const push = (s: SessionSummary, act: LiveActivity, isPending: boolean) => {
    const i = statusInputs(s, act);
    // lastViewed only decides the unread dot, never whether a session is running.
    const working = deriveStatus(i.lastEventAt, i.awaiting, 0, now, i.terminal, i.hook).running;
    // An unstamped owner counts as here: an "other window" row needs a label to bring forward.
    const mine = act.owner == null || act.owner === here;
    rows.push({ session: s, working, owner: act.owner, mine, pending: isPending, updated: sessionUpdatedAt(s, act.at) });
  };
  const listed = new Set<string>();
  for (const s of sessions) {
    const act = activity.get(s.id);
    listed.add(s.id);
    if (act?.hosted) push(s, act, false);
  }
  for (const [id, p] of pending) {
    const act = activity.get(id);
    if (act?.hosted && !listed.has(id)) push(pendingSummary(id, p, act.at), act, true);
  }
  rows.sort((a, b) => Number(b.mine) - Number(a.mine) || Number(b.working) - Number(a.working) || b.updated - a.updated);
  return rows.map(({ session, working, owner, mine, pending }) => ({ session, working, owner, mine, pending }));
}

/** The Open tab's badge: how many sessions are open and whether Claude is working in any; null when none are. */
export function openBadge(rows: OpenSession[]): { label: string; count: number; working: boolean } | null {
  if (rows.length === 0) return null;
  const count = rows.length;
  return { label: count > 99 ? "99+" : String(count), count, working: rows.some((r) => r.working) };
}
