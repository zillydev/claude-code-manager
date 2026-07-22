# UI customization via stored configuration — design

## Goal

Let the user customize claude-code-manager's appearance and remembered UI state through
a single on-disk config file, edited via an in-app settings panel with live
preview. Covers: **preset themes** (incl. light mode), **typography** (font size,
density, UI + mono font choices), and **behavior defaults** (compact, details,
sidebar width/collapsed, default tab).

Explicitly **not** in scope: hand-editing individual palette colors (presets
only), bundling fonts (uses fonts already installed), and migrating existing
localStorage UI prefs (they reset to config defaults once).

## Config file

### Location (configurable)

Resolved at startup, first match wins:

1. `$CLAUDE_CODE_MANAGER_CONFIG_DIR`
2. `$XDG_CONFIG_HOME/claude-code-manager`
3. `~/.config/claude-code-manager` (home from `dirs::home_dir()`)

The file is `config.json` inside that directory. (Env var, not a CLI flag,
because the bundled/launchd app has no argv; the default needs nothing set. Note:
not `dirs::config_dir()` — on macOS that is `~/Library/Application Support`, but
the user wants `~/.config`.)

### Schema

```json
{
  "version": 1,
  "theme": "github-dark",
  "typography": {
    "fontSize": 14,
    "density": "comfortable",
    "uiFont": "system",
    "monoFont": "system"
  },
  "ui": {
    "compact": false,
    "detailsOpen": true,
    "sidebarWidth": 320,
    "sidebarCollapsed": false,
    "defaultTab": "manager"
  }
}
```

- `theme`: `"github-dark" | "light"`.
- `typography.fontSize`: px, clamped `12..=18`.
- `typography.density`: `"comfortable" | "compact"`.
- `typography.uiFont` / `monoFont`: a known preset key (e.g. `"system"`, `"menlo"`,
  `"jetbrains-mono"`) **or** an arbitrary font-family string ("Custom…").
- `ui.defaultTab`: `"manager" | "terminal"`.

### Load / save semantics

- **Load:** missing or unparseable file → full defaults, and the defaults are
  written back (creating the directory). Partial/unknown fields → merge over
  defaults (serde `#[serde(default)]` per field + per nested struct), so older or
  hand-truncated files still load. Out-of-range values are clamped on load.
- **Save:** atomic — write to `config.json.tmp` then rename over `config.json`.

## Themes

Two palettes defined in **frontend** code as `Record<string,string>` of the 14
`--color-*` variables (`THEMES["github-dark"]` = the current `@theme` values;
`THEMES["light"]` = a light palette). Applied live by setting every var on
`document.documentElement.style` — overriding the `@theme` defaults with no
reload. Each theme **must** define all 14 keys (enforced by a test).

## Typography

- **fontSize** → set `document.documentElement.style.fontSize = "<n>px"`. The UI
  uses rem-based Tailwind `text-*` utilities, so this scales the whole app. (Inline
  style beats the `html,body { font-size:14px }` rule in `index.css`; that rule
  becomes the fallback.)
- **density** → `document.documentElement.dataset.density = "comfortable"|"compact"`.
  A curated `[data-density="compact"]` block in `index.css` tightens the
  highest-impact rhythms (transcript turn `space-y`, turn/row padding, header
  padding, prose `line-height`). Honestly partial — a curated, expandable set, not
  every element. Named "Density/Spacing" in the panel to avoid colliding with the
  transcript **Compact** toggle (which hides turns — unrelated).
- **fonts** → two CSS vars: `--font-ui` (used by `html,body`) and `--font-mono`
  (used by `.prose code/pre` and passed to the terminal). A `FONTS` map in code
  resolves a preset key to a font-family stack; a custom string is used verbatim.
  Changing `monoFont` also updates `term.options.fontFamily` and re-fits every live
  xterm instance.

  Initial presets — UI: System, Inter, Helvetica Neue. Mono: System, Menlo,
  Monaco, JetBrains Mono, Fira Code. Rendering depends on the font being installed
  (no bundling).

## Behavior defaults (unified)

The config is the **single source of truth** for these; the existing live controls
read/write it (no separate localStorage):

