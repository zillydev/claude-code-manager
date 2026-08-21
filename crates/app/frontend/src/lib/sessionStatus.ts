export type SessionStatus = { running: boolean; unread: boolean };

/**
 * A mid-turn session counts as "running" until its last write goes this stale.
 * Generous so a long thinking pause (no file writes) doesn't drop the dot — a
 * completed turn (`awaitingInput`) ends "running" immediately regardless.
 *
 * Only a guess at "the process died mid-turn", though: a long tool call (a
 * build, a CI poll) writes nothing for minutes and is still very much running.
 * When ccm HOSTS the session it doesn't have to guess — see `hosted` below.
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
 * @param hosted        ccm is running this session's `claude` process (live PTY).
 *                      Then liveness is known, not inferred, so the staleness
 *                      backstop is skipped: a 20-minute tool call stays running.
 */
export function deriveStatus(
  lastEventAt: number | null,
  awaitingInput: boolean,
  lastViewed: number,
  now: number,
  hosted = false,
): SessionStatus {
  if (lastEventAt == null) return { running: false, unread: false };
  return {
    // Mid-turn → claude is working (through thinking pauses and long tools).
    running: !awaitingInput && (hosted || now - lastEventAt < RUNNING_STALE_SECS),
    // Completed a turn you haven't seen → unread (never while mid-turn).
    unread: awaitingInput && lastEventAt > lastViewed,
  };
}
