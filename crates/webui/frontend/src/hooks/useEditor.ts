import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CommitResult } from "../types";

export function useEditor() {
  const [result, setResult] = useState<CommitResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const commitEdit = useCallback(
    async (filePath: string, editIndex: number, newText: string) => {
      setBusy(true);
      setError(null);
      try {
        const res = await invoke<CommitResult>("commit_session", {
          filePath,
          editIndex,
          newText,
          customTitle: null,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    []
  );

  const fork = useCallback(
    async (filePath: string, forkIndex: number) => {
      setBusy(true);
      setError(null);
      try {
        const res = await invoke<CommitResult>("fork_session", {
          filePath,
          forkIndex,
          customTitle: null,
        });
        setResult(res);
        return res;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    []
  );

  const dismiss = useCallback(() => setResult(null), []);

  return { result, busy, error, commitEdit, fork, dismiss };
}
