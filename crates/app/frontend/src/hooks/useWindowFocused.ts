import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Whether THIS window is currently focused.
 *
 * Drives what counts as "viewed": a session live-tailing in a window you are
 * not looking at — on another Space, behind another app — must not mark itself
 * read as its turns arrive. Native window focus (not `document.hasFocus()`
 * alone) is the signal, since that is what changes when you switch Spaces or
 * apps; the DOM value only seeds the initial state before the first event.
 */
export function useWindowFocused(): boolean {
  const [focused, setFocused] = useState(() =>
    typeof document !== "undefined" ? document.hasFocus() : true,
  );
  useEffect(() => {
    const un = getCurrentWindow().onFocusChanged(({ payload }) => setFocused(payload));
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);
  return focused;
}
