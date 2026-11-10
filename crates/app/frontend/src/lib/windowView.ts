// Per-window transcript view state. Each window owns its own copy; the config
// only records the last-used value, which a window reads once as its seed.
import { DEFAULT_CONFIG, type Config } from "./config";

export interface WindowView {
  compact: boolean;
  detailsOpen: boolean;
  /** Set once the view has taken its seed or the user has toggled it; after
   *  that, config changes from other windows no longer touch it. */
  seeded: boolean;
}

export type WindowViewPatch = Partial<Pick<WindowView, "compact" | "detailsOpen">>;

export function initialWindowView(): WindowView {
  const { compact, detailsOpen } = DEFAULT_CONFIG.ui;
  return { compact, detailsOpen, seeded: false };
}

/** Adopt the config's last-used values once it has loaded; otherwise a no-op. */
export function seedWindowView(v: WindowView, cfg: Config, loaded: boolean): WindowView {
  if (!loaded || v.seeded) return v;
  return { compact: cfg.ui.compact, detailsOpen: cfg.ui.detailsOpen, seeded: true };
}

/** Apply a user toggle. It counts as seeded so a late config load can't undo it. */
export function toggleWindowView(v: WindowView, patch: WindowViewPatch): WindowView {
  return { ...v, ...patch, seeded: true };
}
