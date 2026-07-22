import type { SessionSummary } from "../types";

/**
 * A session's "updated" time in epoch seconds — when its last real conversation
 * turn happened, NOT when the file was last touched on disk.
 *
 * The sidebar's "Updated" column, its Updated sort, and its date-range filter
 * all read this so they agree. Precedence:
 *   1. freshest live activity (`activityAt`, from the 3s poller),
 *   2. the discovered last-event time,
 *   3. the file mtime — ONLY as a fallback for a session with no real turn yet
 *      (brand-new / turnless), so it still orders and displays sensibly.
 *
 * File mtime is deliberately last: a rename append, a copy, or any non-turn
 * write bumps the file's mtime without being a new message, and those must not
 * make a session look freshly "updated".
 */
export function sessionUpdatedAt(
  s: Pick<SessionSummary, "last_event_at" | "mtime">,
  activityAt: number | null | undefined,
): number {
  return activityAt ?? s.last_event_at?.secs_since_epoch ?? s.mtime.secs_since_epoch;
}
