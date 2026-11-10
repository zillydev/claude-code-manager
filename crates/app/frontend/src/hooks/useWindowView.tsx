import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { useConfig } from "./useConfig";
import { initialWindowView, seedWindowView, toggleWindowView } from "../lib/windowView";
import { DEFAULT_MODE, modeById, type AppearanceMode } from "../lib/appearance";

interface WindowViewCtx {
  /** A frozen preset, so its identity changes only when the user switches. */
  mode: AppearanceMode;
  /** Bumps on every switch; per-part clicks are kept against it. */
  modeSeq: number;
  detailsOpen: boolean;
  setMode: (id: string) => void;
  setDetailsOpen: (v: boolean) => void;
}

const Ctx = createContext<WindowViewCtx>({
  mode: DEFAULT_MODE,
  modeSeq: 0,
  detailsOpen: true,
  setMode: () => {},
  setDetailsOpen: () => {},
});

export function useWindowView() {
  return useContext(Ctx);
}

/** This window's appearance mode and Details state. Mounted above Transcript
 *  so it survives closing and reopening sessions. */
export function WindowViewProvider({ children }: { children: ReactNode }) {
  const { config, loaded, setConfig } = useConfig();
  const [view, setView] = useState(initialWindowView);

  useEffect(() => {
    setView((v) => seedWindowView(v, config, loaded));
  }, [config, loaded]);

  // No config write: a window's mode is its own, and new windows start from Settings' default.
  const setMode = useCallback((modeId: string) => {
    setView((v) => toggleWindowView(v, { modeId }));
  }, []);
  // The config write records the last-used value; other windows have already
  // seeded, so the broadcast doesn't change them.
  const setDetailsOpen = useCallback(
    (detailsOpen: boolean) => {
      setView((v) => toggleWindowView(v, { detailsOpen }));
      setConfig({ ui: { detailsOpen } });
    },
    [setConfig],
  );

  return (
    <Ctx.Provider
      value={{
        mode: modeById(view.modeId),
        modeSeq: view.modeSeq,
        detailsOpen: view.detailsOpen,
        setMode,
        setDetailsOpen,
      }}
    >
      {children}
    </Ctx.Provider>
  );
}
