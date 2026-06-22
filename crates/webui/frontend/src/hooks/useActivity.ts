import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const POLL_MS = 3000;
const REFRESH_DEBOUNCE_MS = 1500;

type Activity = { id: string; last_event_at: number };

/**
 * Polls the backend for recently-active sessions' last-event time (epoch s) so
 * the sidebar can show a live "running" dot. Only polls while the window is
 * visible. When any session's activity advances, fires a debounced full
 * `refresh()` so the parsed list (tokens/title/last_event_at) catches up.
 * Returns a Map of sessionId → last_event_at for recently-touched sessions only.
 */
export function useActivity(refresh: () => void): Map<string, number> {
  const [map, setMap] = useState<Map<string, number>>(new Map());
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
        const next = new Map(rows.map((r) => [r.id, r.last_event_at]));
        let advanced = false;
        for (const [id, t] of next) {
          if (t > (mapRef.current.get(id) ?? 0)) {
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
