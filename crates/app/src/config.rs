//! User configuration: persisted UI preferences (theme, typography, behavior).
//!
//! Stored as JSON at `<dir>/config.json`, where `<dir>` resolves (first match):
//! `$CLAUDE_CODE_MANAGER_CONFIG_DIR` → `$XDG_CONFIG_HOME/claude-code-manager` →
//! `~/.config/claude-code-manager`. The frontend tolerates unknown enum strings, so
//! we only clamp numeric ranges here; missing fields fall back to defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub typography: Typography,
    #[serde(default)]
    pub ui: Ui,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub integrations: Integrations,
}

/// How much of a transcript a window shows. Only the id is stored; the modes live in the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    /// The mode a new window starts in. Unknown ids are kept; the frontend maps them to "default".
    #[serde(default = "default_mode")]
    pub default_mode: String,
}

/// Integrations with other tools. Precise status is owned by the backend (see precise_status.rs); the rest are
/// opt-in side effects.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    /// Publish live session state to `<config-dir>/session-status.json` for
    /// third-party tools (status bars, notifiers).
    #[serde(default)]
    pub publish_status: bool,
    #[serde(default)]
    pub precise_status: PreciseStatus,
}

/// Whether the bundled status plugin should be in Claude Code, as far as anyone has chosen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreciseStatus {
    /// Nobody has chosen; the installed app enables it at launch.
    #[default]
    Unset,
    /// Enabled by the app on its own; the sidebar announces it once.
    Auto,
    On,
    Off,
}

/// `incoming` from a window, with the fields only the backend may change taken from disk.
pub fn with_backend_owned(mut incoming: Config, on_disk: &Config) -> Config {
    incoming.integrations.precise_status = on_disk.integrations.precise_status;
    incoming
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Typography {
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default = "default_density")]
    pub density: String,
    #[serde(default = "default_font")]
    pub ui_font: String,
    #[serde(default = "default_font")]
    pub mono_font: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ui {
    /// Read only to migrate into `appearance.default_mode`; never written back.
    #[serde(default, skip_serializing)]
    pub compact: bool,
    #[serde(default = "default_true")]
    pub details_open: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u32,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default = "default_tab")]
    pub default_tab: String,
    /// Launch-only project filter: "" (all) | `name:<name>` | `dir:<cwd>`,
    /// the same encoding the sidebar's filter uses.
    #[serde(default)]
    pub default_project: String,
}

fn default_version() -> u32 {
    1
}
fn default_theme() -> String {
    "github-dark".to_string()
}
fn default_font_size() -> u32 {
    14
}
fn default_density() -> String {
    "comfortable".to_string()
}
fn default_font() -> String {
    "system".to_string()
}
fn default_true() -> bool {
    true
}
fn default_sidebar_width() -> u32 {
    320
}
fn default_tab() -> String {
    "manager".to_string()
}
fn default_mode() -> String {
    "default".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: default_version(),
            theme: default_theme(),
            typography: Typography::default(),
            ui: Ui::default(),
            appearance: Appearance::default(),
            integrations: Integrations::default(),
        }
    }
}
impl Default for Typography {
    fn default() -> Self {
        Typography {
            font_size: default_font_size(),
            density: default_density(),
            ui_font: default_font(),
            mono_font: default_font(),
        }
    }
}
impl Default for Appearance {
    fn default() -> Self {
        Appearance { default_mode: default_mode() }
    }
}
impl Default for Ui {
    fn default() -> Self {
        Ui {
            compact: false,
            details_open: true,
            sidebar_width: default_sidebar_width(),
            sidebar_collapsed: false,
            default_tab: default_tab(),
            default_project: String::new(),
        }
    }
}

impl Config {
    /// Keep numeric fields in sane ranges (string enums are validated by the UI).
    fn clamp(&mut self) {
        self.typography.font_size = self.typography.font_size.clamp(12, 18);
        self.ui.sidebar_width = self.ui.sidebar_width.clamp(200, 600);
    }

    /// Conversation is Compact's closest successor, so a user who left Compact on lands there.
    fn migrate(&mut self) {
        if self.ui.compact && self.appearance.default_mode == "default" {
            self.appearance.default_mode = "conversation".to_string();
        }
    }
}

