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
  type SessionMeta,
} from "../lib/sessionMeta";

interface SessionMetaCtx {
  meta: SessionMeta;
  setAlias: (id: string, alias: string | null) => void;
  setNote: (id: string, note: string | null) => void;
  setSessionTags: (id: string, names: string[]) => void;
  upsertTag: (name: string, color: string) => void;
  removeTag: (name: string) => void;
}

const Ctx = createContext<SessionMetaCtx>({
  meta: DEFAULT_META,
  setAlias: () => {},
  setNote: () => {},
  setSessionTags: () => {},
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
  // JSON of our last write, so we ignore the broadcast echo of our own save.
  const lastWritten = useRef<string>("");

  useEffect(() => {
    let alive = true;
    invoke<SessionMeta>("get_session_meta")
      .then((m) => {
        if (alive) setMeta(m);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    const un = listen<SessionMeta>("session-meta-changed", (e) => {
      if (JSON.stringify(e.payload) === lastWritten.current) return; // our own echo
      setMeta(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Apply a pure mutation, set optimistically, persist (broadcasts to others).
  const apply = useCallback((fn: (m: SessionMeta) => SessionMeta) => {
    const next = fn(metaRef.current);
    setMeta(next);
    lastWritten.current = JSON.stringify(next);
    invoke("set_session_meta", { meta: next }).catch(() => {});
  }, []);

  const value: SessionMetaCtx = {
    meta,
    setAlias: (id, alias) => apply((m) => setAliasIn(m, id, alias)),
    setNote: (id, note) => apply((m) => setNoteIn(m, id, note)),
    setSessionTags: (id, names) => apply((m) => setSessionTagsIn(m, id, names)),
    upsertTag: (name, color) => apply((m) => upsertTagIn(m, name, color)),
    removeTag: (name) => apply((m) => removeTagIn(m, name)),
  };

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
