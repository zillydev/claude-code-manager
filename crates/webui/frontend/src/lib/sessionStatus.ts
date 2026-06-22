export type SessionStatus = { running: boolean; unread: boolean };

/** A session is "running" if its last real event landed within this window. */
export const RUNNING_WINDOW_SECS = 10;

/**
 * Derive a session's sidebar status from epoch-second inputs.
 * @param lastEventAt last real-event time (epoch s), or null if no turns yet
 * @param lastViewed  epoch s the user last opened this session (0 if never)
 * @param now         current epoch s
 */
export function deriveStatus(
  lastEventAt: number | null,
  lastViewed: number,
  now: number,
): SessionStatus {
  if (lastEventAt == null) return { running: false, unread: false };
  return {
    running: now - lastEventAt < RUNNING_WINDOW_SECS,
    unread: lastEventAt > lastViewed,
  };
}
