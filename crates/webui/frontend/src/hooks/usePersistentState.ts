import { useEffect, useState } from "react";

/**
 * A `useState` whose value is persisted to localStorage under `key`: it is
 * restored on mount and written back whenever it changes, so UI preferences
 * survive app restarts. The Tauri webview persists localStorage across launches.
 */
export function usePersistentState<T>(
  key: string,
  initial: T
): [T, React.Dispatch<React.SetStateAction<T>>] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(key);
      return raw === null ? initial : (JSON.parse(raw) as T);
    } catch {
      return initial;
    }
  });

  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      // Ignore write failures (e.g. storage unavailable/full).
    }
  }, [key, value]);

  return [value, setValue];
}
