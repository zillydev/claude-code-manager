import { describe, it, expect } from "vitest";
import { THEMES, resolveFont, themePalette, DEFAULT_CONFIG, type Config } from "./config";
import { applyConfig } from "./applyConfig";

describe("themes", () => {
  it("every theme defines the same vars (16 color + 6 syntax)", () => {
    const keys = Object.keys(THEMES["github-dark"]).sort();
    expect(keys).toHaveLength(22);
    for (const [name, palette] of Object.entries(THEMES)) {
      expect(Object.keys(palette).sort(), `theme ${name}`).toEqual(keys);
    }
  });

  it("the active find match is a solid fill its own text stays readable on", () => {
    // WCAG relative luminance and contrast ratio; 4.5:1 is the body-text minimum.
    const lum = (hex: string) => {
      const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
      const [r, g, b] = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    for (const [name, palette] of Object.entries(THEMES)) {
      const fill = palette["--color-find-active"];
      const text = palette["--color-find-active-text"];
      expect(fill, `theme ${name}`).toMatch(/^#[0-9a-f]{6}$/i);
      expect(text, `theme ${name}`).toMatch(/^#[0-9a-f]{6}$/i);
      const [hi, lo] = [lum(fill), lum(text)].sort((a, b) => b - a);
      expect((hi + 0.05) / (lo + 0.05), `theme ${name}`).toBeGreaterThanOrEqual(4.5);
      expect(fill.toLowerCase(), `theme ${name}: not the other matches' amber`).not.toBe(
        palette["--color-system"].toLowerCase(),
      );
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

  it("publishes the terminal's ANSI slots as --ansi-N, following the theme", () => {
    const root = makeRoot();
    applyConfig({ ...DEFAULT_CONFIG, theme: "github-dark" }, root as unknown as HTMLElement);
    expect(root.style.getPropertyValue("--ansi-2")).toBe("#3fb950");
    expect(root.style.getPropertyValue("--ansi-15")).toMatch(/^#[0-9a-f]{6}$/);
    applyConfig({ ...DEFAULT_CONFIG, theme: "light" }, root as unknown as HTMLElement);
    expect(root.style.getPropertyValue("--ansi-2")).toBe("#116329");
  });

  it("falls back to github-dark for an unknown theme", () => {
    const root = makeRoot();
    applyConfig({ ...DEFAULT_CONFIG, theme: "nope" }, root as unknown as HTMLElement);
    expect(root.style.getPropertyValue("--color-bg")).toBe(THEMES["github-dark"]["--color-bg"]);
  });
});
