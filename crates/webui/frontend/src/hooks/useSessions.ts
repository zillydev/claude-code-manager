import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { SessionSummary, RenderedSession, RenderedTurn } from "../types";

export function useSessions() {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Pull the cached list (cheap — discovery is shared in the backend).
  const pull = useCallback(async () => {
    try {
      setSessions(await invoke<SessionSummary[]>("list_sessions"));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // Manual ↻: force a re-discovery (the backend broadcasts to other windows too).
  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setSessions(await invoke<SessionSummary[]>("refresh_sessions"));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    pull();
    // The backend poller broadcasts "sessions-changed" to ALL windows on any
    // discovery change; re-pull the (now cached) list.
    const un = listen("sessions-changed", () => pull());
    return () => {
      un.then((f) => f());
    };
  }, [pull]);

  return { sessions, loading, error, refresh };
}

export function useTranscript() {
  const [transcript, setTranscript] = useState<RenderedSession | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const open = useCallback(async (filePath: string, projectLabel: string) => {
    // Clean up previous listener.
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
    await invoke("stop_watching").catch(() => {});

    setLoading(true);
    setError(null);
    try {
      const result = await invoke<RenderedSession>("open_session", {
        filePath,
        projectLabel,
      });
      setTranscript(result);

      // Subscribe to live tail events for THIS window only. The backend emits
      // with emit_to(<this window's label>), so each window receives only the
      // turns for the session it opened — no cross-window cross-feed.
      const unlisten = await getCurrentWebviewWindow().listen<RenderedTurn[]>(
        "session-updated",
        (event) => {
          setTranscript((prev) => {
            if (!prev) return prev;
            return {
              ...prev,
              turns: [...prev.turns, ...event.payload],
              event_count: prev.event_count + event.payload.length,
            };
          });
        }
      );
      unlistenRef.current = unlisten;
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const close = useCallback(async () => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
    await invoke("stop_watching").catch(() => {});
    setTranscript(null);
    setError(null);
  }, []);

  // Cleanup on unmount.
  useEffect(() => {
    return () => {
      if (unlistenRef.current) {
        unlistenRef.current();
      }
      invoke("stop_watching").catch(() => {});
    };
  }, []);

  return { transcript, loading, error, open, close };
}
