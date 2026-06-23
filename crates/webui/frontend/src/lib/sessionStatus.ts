export type SessionStatus = { running: boolean; unread: boolean };

/**
 * A mid-turn session counts as "running" until its last write goes this stale.
 * Generous so a long thinking pause (no file writes) doesn't drop the dot — a
 * completed turn (`awaitingInput`) ends "running" immediately regardless.
 */
export const RUNNING_STALE_SECS = 180;

/**
 * Derive a session's sidebar status. `running` and `unread` are mutually
 * exclusive: a session is either mid-turn (claude working) or has completed a
 * turn (your turn). Keying on turn-completeness — not just write recency — is
 * what stops a thinking pause from flipping a live session to "unread".
 *
 * @param lastEventAt   last real-event time (epoch s), or null if no turns yet
 * @param awaitingInput last turn is a completed assistant turn (claude is done)
 * @param lastViewed    epoch s the user last opened this session (0 if never)
 * @param now           current epoch s
 */
export function deriveStatus(
  lastEventAt: number | null,
  awaitingInput: boolean,
  lastViewed: number,
  now: number,
): SessionStatus {
  if (lastEventAt == null) return { running: false, unread: false };
  return {
    // Mid-turn and not gone stale → claude is working (through thinking pauses).
    running: !awaitingInput && now - lastEventAt < RUNNING_STALE_SECS,
    // Completed a turn you haven't seen → unread (never while mid-turn).
    unread: awaitingInput && lastEventAt > lastViewed,
  };
}
