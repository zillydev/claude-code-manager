# UI customization — implementation plan

Spec: `docs/superpowers/specs/2026-06-28-ui-customization-design.md`.
Each phase ends green (`cargo test` / `tsc` / `vitest` / `vite build`) and is one commit.

## Phase 1 — backend config (Rust)
- `crates/webui/src/config.rs`:
  - `Config { version, theme, typography: Typography, ui: Ui }`, `Typography { font_size, density, ui_font, mono_font }`, `Ui { compact, details_open, sidebar_width, sidebar_collapsed, default_tab }` — serde with `#[serde(default)]` per field; `Default` = schema defaults.
  - `config_path()` — `$CLAUDE_MANAGER_CONFIG_DIR` → `$XDG_CONFIG_HOME/claude-manager` → `~/.config/claude-manager`, then `config.json`.
  - `load()` — read+parse+clamp(font_size 12..=18); missing/bad → defaults + write.
  - `save(&Config)` — atomic temp-write + rename (creates dir).
- `commands.rs`: `get_config()`, `set_config(config)` → save + `app.emit("config-changed", &config)`.
- `lib.rs`: `mod config;` + register both commands.
- Tests: path precedence, load-missing→defaults+write, parse-partial→merge, clamp, save→load roundtrip.

## Phase 2 — frontend config context
- `lib/config.ts`: `Config` type; `DEFAULT_CONFIG`; `THEMES: Record<string, Record<string,string>>` (github-dark = current 14 vars, light = new); `FONTS = { ui: {...}, mono: {...} }` preset→stack; `resolveFont(key)`.
- `lib/applyConfig.ts`: `applyConfig(cfg)` — set theme vars + `fontSize` + `dataset.density` + `--font-ui`/`--font-mono` on `document.documentElement`.
- `hooks/useConfig.tsx`: `ConfigProvider` (load via `get_config`, apply, `setConfig(partial)` = merge+apply+`set_config`, listen `config-changed`) + `useConfig()`. Mount in `main.tsx`.
- Tests: `applyConfig` writes expected style/dataset (jsdom); every theme has all 14 keys.

## Phase 3 — CSS
- `index.css`: `html,body { font-family: var(--font-ui, <current stack>) }`; `[data-density="compact"]` curated block (transcript `space-y`, turn/row/header padding, prose line-height); prose code/pre → `var(--font-mono, …)`.
- Light palette values in `THEMES.light`.

## Phase 4 — unification refactor
- `Transcript`: read `compact`/`detailsOpen` from `useConfig()`, toggles call `setConfig`; drop the two `usePersistentState` calls.
- `App`: `sidebarWidth`/`sidebarCollapsed`/`activeTab` from config; resize drag writes debounced (~300ms); ⌘B + collapse/expand write config; `activeTab` inits from `defaultTab`, NOT rewritten on toggle.
- `TerminalView`: `fontFamily` from config mono font; update + refit on change.

## Phase 5 — Settings panel
- `components/Settings.tsx`: modal (Esc/backdrop, like LineageTree) — Theme swatches; font-size stepper; density toggle; UI + Mono `<select>` (presets + "Custom…" text); behavior toggles incl. default tab. All via `setConfig`.
- `⚙` button in `Sidebar` header (beside `↻`/`«`) → opens it.

## Phase 6 — verify, FEATURES.md, finish
- Full `cargo test` + frontend tsc/vitest/build.
- FEATURES.md entry under a new "Customization" or existing section.
- Present `feat/ui-config` for merge.
