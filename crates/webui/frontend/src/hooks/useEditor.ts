import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CommitResult } from "../types";

export function useEditor() {
  const [result, setResult] = useState<CommitResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const commitEdit = useCallback(
    async (filePath: string, editUuid: string, newText: string) => {
      setBusy(true);
      setError(null);
      try {
        const res = await invoke<CommitResult>("commit_session", {
          filePath,
          editUuid,
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
    async (filePath: string, forkUuid: string) => {
      setBusy(true);
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
        setBusy(false);
      }
    },
    []
  );

  const deleteMessage = useCallback(
    async (filePath: string, messageUuid: string) => {
      setBusy(true);
      setError(null);
      try {
        const res = await invoke<CommitResult>("delete_message", {
          filePath,
          messageUuid,
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

  return { result, busy, error, commitEdit, fork, deleteMessage, dismiss };
}
