import { describe, it, expect, afterEach, vi } from "vitest";
import { terminalTheme, withAlpha } from "./terminalTheme";
import { THEMES } from "./config";

/** Force `prefers-color-scheme: dark` to a known value. These tests run under
 *  the node environment, where there is no `window` at all — so stub the whole
 *  object, not just matchMedia, or `prefersDark()` short-circuits to its
 *  "matchMedia is absent → assume dark" default and both cases look dark. */
function setOsDark(dark: boolean) {
  vi.stubGlobal("window", {
    matchMedia: (q: string) => ({ matches: q.includes("dark") ? dark : !dark }),
  });
}

afterEach(() => vi.unstubAllGlobals());

describe("withAlpha", () => {
  it("converts #rrggbb to rgba", () => {
    expect(withAlpha("#58a6ff", 0.3)).toBe("rgba(88, 166, 255, 0.3)");
  });
  it("passes anything unparseable straight through", () => {
    expect(withAlpha("transparent", 0.3)).toBe("transparent");
  });
});

describe("terminalTheme", () => {
  it("takes background and foreground from the app palette, not xterm defaults", () => {
    // The terminal must be the same surface as the pane around it — the old
    // hardcoded #0a0a0a matched neither palette.
    const t = terminalTheme("github-dark");
    expect(t.background).toBe(THEMES["github-dark"]["--color-bg"]);
    expect(t.foreground).toBe(THEMES["github-dark"]["--color-text"]);
    expect(t.background).not.toBe("#0a0a0a");
  });

  it("gives the light theme a light background", () => {
    const t = terminalTheme("light");
    expect(t.background).toBe(THEMES.light["--color-bg"]);
    expect(t.background).toBe("#ffffff");
  });

  it("ships a full 16-colour ANSI set, so non-truecolor output stays readable", () => {
    // Claude emits 24-bit colour, but git/ls/test runners use the ANSI slots —
    // xterm's dark-tuned defaults would be unreadable on the light background.
    const slots = [
      "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
      "brightBlack", "brightRed", "brightGreen", "brightYellow",
      "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
    ] as const;
    for (const key of ["github-dark", "light"]) {
      const t = terminalTheme(key) as Record<string, string>;
      for (const s of slots) {
        expect(t[s], `${key}.${s}`).toMatch(/^#[0-9a-f]{6}$/i);
      }
    }
  });

  it("uses distinct ANSI sets per theme", () => {
    expect(terminalTheme("github-dark").red).not.toBe(terminalTheme("light").red);
  });

  it("selection is translucent so glyphs stay visible under it", () => {
    expect(terminalTheme("github-dark").selectionBackground).toMatch(/^rgba\(/);
  });

  it("resolves \"system\" by the OS preference", () => {
    setOsDark(true);
    expect(terminalTheme("system").background).toBe(THEMES["github-dark"]["--color-bg"]);
    setOsDark(false);
    expect(terminalTheme("system").background).toBe(THEMES.light["--color-bg"]);
  });

  it("falls back to github-dark for an unknown theme", () => {
    expect(terminalTheme("solarized-mango").background).toBe(
      THEMES["github-dark"]["--color-bg"],
    );
  });
});
