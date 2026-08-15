import { describe, it, expect } from "vitest";
import { THEMES, resolveFont, themePalette, DEFAULT_CONFIG, type Config } from "./config";
import { applyConfig } from "./applyConfig";

describe("themes", () => {
  it("every theme defines the same vars (14 color + 6 syntax)", () => {
    const keys = Object.keys(THEMES["github-dark"]).sort();
    expect(keys).toHaveLength(20);
    for (const [name, palette] of Object.entries(THEMES)) {
      expect(Object.keys(palette).sort(), `theme ${name}`).toEqual(keys);
    }
  });
});

describe("themePalette", () => {
  it("maps 'system' to a concrete palette (dark when matchMedia is absent)", () => {
    expect(themePalette("system")).toEqual(THEMES["github-dark"]);
  });
  it("falls back to github-dark for an unknown theme", () => {
    expect(themePalette("nope")).toEqual(THEMES["github-dark"]);
  });
});

describe("resolveFont", () => {
  it("resolves a preset key to its stack", () => {
    expect(resolveFont("menlo", "mono")).toContain("Menlo");
    expect(resolveFont("inter", "ui")).toContain("Inter");
  });
  it("treats an unknown value as a quoted custom family", () => {
    expect(resolveFont("Comic Sans MS", "ui")).toContain(`"Comic Sans MS"`);
  });
});

// A minimal root stand-in so applyConfig is testable without a DOM library.
function makeRoot() {
  const props: Record<string, string> = {};
  return {
    style: {
      setProperty(k: string, v: string) {
        props[k] = v;
      },
      getPropertyValue(k: string) {
        return props[k] ?? "";
      },
      fontSize: "",
    },
    dataset: {} as Record<string, string>,
  };
}

describe("applyConfig", () => {
  it("writes theme vars, font size, density, and fonts", () => {
    const root = makeRoot();
    const cfg: Config = {
      ...DEFAULT_CONFIG,
      theme: "light",
      typography: { fontSize: 16, density: "compact", uiFont: "inter", monoFont: "menlo" },
    };
    applyConfig(cfg, root as unknown as HTMLElement);
    expect(root.style.getPropertyValue("--color-bg")).toBe(THEMES.light["--color-bg"]);
    expect(root.style.fontSize).toBe("16px");
    expect(root.dataset.density).toBe("compact");
    expect(root.style.getPropertyValue("--font-ui")).toContain("Inter");
    expect(root.style.getPropertyValue("--font-mono")).toContain("Menlo");
  });

  it("falls back to github-dark for an unknown theme", () => {
    const root = makeRoot();
    applyConfig({ ...DEFAULT_CONFIG, theme: "nope" }, root as unknown as HTMLElement);
    expect(root.style.getPropertyValue("--color-bg")).toBe(THEMES["github-dark"]["--color-bg"]);
  });
});
