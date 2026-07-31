import type { ITheme } from "@xterm/xterm";
import { THEMES, resolveThemeKey } from "./config";

/**
 * xterm.js renders to a canvas and cannot read the `--color-*` CSS variables the
 * rest of the app is themed with, so the palette has to be handed to it
 * explicitly and re-pushed whenever the theme changes.
 *
 * The 16 ANSI slots come from GitHub's own `terminal.ansi*` values (the same
 * ones its editor themes ship), not from our 14 semantic variables — those are
 * named for transcript roles (`--color-assistant`, `--color-tool`) and mapping
 * them onto ANSI slots would be arbitrary. Claude itself emits 24-bit colour
 * (the PTY is launched with `COLORTERM=truecolor`) and is unaffected by these;
 * they exist so that everything ELSE you run in the terminal — git, ls, a test
 * runner — stays readable, which matters most on the light background.
 */
const ANSI: Record<string, Record<string, string>> = {
  "github-dark": {
    black: "#484f58",
    red: "#ff7b72",
    green: "#3fb950",
    yellow: "#d29922",
    blue: "#58a6ff",
    magenta: "#bc8cff",
    cyan: "#39c5cf",
    white: "#b1bac4",
    brightBlack: "#6e7681",
    brightRed: "#ffa198",
    brightGreen: "#56d364",
    brightYellow: "#e3b341",
    brightBlue: "#79c0ff",
    brightMagenta: "#d2a8ff",
    brightCyan: "#56d4dd",
    brightWhite: "#f0f6fc",
  },
  light: {
    black: "#24292f",
    red: "#cf222e",
    green: "#116329",
    yellow: "#4d2d00",
    blue: "#0969da",
    magenta: "#8250df",
    cyan: "#1b7c83",
    white: "#6e7781",
    brightBlack: "#57606a",
    brightRed: "#a40e26",
    brightGreen: "#1a7f37",
    brightYellow: "#633c01",
    brightBlue: "#218bff",
    brightMagenta: "#a475f9",
    brightCyan: "#3192aa",
    brightWhite: "#8c959f",
  },
};

/** `#rrggbb` → `rgba(r, g, b, a)`. xterm needs a real alpha for the selection
 *  layer, which is drawn OVER the glyphs — an opaque colour would hide them. */
export function withAlpha(hex: string, alpha: number): string {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return hex;
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/**
 * The xterm theme for a theme setting ("github-dark" | "light" | "system").
 *
 * Background/foreground/cursor come from the app palette so the terminal is the
 * same surface as everything around it; the ANSI set comes from GitHub's.
 */
export function terminalTheme(theme: string): ITheme {
  const key = resolveThemeKey(theme);
  const p = THEMES[key];
  return {
    background: p["--color-bg"],
    foreground: p["--color-text"],
    cursor: p["--color-accent"],
    cursorAccent: p["--color-bg"],
    selectionBackground: withAlpha(p["--color-accent"], 0.3),
    ...ANSI[key],
  };
}