/// Resolve the config directory from the given env values (pure, for testing).
fn resolve_dir(cm: Option<String>, xdg: Option<String>, home: Option<String>) -> PathBuf {
    if let Some(d) = cm.filter(|s| !s.is_empty()) {
        return PathBuf::from(d);
    }
    if let Some(x) = xdg.filter(|s| !s.is_empty()) {
        return PathBuf::from(x).join("claude-code-manager");
    }
    PathBuf::from(home.unwrap_or_else(|| ".".to_string()))
        .join(".config")
        .join("claude-code-manager")
}

pub(crate) fn config_dir() -> PathBuf {
    resolve_dir(
        std::env::var("CLAUDE_CODE_MANAGER_CONFIG_DIR").ok(),
        std::env::var("XDG_CONFIG_HOME").ok(),
        std::env::var("HOME").ok(),
    )
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Read + parse + clamp a config file. Returns defaults for a missing or
/// malformed file (without overwriting it — that's `load`'s job, and only for a
/// genuinely absent file). Partial JSON merges over defaults (serde field defaults).
fn load_from(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Config>(&s) {
            Ok(mut cfg) => {
                cfg.migrate();
                cfg.clamp();
                cfg
            }
            Err(_) => Config::default(),
        },
        Err(_) => Config::default(),
    }
}

