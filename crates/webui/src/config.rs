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
    #[serde(default)]
    pub compact: bool,
    #[serde(default = "default_true")]
    pub details_open: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u32,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default = "default_tab")]
    pub default_tab: String,
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

impl Default for Config {
    fn default() -> Self {
        Config {
            version: default_version(),
            theme: default_theme(),
            typography: Typography::default(),
            ui: Ui::default(),
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
impl Default for Ui {
    fn default() -> Self {
        Ui {
            compact: false,
            details_open: true,
            sidebar_width: default_sidebar_width(),
            sidebar_collapsed: false,
            default_tab: default_tab(),
        }
    }
}

impl Config {
    /// Keep numeric fields in sane ranges (string enums are validated by the UI).
    fn clamp(&mut self) {
        self.typography.font_size = self.typography.font_size.clamp(12, 18);
        self.ui.sidebar_width = self.ui.sidebar_width.clamp(200, 600);
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
        let mut cfg = Config::default();
        cfg.theme = "light".into();
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
}
