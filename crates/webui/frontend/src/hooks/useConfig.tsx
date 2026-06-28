import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { DEFAULT_CONFIG, type Config } from "../lib/config";
import { applyConfig } from "../lib/applyConfig";

type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? Partial<T[K]> : T[K] };

function mergeConfig(base: Config, p: DeepPartial<Config>): Config {
  return {
    ...base,
    ...p,
    typography: { ...base.typography, ...(p.typography ?? {}) },
    ui: { ...base.ui, ...(p.ui ?? {}) },
  };
}

interface ConfigCtx {
  config: Config;
  /** Merge a partial change → apply live → persist (broadcasts to other windows). */
  setConfig: (partial: DeepPartial<Config>) => void;
}

const Ctx = createContext<ConfigCtx>({ config: DEFAULT_CONFIG, setConfig: () => {} });

export function useConfig() {
  return useContext(Ctx);
}

export function ConfigProvider({ children }: { children: ReactNode }) {
  const [config, setConfigState] = useState<Config>(DEFAULT_CONFIG);
  const configRef = useRef(config);
  configRef.current = config;
  // JSON of our last write, so we ignore the config-changed echo of our own save.
  const lastWritten = useRef<string>("");

  useEffect(() => {
    let alive = true;
    invoke<Config>("get_config")
      .then((cfg) => {
        if (!alive) return;
        setConfigState(cfg);
        applyConfig(cfg);
      })
      .catch(() => applyConfig(DEFAULT_CONFIG));
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    const un = listen<Config>("config-changed", (e) => {
      if (JSON.stringify(e.payload) === lastWritten.current) return; // our own echo
      setConfigState(e.payload);
      applyConfig(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const setConfig = (partial: DeepPartial<Config>) => {
    const next = mergeConfig(configRef.current, partial);
    setConfigState(next);
    applyConfig(next);
    lastWritten.current = JSON.stringify(next);
    invoke("set_config", { config: next }).catch(() => {});
  };

  return <Ctx.Provider value={{ config, setConfig }}>{children}</Ctx.Provider>;
}
