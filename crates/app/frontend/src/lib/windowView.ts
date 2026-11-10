// Per-window transcript view state. Each window owns its own copy. The mode is
// seeded from Settings' default; Details from the config's last-used value.
import { DEFAULT_CONFIG, type Config } from "./config";
import { modeById } from "./appearance";

export interface WindowView {
  /** A preset id, always one `modeById` knows. */
  modeId: string;
  /** Bumped on every switch, so state kept from before a switch is dropped even after switching back. */
  modeSeq: number;
  detailsOpen: boolean;
  /** Set once the view has taken its seed or the user has toggled it; after
   *  that, config changes from other windows no longer touch it. */
  seeded: boolean;
}

export type WindowViewPatch = Partial<Pick<WindowView, "modeId" | "detailsOpen">>;

export function initialWindowView(): WindowView {
  return {
    modeId: modeById(DEFAULT_CONFIG.appearance.defaultMode).id,
    modeSeq: 0,
    detailsOpen: DEFAULT_CONFIG.ui.detailsOpen,
    seeded: false,
  };
}

/** Adopt the config's values once it has loaded; otherwise a no-op. */
export function seedWindowView(v: WindowView, cfg: Config, loaded: boolean): WindowView {
  if (!loaded || v.seeded) return v;
  return {
    ...v,
    modeId: modeById(cfg.appearance.defaultMode).id,
    detailsOpen: cfg.ui.detailsOpen,
    seeded: true,
  };
}

/** Apply a user toggle. It counts as seeded so a late config load can't undo it. */
export function toggleWindowView(v: WindowView, patch: WindowViewPatch): WindowView {
  const switched = patch.modeId !== undefined && patch.modeId !== v.modeId;
  return { ...v, ...patch, modeSeq: switched ? v.modeSeq + 1 : v.modeSeq, seeded: true };
}
