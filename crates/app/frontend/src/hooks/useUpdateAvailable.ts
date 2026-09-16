import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** A release newer than this build, once the backend's check has found one. */
export function useUpdateAvailable(): string | null {
  const [latest, setLatest] = useState<string | null>(null);
  useEffect(() => {
    const un = listen<string>("update-available", (e) => setLatest(e.payload));
    // Fills in only if no event beat it, so a stale reply can't replace a newer release.
    invoke<string | null>("update_available")
      .then((v) => setLatest((cur) => cur ?? v))
      .catch(() => {});
    return () => {
      un.then((f) => f());
    };
  }, []);
  return latest;
}
