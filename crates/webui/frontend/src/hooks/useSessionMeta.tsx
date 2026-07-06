import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  DEFAULT_META,
  setAliasIn,
  setNoteIn,
  setSessionTagsIn,
  upsertTagIn,
  removeTagIn,
  addSessionTagIn,
  reconcileMeta,
  type SessionMeta,
} from "../lib/sessionMeta";

interface SessionMetaCtx {
  meta: SessionMeta;
  setAlias: (id: string, alias: string | null) => void;
  setNote: (id: string, note: string | null) => void;
  setSessionTags: (id: string, names: string[]) => void;
  /** Atomic create-if-missing + add-to-session (one write; see addSessionTagIn). */
  addSessionTag: (id: string, name: string) => void;
  upsertTag: (name: string, color: string) => void;
  removeTag: (name: string) => void;
}

const Ctx = createContext<SessionMetaCtx>({
  meta: DEFAULT_META,
  setAlias: () => {},
  setNote: () => {},
  setSessionTags: () => {},
  addSessionTag: () => {},
  upsertTag: () => {},
  removeTag: () => {},
});

export function useSessionMeta() {
  return useContext(Ctx);
}

export function SessionMetaProvider({ children }: { children: ReactNode }) {
  const [meta, setMeta] = useState<SessionMeta>(DEFAULT_META);
  const metaRef = useRef(meta);
  metaRef.current = meta;
  // JSONs of our in-flight writes, so we ignore the broadcast echo of each of
  // our own saves (a Set, not a single string: rapid writes each echo back).
  const inFlight = useRef<Set<string>>(new Set());

  useEffect(() => {
    let alive = true;
    invoke<SessionMeta>("get_session_meta")
      .then((m) => {
        if (!alive) return;
        // Self-heal legacy data: register any session tag missing from the
        // registry (else it never shows in the selector) and persist once.
        const healed = reconcileMeta(m);
        metaRef.current = healed;
        setMeta(healed);
        if (healed !== m) {
          inFlight.current.add(JSON.stringify(healed));
          invoke("set_session_meta", { meta: healed }).catch(() => {});
        }
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    const un = listen<SessionMeta>("session-meta-changed", (e) => {
      const json = JSON.stringify(e.payload);
      if (inFlight.current.has(json)) {
        inFlight.current.delete(json); // our own echo
        return;
      }
      setMeta(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Apply a pure mutation, set optimistically, persist (broadcasts to others).
  // metaRef is updated HERE (not just on render) so two apply() calls in the
  // same event handler chain correctly instead of the second clobbering the
  // first — the cause of the lost-registry-entry tag bug.
  const apply = useCallback((fn: (m: SessionMeta) => SessionMeta) => {
    const next = fn(metaRef.current);
    metaRef.current = next;
    setMeta(next);
    inFlight.current.add(JSON.stringify(next));
    invoke("set_session_meta", { meta: next }).catch(() => {});
  }, []);

  const value: SessionMetaCtx = {
    meta,
    setAlias: (id, alias) => apply((m) => setAliasIn(m, id, alias)),
    setNote: (id, note) => apply((m) => setNoteIn(m, id, note)),
    setSessionTags: (id, names) => apply((m) => setSessionTagsIn(m, id, names)),
    addSessionTag: (id, name) => apply((m) => addSessionTagIn(m, id, name)),
    upsertTag: (name, color) => apply((m) => upsertTagIn(m, name, color)),
    removeTag: (name) => apply((m) => removeTagIn(m, name)),
  };

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
