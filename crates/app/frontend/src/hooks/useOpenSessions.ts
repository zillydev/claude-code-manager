import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { LiveActivity, PendingSession } from "./useActivity";
import type { SessionSummary } from "../types";
import { heldPending, openSessions, type OpenSession } from "../lib/openSessions";

/** The sessions running in a terminal, as the Open tab and its badge show them, and a way to close one. */
export function useOpenSessions(
  sessions: SessionSummary[],
  activity: Map<string, LiveActivity>,
  now: number,
): { rows: OpenSession[]; close: (id: string) => void } {
  // Closed here but still hosted in the last poll (every 3 s): hidden rather than lingering.
  const [closing, setClosing] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    setClosing((prev) => {
      const still = [...prev].filter((id) => activity.get(id)?.hosted);
      return still.length === prev.size ? prev : new Set(still);
    });
  }, [activity]);

  const held = useRef<ReadonlyMap<string, PendingSession>>(new Map());
  const pending = useMemo(() => (held.current = heldPending(held.current, sessions, activity)), [sessions, activity]);

  const [here] = useState(() => getCurrentWebviewWindow().label);
  const rows = openSessions(sessions, activity, now, here, pending).filter((r) => !closing.has(r.session.id));
  const close = (id: string) => {
    setClosing((prev) => new Set(prev).add(id));
    invoke("terminal_close", { sessionId: id }).catch(() =>
      setClosing((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      }),
    );
  };
  return { rows, close };
}