| Setting | Today | After |
|---|---|---|
| `compact` | `usePersistentState("cm.compact")` | config; toggle writes config |
| `detailsOpen` | `usePersistentState("cm.detailsOpen")` | config; toggle writes config |
| `sidebarWidth` | ephemeral `useState` (resets on reload) | config; drag writes (debounced ~300ms) |
| `sidebarCollapsed` | ephemeral `useState` | config; ⌘B / button writes config |
| `defaultTab` | n/a (`activeTab` is `useState`) | config; **launch-only** default |

- `compact`/`detailsOpen`/`sidebarWidth`/`sidebarCollapsed` are **set-and-stick**:
  changing the live control writes the config (and broadcasts). This is an upgrade
  for sidebar width/collapsed, which currently don't persist at all.
- `defaultTab` is **launch-only**: `activeTab` initializes from it, but switching
  tabs mid-session (⌘E / the toggle) does **not** rewrite the config — that's
  navigation, not a preference. Only the settings panel changes it.
- `cm:lastViewed` stays in localStorage — it is view history, not a preference.
- The old `cm.compact` / `cm.detailsOpen` localStorage keys are abandoned (no
  migration; first run after this feature uses config defaults).

## Backend (Tauri, `crates/webui/src/config.rs`)

- `pub struct Config { ... }` with serde defaults, plus `Typography` / `Ui` nested
  structs. `Default` impl = the schema defaults above.
- `fn config_path() -> PathBuf` — the location resolution above.
- `fn load() -> Config` — read + parse + merge/clamp; write defaults if absent.
- `fn save(cfg: &Config) -> Result<()>` — atomic temp-write + rename.
- Commands: `get_config() -> Config`, `set_config(config: Config) -> Result<(), String>`.
- `set_config` saves then **broadcasts** a global `config-changed` event with the
  new config (`app.emit`), so every open window re-applies live — mirroring the
  activity poller's broadcast. Registered in `lib.rs invoke_handler`.

## Frontend

### `ConfigProvider` + `useConfig` (React context)

A provider at the app root owns the config and exposes it via context, so any
component reads/writes without prop-drilling.

- On mount: `invoke("get_config")` → store in state → `applyConfig(cfg)`.
- `applyConfig(cfg)`: set the theme's 14 vars, root `font-size`, `data-density`,
  `--font-ui` / `--font-mono` on `document.documentElement`; notify terminals of
  the mono font.
- `setConfig(partial)`: merge into current → `applyConfig` immediately (optimistic
  live preview) → `invoke("set_config", { config })`.
- Listen for `config-changed` (global) → update state + `applyConfig` (cross-window
  sync; ignore if it equals our last write to avoid a loop).

### Consuming components (the unification refactor)

- `Transcript` no longer owns `compact`/`statsOpen` via `usePersistentState`; it
  reads them from `useConfig()` and toggles via `setConfig`. (Lifts that state out
  of the component — the dependency the header controls already need.)
- `App` reads `sidebarWidth` / `sidebarCollapsed` / `defaultTab` from `useConfig()`;
  the resize drag and ⌘B/collapse button write via `setConfig` (drag debounced).
  `activeTab` initializes from `defaultTab`.

### Settings panel

A `Settings` modal opened from a new `⚙` button in the sidebar header (beside
refresh `↻` / collapse `«`), built on the existing modal/overlay pattern
(Esc/backdrop close, like `LineageTree`):

- **Theme**: GitHub Dark / Light (swatch buttons).
- **Typography**: font-size (number/stepper 12–18), density (Comfortable/Compact),
  UI font + Mono font (`<select>` of presets + a "Custom…" text field).
- **Behavior**: default tab (Manager/Terminal); compact / details / start-collapsed
  toggles (mirrors of the live controls).

All changes apply live via `setConfig`.

## Testing

- **Rust:** `config_path` precedence (env / XDG / home, via temp env); load-missing
  → defaults + file written; parse-partial → merged defaults; out-of-range clamp;
  save→load roundtrip; atomic write leaves no `.tmp` on success.
- **Frontend:** `applyConfig` sets the expected `documentElement` style/dataset
  (jsdom); every theme defines all 14 `--color-*` keys; font/preset resolution;
  config merge reducer.

## Out of scope / non-goals

- Per-color custom palettes (presets only).
- Font bundling (uses installed fonts; custom strings may not render).
- Migrating existing localStorage prefs (one-time reset to config defaults).
- Comprehensive density (curated spacing overrides only).
- Setting the env-var location for the bundled/launchd app (it uses the default;
  env override is for terminal/`make webui` launches).