fn save_to(path: &Path, cfg: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Load the user config. A genuinely absent file is created with defaults; a
/// malformed file is left untouched (defaults returned) so the user can fix it.
pub fn load() -> Config {
    let path = config_path();
    if !path.exists() {
        let cfg = Config::default();
        let _ = save_to(&path, &cfg);
        return cfg;
    }
    load_from(&path)
}

pub fn save(cfg: &Config) -> std::io::Result<()> {
    save_to(&config_path(), cfg)
}

#[cfg(test)]
mod tests {

    #[test]
    fn default_project_is_empty_and_round_trips() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.ui.default_project, "", "no default project out of the box");

        let c: Config =
            serde_json::from_str(r#"{"ui":{"defaultProject":"name:mono"}}"#).unwrap();
        assert_eq!(c.ui.default_project, "name:mono");
        assert!(serde_json::to_string(&c).unwrap().contains("\"defaultProject\":\"name:mono\""));
    }


    #[test]
    fn integrations_default_off_and_round_trip() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.integrations.publish_status, "publishing is opt-in");

        let c: Config = serde_json::from_str(r#"{"integrations":{"publishStatus":true}}"#).unwrap();
        assert!(c.integrations.publish_status);

        let back = serde_json::to_string(&c).unwrap();
        assert!(back.contains("\"publishStatus\":true"));
    }

    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cm-config-test-{name}")).join("config.json")
    }

    #[test]
    fn resolve_dir_precedence() {
        // CLAUDE_CODE_MANAGER_CONFIG_DIR wins.
        assert_eq!(
            resolve_dir(Some("/a".into()), Some("/b".into()), Some("/h".into())),
            PathBuf::from("/a")
        );
        // then XDG_CONFIG_HOME/claude-code-manager.
        assert_eq!(
            resolve_dir(None, Some("/b".into()), Some("/h".into())),
            PathBuf::from("/b/claude-code-manager")
        );
        // then ~/.config/claude-code-manager.
        assert_eq!(
            resolve_dir(None, None, Some("/h".into())),
            PathBuf::from("/h/.config/claude-code-manager")
        );
        // empty strings are ignored.
        assert_eq!(
            resolve_dir(Some(String::new()), Some(String::new()), Some("/h".into())),
            PathBuf::from("/h/.config/claude-code-manager")
        );
    }

    #[test]
    fn roundtrip_save_load() {
        let path = tmp("roundtrip");
        let mut cfg = Config { theme: "light".into(), ..Default::default() };
        cfg.typography.mono_font = "JetBrains Mono".into();
        cfg.ui.sidebar_width = 420;
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path), cfg);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_defaults() {
        let path = tmp("missing-xyz");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(load_from(&path), Config::default());
    }

    #[test]
    fn malformed_file_is_defaults() {
        let path = tmp("malformed");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path), Config::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn partial_json_merges_defaults_and_clamps() {
        let path = tmp("partial");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Only theme + an out-of-range font size; everything else should default.
        std::fs::write(&path, r#"{ "theme": "light", "typography": { "fontSize": 99 } }"#)
            .unwrap();
        let cfg = load_from(&path);
        assert_eq!(cfg.theme, "light");
        assert_eq!(cfg.typography.font_size, 18); // clamped from 99
        assert_eq!(cfg.typography.density, "comfortable"); // defaulted
        assert!(cfg.ui.details_open); // defaulted
        assert_eq!(cfg.ui.sidebar_width, 320); // defaulted
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn atomic_save_leaves_no_tmp() {
        let path = tmp("atomic");
        save_to(&path, &Config::default()).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn precise_status_defaults_to_unset_and_round_trips() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.integrations.precise_status, PreciseStatus::Unset);
        let c: Config = serde_json::from_str(r#"{"integrations":{"preciseStatus":"off"}}"#).unwrap();
        assert_eq!(c.integrations.precise_status, PreciseStatus::Off);
        let out = serde_json::to_value(&c).unwrap();
        assert_eq!(out["integrations"]["preciseStatus"], "off");
    }

    #[test]
    fn default_mode_defaults_and_round_trips() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.appearance.default_mode, "default");
        let c: Config =
            serde_json::from_str(r#"{"appearance":{"defaultMode":"truncated"}}"#).unwrap();
        assert_eq!(c.appearance.default_mode, "truncated");
        let out = serde_json::to_value(&c).unwrap();
        assert_eq!(out["appearance"]["defaultMode"], "truncated");
    }

    #[test]
    fn set_config_path_keeps_the_default_mode() {
        // What set_config does with a window's JSON: parse, keep backend-owned fields, save; then reload.
        let path = tmp("appearance-roundtrip");
        let incoming: Config = serde_json::from_value(serde_json::json!({
            "theme": "light",
            "appearance": { "defaultMode": "everything" },
        }))
        .unwrap();
        save_to(&path, &with_backend_owned(incoming, &Config::default())).unwrap();
        assert_eq!(load_from(&path).appearance.default_mode, "everything");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unknown_default_mode_is_kept_for_the_frontend_to_map() {
        let c: Config = serde_json::from_str(r#"{"appearance":{"defaultMode":"gone"}}"#).unwrap();
        assert_eq!(c.appearance.default_mode, "gone");
    }

    fn load_json(name: &str, json: &str) -> Config {
        let path = tmp(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, json).unwrap();
        let cfg = load_from(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        cfg
    }

    #[test]
    fn compact_left_on_loads_as_conversation() {
        let cfg = load_json("compact-on", r#"{"ui":{"compact":true}}"#);
        assert_eq!(cfg.appearance.default_mode, "conversation");
        let cfg = load_json(
            "compact-on-default",
            r#"{"ui":{"compact":true},"appearance":{"defaultMode":"default"}}"#,
        );
        assert_eq!(cfg.appearance.default_mode, "conversation");
    }

    #[test]
    fn compact_does_not_override_a_chosen_mode() {
        let cfg = load_json(
            "compact-on-inputs",
            r#"{"ui":{"compact":true},"appearance":{"defaultMode":"inputs"}}"#,
        );
        assert_eq!(cfg.appearance.default_mode, "inputs");
        let cfg = load_json("compact-off", r#"{"ui":{"compact":false}}"#);
        assert_eq!(cfg.appearance.default_mode, "default");
    }

    #[test]
    fn a_saved_config_never_contains_compact() {
        let path = tmp("no-compact");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"ui":{"compact":true}}"#).unwrap();
        save_to(&path, &load_from(&path)).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("compact"), "{saved}");
        assert_eq!(load_from(&path).appearance.default_mode, "conversation");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_window_saving_config_keeps_the_backend_owned_choice() {
        let mut on_disk = Config::default();
        on_disk.integrations.precise_status = PreciseStatus::Off;
        let incoming = Config { theme: "light".into(), ..Config::default() };
        let saved = with_backend_owned(incoming, &on_disk);
        assert_eq!(saved.integrations.precise_status, PreciseStatus::Off);
        assert_eq!(saved.theme, "light");
    }
}
