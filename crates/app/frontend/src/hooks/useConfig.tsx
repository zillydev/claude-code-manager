import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { DEFAULT_CONFIG, prefersDark, resolveThemeKey, type Config } from "../lib/config";
import { applyConfig } from "../lib/applyConfig";

type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? Partial<T[K]> : T[K] };

export function mergeConfig(base: Config, p: DeepPartial<Config>): Config {
  return {
    ...base,
    ...p,
    typography: { ...base.typography, ...(p.typography ?? {}) },
    ui: { ...base.ui, ...(p.ui ?? {}) },
    appearance: { ...base.appearance, ...(p.appearance ?? {}) },
    integrations: { ...base.integrations, ...(p.integrations ?? {}) },
  };
}

interface ConfigCtx {
  config: Config;
  /** True once the real config has loaded from disk (vs the initial default). */
  loaded: boolean;
  /** The concrete palette key in force ("github-dark" | "light") — `config.theme`
   *  with "system" already resolved. Unlike `config.theme` this CHANGES when the
   *  OS flips, which is what anything themed by value (rather than by CSS
   *  variable) has to react to. */
  resolvedTheme: string;
  /** Merge a partial change → apply live → persist (broadcasts to other windows). */
  setConfig: (partial: DeepPartial<Config>) => void;
}

const Ctx = createContext<ConfigCtx>({
  config: DEFAULT_CONFIG,
  loaded: false,
  resolvedTheme: resolveThemeKey(DEFAULT_CONFIG.theme),
  setConfig: () => {},
});

export function useConfig() {
  return useContext(Ctx);
}

export function ConfigProvider({ children }: { children: ReactNode }) {
  const [config, setConfigState] = useState<Config>(DEFAULT_CONFIG);
  const [loaded, setLoaded] = useState(false);
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
      .catch(() => applyConfig(DEFAULT_CONFIG))
      .finally(() => {
        if (alive) setLoaded(true);
      });
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

  // Track the OS light/dark preference. Listen unconditionally: the flip has to
  // re-render consumers even while the theme is a fixed preset, or `osDark` goes
  // stale and a later switch to "system" resolves against the wrong value.
  const [osDark, setOsDark] = useState(prefersDark);
  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      setOsDark(mq.matches);
      if (configRef.current.theme === "system") applyConfig(configRef.current);
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  // `osDark` isn't read here — resolveThemeKey re-reads the media query itself —
  // but listing it as a dep is what makes this recompute when the OS flips.
  const resolvedTheme = useMemo(
    () => resolveThemeKey(config.theme),
    [config.theme, osDark],
  );

  const setConfig = useCallback((partial: DeepPartial<Config>) => {
    const next = mergeConfig(configRef.current, partial);
    setConfigState(next);
    applyConfig(next);
    lastWritten.current = JSON.stringify(next);
    invoke("set_config", { config: next }).catch(() => {});
  }, []);

  return (
    <Ctx.Provider value={{ config, loaded, resolvedTheme, setConfig }}>{children}</Ctx.Provider>
  );
}
