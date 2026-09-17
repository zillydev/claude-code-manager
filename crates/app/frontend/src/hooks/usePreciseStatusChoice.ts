import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PreciseStatusChoice } from "../lib/preciseStatus";

/** The backend-owned precise status choice; `auto` also arrives as an event when launch installs the plugin. */
export function usePreciseStatusChoice(): PreciseStatusChoice | null {
  const [choice, setChoice] = useState<PreciseStatusChoice | null>(null);
  useEffect(() => {
    const un = listen("precise-status-auto", () => setChoice("auto"));
    // Fills in only if the event didn't beat it.
    invoke<PreciseStatusChoice>("precise_status_choice")
      .then((c) => setChoice((cur) => cur ?? c))
      .catch(() => {});
    return () => {
      un.then((f) => f());
    };
  }, []);
  return choice;
}
