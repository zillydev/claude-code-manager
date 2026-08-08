import { themePalette, resolveFont, resolveThemeKey, type Config } from "./config";

/** Apply a config to the live DOM: theme palette, font size, density, and fonts
 *  on the root element (default `document.documentElement`; injectable for tests).
 *  (The terminal's mono font is applied separately by TerminalView, since
 *  xterm.js doesn't read CSS variables.) */
export function applyConfig(cfg: Config, root: HTMLElement = document.documentElement) {
  const palette = themePalette(cfg.theme);
  for (const [k, v] of Object.entries(palette)) root.style.setProperty(k, v);
  // Native form controls (the datetime picker + its calendar popover,
  // scrollbars) follow color-scheme, not our CSS vars — without this WKWebView
  // renders them light-mode inside a dark app, and the focused date segment's
  // system highlight lands behind near-invisible text.
  root.style.colorScheme = resolveThemeKey(cfg.theme) === "light" ? "light" : "dark";

  root.style.fontSize = `${cfg.typography.fontSize}px`;
  root.dataset.density = cfg.typography.density === "compact" ? "compact" : "comfortable";
  root.style.setProperty("--font-ui", resolveFont(cfg.typography.uiFont, "ui"));
  root.style.setProperty("--font-mono", resolveFont(cfg.typography.monoFont, "mono"));
}
