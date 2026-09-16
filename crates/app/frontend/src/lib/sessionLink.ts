import type { SessionSummary } from "../types";

export const SESSION_LINK_SCHEME = "claude-code-manager";

/** A link to a session; the backend registers `SESSION_LINK_SCHEME` and parses the same form. */
export function sessionLink(sessionId: string): string {
  return `${SESSION_LINK_SCHEME}://session/${sessionId}`;
}

export type LinkResolution =
  | { kind: "wait" }
  | { kind: "open"; session: SessionSummary }
  | { kind: "refresh" }
  | { kind: "missing" };

/** What to do with a received session id: wait for the list, open it, refresh once for a new file, or give up. */
export function resolveLink(id: string, sessions: SessionSummary[], loading: boolean, refreshed: boolean): LinkResolution {
  if (loading) return { kind: "wait" };
  const session = sessions.find((s) => s.id === id);
  if (session) return { kind: "open", session };
  return refreshed ? { kind: "missing" } : { kind: "refresh" };
}

/** What to say when a session opened by id never appears. A fork's file was just written, so it can't be gone. */
export function missingNotice(source: "link" | "fork"): string {
  return source === "fork"
    ? "The fork was created, but it isn't showing yet — it will appear in the sidebar shortly."
    : "No session with that id — it may have been deleted, or lives on another machine.";
}
