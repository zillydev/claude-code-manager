import { describe, it, expect } from "vitest";
import { mergeConfig } from "./useConfig";
import { DEFAULT_CONFIG } from "../lib/config";

describe("mergeConfig", () => {
  it("sets the default mode without touching the other sections", () => {
    const next = mergeConfig(DEFAULT_CONFIG, { appearance: { defaultMode: "truncated" } });
    expect(next.appearance).toEqual({ defaultMode: "truncated" });
    expect(next.ui).toEqual(DEFAULT_CONFIG.ui);
  });

  it("keeps the default mode when another section changes", () => {
    const base = { ...DEFAULT_CONFIG, appearance: { defaultMode: "inputs" } };
    expect(mergeConfig(base, { ui: { detailsOpen: false } }).appearance.defaultMode).toBe("inputs");
  });
});
