import { THEMES, resolveFont, type Config } from "./config";

/** Apply a config to the live DOM: theme palette, font size, density, and fonts
 *  on the root element (default `document.documentElement`; injectable for tests).
 *  (The terminal's mono font is applied separately by TerminalView, since
 *  xterm.js doesn't read CSS variables.) */
export function applyConfig(cfg: Config, root: HTMLElement = document.documentElement) {
  const palette = THEMES[cfg.theme] ?? THEMES["github-dark"];
  for (const [k, v] of Object.entries(palette)) root.style.setProperty(k, v);

  root.style.fontSize = `${cfg.typography.fontSize}px`;
  root.dataset.density = cfg.typography.density === "compact" ? "compact" : "comfortable";
  root.style.setProperty("--font-ui", resolveFont(cfg.typography.uiFont, "ui"));
  root.style.setProperty("--font-mono", resolveFont(cfg.typography.monoFont, "mono"));
}
