import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Activity = {
  id: string;
  last_event_at: number;
  awaiting_input: boolean;
  /** ccm is running this session's claude process (live PTY in a terminal tab). */
  hosted: boolean;
  /** Label of the window whose terminal runs it, when hosted. */
  owner?: string | null;
  busy: boolean;
  /** Epoch s the terminal was last working; once quiet, when claude began waiting. */
  busy_at: number;
  hook: { phase: "running" | "attention" | "awaiting"; since: number } | null;
  /** Epoch s of the user's last Esc / Ctrl-C in this session's terminal. */
  interrupt_at: number;
};
export type LiveActivity = {
  at: number;
  awaiting: boolean;
  hosted: boolean;
  owner: string | null;
  busy: boolean;
  busyAt: number;
  /** Claude's own phase from the opt-in hook relay, when installed. */
  hook: { phase: "running" | "attention" | "awaiting"; since: number } | null;
  interruptAt: number;
};

const toMap = (rows: Activity[]): Map<string, LiveActivity> =>
  new Map(
    rows.map((r) => [
      r.id,
      {
        at: r.last_event_at,
        awaiting: r.awaiting_input,
        hosted: r.hosted,
        owner: r.owner ?? null,
        busy: r.busy,
        busyAt: r.busy_at,
        hook: r.hook ?? null,
        interruptAt: r.interrupt_at ?? 0,
      },
    ]),
  );

/**
 * Live per-session activity, pushed by the single backend poller (one scan
 * app-wide). Pulls once on mount for instant dots, then updates on each
 * "activity-updated" broadcast. No per-window polling.
 */
export function useActivity(): Map<string, LiveActivity> {
  const [map, setMap] = useState<Map<string, LiveActivity>>(new Map());
  useEffect(() => {
    invoke<Activity[]>("session_activity")
      .then((rows) => setMap(toMap(rows)))
      .catch(() => {});
    const un = listen<Activity[]>("activity-updated", (e) => setMap(toMap(e.payload)));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return map;
}
