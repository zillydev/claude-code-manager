import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { useConfig } from "./useConfig";
import {
  initialWindowView,
  seedWindowView,
  toggleWindowView,
  type WindowViewPatch,
} from "../lib/windowView";

interface WindowViewCtx {
  compact: boolean;
  detailsOpen: boolean;
  setCompact: (v: boolean) => void;
  setDetailsOpen: (v: boolean) => void;
}

const Ctx = createContext<WindowViewCtx>({
  compact: false,
  detailsOpen: true,
  setCompact: () => {},
  setDetailsOpen: () => {},
});

export function useWindowView() {
  return useContext(Ctx);
}

/** This window's Compact/Details state. Mounted above Transcript so it
 *  survives closing and reopening sessions. */
export function WindowViewProvider({ children }: { children: ReactNode }) {
  const { config, loaded, setConfig } = useConfig();
  const [view, setView] = useState(initialWindowView);

  useEffect(() => {
    setView((v) => seedWindowView(v, config, loaded));
  }, [config, loaded]);

  // The config write records the last-used value; other windows have already
  // seeded, so the broadcast doesn't change them.
  const apply = useCallback(
    (patch: WindowViewPatch) => {
      setView((v) => toggleWindowView(v, patch));
      setConfig({ ui: patch });
    },
    [setConfig],
  );
  const setCompact = useCallback((compact: boolean) => apply({ compact }), [apply]);
  const setDetailsOpen = useCallback((detailsOpen: boolean) => apply({ detailsOpen }), [apply]);

  return (
    <Ctx.Provider
      value={{ compact: view.compact, detailsOpen: view.detailsOpen, setCompact, setDetailsOpen }}
    >
      {children}
    </Ctx.Provider>
  );
}
