import { useEffect, useRef, useState } from "react";

/**
 * A `useState` whose value is persisted to localStorage under `key`: it is
 * restored on mount, written back whenever it changes (so UI preferences survive
 * app restarts), and synced across windows — when another window of the same app
 * changes the key, this window adopts the new value. A `lastWritten` guard skips
 * our own echoes so the cross-window sync can't loop.
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

  // What this window last wrote — used to ignore our own storage echoes.
  const lastWritten = useRef<string>(JSON.stringify(value));

  useEffect(() => {
    const s = JSON.stringify(value);
    lastWritten.current = s;
    try {
      localStorage.setItem(key, s);
    } catch {
      // Ignore write failures (e.g. storage unavailable/full).
    }
  }, [key, value]);

  // Another window changed this key → adopt it (unless it's our own echo).
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key !== key || e.newValue == null) return;
      if (e.newValue === lastWritten.current) return;
      try {
        setValue(JSON.parse(e.newValue) as T);
      } catch {
        // Ignore malformed values.
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [key]);

  return [value, setValue];
}
