import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CommitResult } from "../types";

export type EditorPending = { kind: "fork" | "edit" | "delete"; uuid: string } | null;

export function useEditor() {
  const [result, setResult] = useState<CommitResult | null>(null);
  const [pending, setPending] = useState<EditorPending>(null);
  const [error, setError] = useState<string | null>(null);

  const commitEdit = useCallback(
    async (filePath: string, editUuid: string, newText: string) => {
      setPending({ kind: "edit", uuid: editUuid });
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
        setPending(null);
      }
    },
    []
  );

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

  /// Fork v2 — copies the source file verbatim up to the cut instead of
  /// rebuilding a flattened chain, so the new session keeps the full history
  /// (abandoned branches, pre-compaction segments) the source had there.
  const cloneFork = useCallback(
    async (filePath: string, forkUuid: string) => {
      setPending({ kind: "fork", uuid: forkUuid });
      setError(null);
      try {
        const res = await invoke<CommitResult>("clone_session", {
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

  const deleteMessage = useCallback(
    async (filePath: string, messageUuid: string) => {
      setPending({ kind: "delete", uuid: messageUuid });
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
    commitEdit,
    fork,
    cloneFork,
    deleteMessage,
    dismiss,
  };
}
