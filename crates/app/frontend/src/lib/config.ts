// User config: mirrors the Rust `Config` (camelCase). Themes, fonts, and apply
// logic live in the frontend; the backend just persists/broadcasts the JSON.

export interface Config {
  version: number;
  theme: string;
  typography: {
    fontSize: number;
    density: "comfortable" | "compact";
    uiFont: string;
    monoFont: string;
  };
  ui: {
    compact: boolean;
    detailsOpen: boolean;
    sidebarWidth: number;
    sidebarCollapsed: boolean;
    defaultTab: "manager" | "terminal";
  };
}

export const DEFAULT_CONFIG: Config = {
  version: 1,
  theme: "github-dark",
  typography: { fontSize: 14, density: "comfortable", uiFont: "system", monoFont: "system" },
  ui: {
    compact: false,
    detailsOpen: true,
    sidebarWidth: 320,
    sidebarCollapsed: false,
    defaultTab: "manager",
  },
};

/** The 14 `--color-*` variables for each preset theme. */
export const THEMES: Record<string, Record<string, string>> = {
  "github-dark": {
    "--color-bg": "#0d1117",
    "--color-surface": "#161b22",
    "--color-surface-hover": "#1c2128",
    "--color-border": "#30363d",
    "--color-text": "#c9d1d9",
    "--color-text-dim": "#8b949e",
    "--color-accent": "#58a6ff",
    "--color-user": "#79c0ff",
    "--color-assistant": "#7ee787",
    "--color-system": "#d29922",
    "--color-tool": "#d2a8ff",
    "--color-tool-result-bg": "#0d1117",
    "--color-error": "#f85149",
    "--color-thinking": "#484f58",
  },
  light: {
    "--color-bg": "#ffffff",
    "--color-surface": "#f6f8fa",
    "--color-surface-hover": "#eaeef2",
    "--color-border": "#d0d7de",
    "--color-text": "#1f2328",
    "--color-text-dim": "#656d76",
    "--color-accent": "#0969da",
    "--color-user": "#0550ae",
    "--color-assistant": "#1a7f37",
    "--color-system": "#9a6700",
    "--color-tool": "#8250df",
    "--color-tool-result-bg": "#eaeef2",
    "--color-error": "#cf222e",
    "--color-thinking": "#afb8c1",
  },
};

export const THEME_LIST: { key: string; label: string }[] = [
  { key: "github-dark", label: "GitHub Dark" },
  { key: "light", label: "Light" },
  { key: "system", label: "System" },
];

/** Whether the OS currently prefers a dark appearance (defaults to dark when
 *  matchMedia is unavailable, e.g. in node tests). */
export function prefersDark(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches
    : true;
}

/** Resolve a theme setting to a concrete preset key, mapping "system" to the OS
 *  preference and anything unknown to github-dark. Everything that needs to know
 *  which palette is live goes through here — the CSS variables and the
 *  terminal's xterm theme must never disagree about it. */
export function resolveThemeKey(theme: string): string {
  const key = theme === "system" ? (prefersDark() ? "github-dark" : "light") : theme;
  return key in THEMES ? key : "github-dark";
}

/** Resolve a theme value to a concrete palette. */
export function themePalette(theme: string): Record<string, string> {
  return THEMES[resolveThemeKey(theme)];
}

const SANS_DEFAULT = `-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif`;
const MONO_DEFAULT = `ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace`;

export const UI_FONTS: { key: string; label: string; stack: string }[] = [
  { key: "system", label: "System", stack: SANS_DEFAULT },
  { key: "inter", label: "Inter", stack: `Inter, ${SANS_DEFAULT}` },
  { key: "helvetica", label: "Helvetica Neue", stack: `"Helvetica Neue", ${SANS_DEFAULT}` },
];

export const MONO_FONTS: { key: string; label: string; stack: string }[] = [
  { key: "system", label: "System", stack: MONO_DEFAULT },
  { key: "menlo", label: "Menlo", stack: `Menlo, ${MONO_DEFAULT}` },
  { key: "monaco", label: "Monaco", stack: `Monaco, ${MONO_DEFAULT}` },
  { key: "jetbrains-mono", label: "JetBrains Mono", stack: `"JetBrains Mono", ${MONO_DEFAULT}` },
  { key: "fira-code", label: "Fira Code", stack: `"Fira Code", ${MONO_DEFAULT}` },
];

/** Resolve a stored font value (a preset key, or a custom family string) to a
 *  full CSS font-family stack. Unknown values are treated as a custom family. */
export function resolveFont(value: string, kind: "ui" | "mono"): string {
  const list = kind === "ui" ? UI_FONTS : MONO_FONTS;
  const preset = list.find((f) => f.key === value);
  if (preset) return preset.stack;
  const fallback = kind === "ui" ? SANS_DEFAULT : MONO_DEFAULT;
  return `"${value}", ${fallback}`;
}
