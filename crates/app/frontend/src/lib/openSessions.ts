import type { LiveActivity } from "../hooks/useActivity";
import type { SessionSummary } from "../types";
import { statusInputs } from "./closeGuard";
import { sessionUpdatedAt } from "./sessions";
import { deriveStatus } from "./sessionStatus";

/** A session whose `claude` runs in one of the app's terminals. */
export type OpenSession = { session: SessionSummary; working: boolean; owner: string | null };

/** Every session with a live terminal, those Claude is working in first, then the most recently updated. */
export function openSessions(sessions: SessionSummary[], activity: Map<string, LiveActivity>, now: number): OpenSession[] {
  const rows: (OpenSession & { updated: number })[] = [];
  for (const s of sessions) {
    const act = activity.get(s.id);
    if (!act?.hosted) continue;
    const i = statusInputs(s, act);
    // lastViewed only decides the unread dot, never whether a session is running.
    const working = deriveStatus(i.lastEventAt, i.awaiting, 0, now, i.terminal, i.hook).running;
    rows.push({ session: s, working, owner: act.owner, updated: sessionUpdatedAt(s, act.at) });
  }
  rows.sort((a, b) => Number(b.working) - Number(a.working) || b.updated - a.updated);
  return rows.map(({ session, working, owner }) => ({ session, working, owner }));
}

/** The Open tab's badge: how many sessions are open and whether Claude is working in any; null when none are. */
export function openBadge(rows: OpenSession[]): { label: string; count: number; working: boolean } | null {
  if (rows.length === 0) return null;
  const count = rows.length;
  return { label: count > 99 ? "99+" : String(count), count, working: rows.some((r) => r.working) };
}
