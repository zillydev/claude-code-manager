import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type DevRebuild =
  | { phase: "building" }
  | { phase: "relaunching" }
  | { phase: "failed"; output: string };

/**
 * Progress of `make dev`'s Develop → Rebuild & Relaunch (⌘⇧R), broadcast to
 * every window. Null when idle — and always in builds without the launcher,
 * where the backend never emits.
 */
export function useDevRebuild() {
  const [progress, setProgress] = useState<DevRebuild | null>(null);

  useEffect(() => {
    const un = listen<DevRebuild>("dev-rebuild", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const dismiss = useCallback(() => setProgress(null), []);
  const retry = useCallback(() => {
    invoke("dev_rebuild").catch(() => {});
  }, []);

  return { progress, dismiss, retry };
}
