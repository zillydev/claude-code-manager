import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const POLL_MS = 3000;
const REFRESH_DEBOUNCE_MS = 1500;

type Activity = { id: string; last_event_at: number; awaiting_input: boolean };
export type LiveActivity = { at: number; awaiting: boolean };

/**
 * Polls the backend for recently-active sessions' last-event time + turn state
 * so the sidebar can show a live "running" dot. Only polls while the window is
 * visible. When any session's activity advances, fires a debounced full
 * `refresh()` so the parsed list (tokens/title/last_event_at) catches up.
 * Returns a Map of sessionId → {at, awaiting} for recently-touched sessions only.
 */
export function useActivity(refresh: () => void): Map<string, LiveActivity> {
  const [map, setMap] = useState<Map<string, LiveActivity>>(new Map());
  const mapRef = useRef(map);
  mapRef.current = map;
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  useEffect(() => {
    let cancelled = false;
    let debounce: ReturnType<typeof setTimeout> | null = null;

    const tick = async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const rows = await invoke<Activity[]>("session_activity");
        if (cancelled) return;
        const next = new Map<string, LiveActivity>(
          rows.map((r) => [r.id, { at: r.last_event_at, awaiting: r.awaiting_input }]),
        );
        let advanced = false;
        for (const [id, v] of next) {
          if (v.at > (mapRef.current.get(id)?.at ?? 0)) {
            advanced = true;
            break;
          }
        }
        setMap(next); // new identity every tick → consumers re-render, running decays
        if (advanced) {
          if (debounce) clearTimeout(debounce);
          debounce = setTimeout(() => refreshRef.current(), REFRESH_DEBOUNCE_MS);
        }
      } catch {
        /* ignore poll errors (e.g. mid-shutdown) */
      }
    };

    const interval = setInterval(tick, POLL_MS);
    tick();
    return () => {
      cancelled = true;
      clearInterval(interval);
      if (debounce) clearTimeout(debounce);
    };
  }, []);

  return map;
}
