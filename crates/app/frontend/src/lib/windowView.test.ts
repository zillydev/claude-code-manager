import { describe, expect, it } from "vitest";
import { DEFAULT_CONFIG, type Config } from "./config";
import { initialWindowView, seedWindowView, toggleWindowView } from "./windowView";

const cfg = (ui: Partial<Config["ui"]>, defaultMode = "default"): Config => ({
  ...DEFAULT_CONFIG,
  ui: { ...DEFAULT_CONFIG.ui, ...ui },
  appearance: { defaultMode },
});

describe("windowView", () => {
  it("starts from the defaults, unseeded", () => {
    expect(initialWindowView()).toEqual({ modeId: "default", modeSeq: 0, detailsOpen: true, seeded: false });
  });

  it("does not seed before the real config has loaded", () => {
    const v = initialWindowView();
    expect(seedWindowView(v, cfg({ detailsOpen: false }, "inputs"), false)).toBe(v);
  });

  it("seeds the mode from Settings' default and Details from its last-used value", () => {
    const v = seedWindowView(initialWindowView(), cfg({ detailsOpen: false }, "conversation"), true);
    expect(v).toEqual({ modeId: "conversation", modeSeq: 0, detailsOpen: false, seeded: true });
  });

  it("seeds an unknown or retired default mode as Default", () => {
    for (const id of ["gone", "skeleton"]) {
      expect(seedWindowView(initialWindowView(), cfg({}, id), true).modeId).toBe("default");
    }
  });

  it("ignores a later config change from another window", () => {
    const seeded = seedWindowView(initialWindowView(), cfg({}), true);
    expect(seedWindowView(seeded, cfg({ detailsOpen: false }, "inputs"), true)).toBe(seeded);
  });

  it("keeps a switch made before the config loads", () => {
    const toggled = toggleWindowView(initialWindowView(), { modeId: "inputs" });
    const after = seedWindowView(toggled, cfg({ detailsOpen: false }, "everything"), true);
    expect(after).toEqual({ modeId: "inputs", modeSeq: 1, detailsOpen: true, seeded: true });
  });

  it("changes one key and leaves the other alone", () => {
    const v = initialWindowView();
    expect(toggleWindowView(v, { modeId: "inputs" })).toEqual({
      modeId: "inputs",
      modeSeq: 1,
      detailsOpen: true,
      seeded: true,
    });
    expect(toggleWindowView(v, { detailsOpen: false })).toEqual({
      modeId: "default",
      modeSeq: 0,
      detailsOpen: false,
      seeded: true,
    });
  });

  it("counts every switch, so a switch back to a mode is still a new switch", () => {
    let v = initialWindowView();
    v = toggleWindowView(v, { modeId: "inputs" });
    v = toggleWindowView(v, { modeId: "default" });
    expect(v).toMatchObject({ modeId: "default", modeSeq: 2 });
  });

  it("does not count picking the mode already showing", () => {
    expect(toggleWindowView(initialWindowView(), { modeId: "default" }).modeSeq).toBe(0);
  });
});
