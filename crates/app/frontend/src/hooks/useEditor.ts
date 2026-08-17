import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CommitResult } from "../types";

export type EditorPending = { kind: "fork"; uuid: string } | null;

export function useEditor() {
  const [result, setResult] = useState<CommitResult | null>(null);
  const [pending, setPending] = useState<EditorPending>(null);
  const [error, setError] = useState<string | null>(null);

  /// Copies the source file verbatim up to the cut, so the new session keeps
  /// the full history (abandoned branches, pre-compaction segments) the source
  /// had at that point.
  const fork = useCallback(
    async (filePath: string, forkUuid: string) => {
      setPending({ kind: "fork", uuid: forkUuid });
      setError(null);
      try {
        const res = await invoke<CommitResult>("fork_session", {
          filePath,
          forkUuid,
          customTitle: null,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setPending(null);
      }
    },
    []
  );

  const dismiss = useCallback(() => setResult(null), []);

  return {
    result,
    pending,
    busy: pending !== null,
    error,
    fork,
    dismiss,
  };
}
