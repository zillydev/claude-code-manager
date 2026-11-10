import { describe, expect, it } from "vitest";
import { DEFAULT_CONFIG, type Config } from "./config";
import { initialWindowView, seedWindowView, toggleWindowView } from "./windowView";

const cfg = (ui: Partial<Config["ui"]>): Config => ({
  ...DEFAULT_CONFIG,
  ui: { ...DEFAULT_CONFIG.ui, ...ui },
});

describe("windowView", () => {
  it("starts from the defaults, unseeded", () => {
    expect(initialWindowView()).toEqual({ compact: false, detailsOpen: true, seeded: false });
  });

  it("does not seed before the real config has loaded", () => {
    const v = initialWindowView();
    expect(seedWindowView(v, cfg({ compact: true, detailsOpen: false }), false)).toBe(v);
  });

  it("seeds from the last-used values once loaded", () => {
    const v = seedWindowView(initialWindowView(), cfg({ compact: true, detailsOpen: false }), true);
    expect(v).toEqual({ compact: true, detailsOpen: false, seeded: true });
  });

  it("ignores a later config change from another window", () => {
    const seeded = seedWindowView(initialWindowView(), cfg({}), true);
    expect(seedWindowView(seeded, cfg({ compact: true, detailsOpen: false }), true)).toBe(seeded);
  });

  it("keeps a toggle made before the config loads", () => {
    const toggled = toggleWindowView(initialWindowView(), { compact: true });
    const after = seedWindowView(toggled, cfg({ compact: false, detailsOpen: false }), true);
    expect(after).toEqual({ compact: true, detailsOpen: true, seeded: true });
  });

  it("toggles one key and leaves the other alone", () => {
    const v = initialWindowView();
    expect(toggleWindowView(v, { compact: true })).toEqual({
      compact: true,
      detailsOpen: true,
      seeded: true,
    });
    expect(toggleWindowView(v, { detailsOpen: false })).toEqual({
      compact: false,
      detailsOpen: false,
      seeded: true,
    });
  });
});
