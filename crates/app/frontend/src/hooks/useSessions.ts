import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { SessionSummary, RenderedSession } from "../types";
import { patchToolResults } from "../lib/toolResults";
import { createTranscriptLoader } from "../lib/transcriptLoader";

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

  // Live tail events arrive with emit_to(<this window's label>), so each window only hears the session it opened.
  const loader = useMemo(() => {
    const win = getCurrentWebviewWindow();
    return createTranscriptLoader(
      {
        invoke: (cmd, args) => invoke(cmd, args),
        listen: (event, handler) => win.listen(event, (e) => handler(e.payload as never)),
      },
      {
        loaded: setTranscript,
        appended: (turns) =>
          setTranscript((prev) =>
            prev ? { ...prev, turns: [...prev.turns, ...turns], event_count: prev.event_count + turns.length } : prev,
          ),
        // A slow tool's result lands in a later tail batch than its call; patch it in by tool_use_id.
        toolResults: (updates) =>
          setTranscript((prev) => {
            if (!prev) return prev;
            const turns = patchToolResults(prev.turns, updates);
            return turns === prev.turns ? prev : { ...prev, turns };
          }),
        // A new prompt or /rename retitles the session while it is open.
        retitled: (title) => setTranscript((prev) => (prev && prev.title !== title ? { ...prev, title } : prev)),
      },
    );
  }, []);

  // `branchUuid` opens the session showing the branch that message sits on
  // rather than the one the file ends with — a find result can land on a prompt
  // the session rewound past. Such a view is frozen: the backend sends no live
  // tail for it, because new turns belong to the branch you're not looking at.
  const open = useCallback(
    async (filePath: string, projectLabel: string, branchUuid?: string) => {
      setLoading(true);
      setError(null);
      try {
        // An open overtaken by a newer one leaves loading and errors to that one.
        if ((await loader.open(filePath, projectLabel, branchUuid)) === "opened") setLoading(false);
      } catch (e) {
        setError(String(e));
        setLoading(false);
      }
    },
    [loader],
  );

  const close = useCallback(async () => {
    await loader.close();
    setTranscript(null);
    setError(null);
    setLoading(false);
  }, [loader]);

  // Cleanup on unmount.
  useEffect(() => {
    return () => {
      loader.close();
    };
  }, [loader]);

  return { transcript, loading, error, open, close };
}
