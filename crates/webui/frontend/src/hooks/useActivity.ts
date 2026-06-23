import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Activity = { id: string; last_event_at: number; awaiting_input: boolean };
export type LiveActivity = { at: number; awaiting: boolean };

const toMap = (rows: Activity[]): Map<string, LiveActivity> =>
  new Map(rows.map((r) => [r.id, { at: r.last_event_at, awaiting: r.awaiting_input }]));

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
