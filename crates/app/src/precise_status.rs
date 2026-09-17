//! Precise session status: the bundled plugin's lifecycle, decided at launch and toggled from Settings.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::PreciseStatus;

/// What launch can see before deciding.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub choice: PreciseStatus,
    /// Our hook entries still in `~/.claude/settings.json`, from before the plugin.
    pub legacy_hooks: bool,
    pub claude_found: bool,
    pub dev_tools: bool,
    /// `None` when `claude plugin list` couldn't answer.
    pub plugin_enabled: Option<bool>,
    pub marketplace_dir_exists: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Install the plugin, then remove the legacy entries; the choice becomes `on`.
    Migrate,
    /// Install the plugin; the choice becomes `auto`, and windows are told.
    AutoInstall,
    /// Already enabled (by a dev build or by hand); record `on`.
    MarkOn,
    /// Removed or disabled outside the app; record `off` and leave it.
    MarkOff,
    /// Enabled, but its marketplace directory is gone; re-add it from this bundle.
    Repoint,
}

pub fn launch_action(f: &Facts) -> Action {
    use PreciseStatus::{Auto, Off, On, Unset};
    if f.legacy_hooks {
        return if f.claude_found {
            Action::Migrate
        } else {
            Action::Nothing
        };
    }
    match (f.choice, f.plugin_enabled) {
        (Off, _) | (_, None) => Action::Nothing,
        (Unset, Some(true)) => Action::MarkOn,
        (Unset, Some(false)) if f.claude_found && f.dev_tools => Action::AutoInstall,
        (Unset, Some(false)) => Action::Nothing,
        (Auto | On, Some(false)) => Action::MarkOff,
        (Auto | On, Some(true)) if f.marketplace_dir_exists == Some(false) => Action::Repoint,
        (Auto | On, Some(true)) => Action::Nothing,
    }
}

fn bundled_plugin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resource_dir()
        .map(|d| d.join("plugin"))
        .map_err(|e| e.to_string())
}

fn set_choice(choice: PreciseStatus) -> Result<(), String> {
    let mut cfg = crate::config::load();
    cfg.integrations.precise_status = choice;
    crate::config::save(&cfg).map_err(|e| e.to_string())
}

fn gather() -> Facts {
    let claude_found = crate::plugin::claude_found();
    let plugin_enabled = if claude_found {
        crate::plugin::enabled().ok()
    } else {
        None
    };
    let marketplace_dir_exists = if plugin_enabled == Some(true) {
        crate::plugin::marketplace_path()
            .ok()
            .map(|p| p.is_some_and(|p| p.is_dir()))
    } else {
        None
    };
    Facts {
        choice: crate::config::load().integrations.precise_status,
        legacy_hooks: crate::hooks::legacy_installed(),
        claude_found,
        dev_tools: crate::plugin::developer_tools_present(),
        plugin_enabled,
        marketplace_dir_exists,
    }
}

/// Applies the launch decision once, in the background. Installed builds only: a dev build would re-point the
/// marketplace at its build folder.
pub fn start(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(dir) = bundled_plugin_dir(&app) else {
            return;
        };
        // Failures change nothing and stay silent; an unset choice tries again next launch.
        let _ = match launch_action(&gather()) {
            Action::Nothing => Ok(()),
            Action::Migrate => crate::plugin::install(&dir)
                .and_then(|_| crate::hooks::remove_legacy())
                .and_then(|_| set_choice(PreciseStatus::On)),
            Action::AutoInstall => crate::plugin::install(&dir)
                .and_then(|_| set_choice(PreciseStatus::Auto))
                .map(|_| {
                    let _ = app.emit("precise-status-auto", ());
                }),
            Action::MarkOn => set_choice(PreciseStatus::On),
            Action::MarkOff => set_choice(PreciseStatus::Off),
            // Adding the marketplace again points it here; installing an installed plugin is a no-op.
            Action::Repoint => crate::plugin::install(&dir),
        };
    });
}

/// The remembered choice, for the sidebar's one-time notice.
#[tauri::command]
pub fn precise_status_choice() -> PreciseStatus {
    crate::config::load().integrations.precise_status
}

/// Whether the plugin is installed and enabled in Claude Code, for the Settings toggle.
#[tauri::command]
pub async fn precise_status_enabled() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(crate::plugin::enabled)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn precise_status_enable(app: AppHandle) -> Result<(), String> {
    if !crate::plugin::developer_tools_present() {
        return Err(
            "Needs Apple's command line tools: run `xcode-select --install`, then try again."
                .into(),
        );
    }
    let dir = bundled_plugin_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::plugin::install(&dir)?;
        if crate::hooks::legacy_installed() {
            crate::hooks::remove_legacy()?;
        }
        set_choice(PreciseStatus::On)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn precise_status_disable() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::plugin::uninstall()?;
        if crate::hooks::legacy_installed() {
            crate::hooks::remove_legacy()?;
        }
        set_choice(PreciseStatus::Off)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(choice: PreciseStatus) -> Facts {
        Facts {
            choice,
            legacy_hooks: false,
            claude_found: true,
            dev_tools: true,
            plugin_enabled: Some(false),
            marketplace_dir_exists: None,
        }
    }

    #[test]
    fn legacy_hooks_migrate_whatever_the_choice() {
        for choice in [PreciseStatus::Unset, PreciseStatus::On, PreciseStatus::Off] {
            assert_eq!(
                launch_action(&Facts {
                    legacy_hooks: true,
                    ..facts(choice)
                }),
                Action::Migrate
            );
        }
        let no_claude = Facts {
            legacy_hooks: true,
            claude_found: false,
            ..facts(PreciseStatus::Unset)
        };
        assert_eq!(launch_action(&no_claude), Action::Nothing);
    }

    #[test]
    fn unset_installs_automatically_when_it_can() {
        assert_eq!(
            launch_action(&facts(PreciseStatus::Unset)),
            Action::AutoInstall
        );
        let no_tools = Facts {
            dev_tools: false,
            ..facts(PreciseStatus::Unset)
        };
        assert_eq!(launch_action(&no_tools), Action::Nothing);
        let no_claude = Facts {
            claude_found: false,
            plugin_enabled: None,
            ..facts(PreciseStatus::Unset)
        };
        assert_eq!(launch_action(&no_claude), Action::Nothing);
        let unknown = Facts {
            plugin_enabled: None,
            ..facts(PreciseStatus::Unset)
        };
        assert_eq!(launch_action(&unknown), Action::Nothing);
    }

    #[test]
    fn unset_but_already_enabled_is_recorded_as_on() {
        let enabled = Facts {
            plugin_enabled: Some(true),
            ..facts(PreciseStatus::Unset)
        };
        assert_eq!(launch_action(&enabled), Action::MarkOn);
    }

    #[test]
    fn a_plugin_removed_outside_the_app_stays_off() {
        assert_eq!(launch_action(&facts(PreciseStatus::Auto)), Action::MarkOff);
        assert_eq!(launch_action(&facts(PreciseStatus::On)), Action::MarkOff);
        let unknown = Facts {
            plugin_enabled: None,
            ..facts(PreciseStatus::On)
        };
        assert_eq!(launch_action(&unknown), Action::Nothing);
    }

    #[test]
    fn an_enabled_plugin_whose_marketplace_moved_is_repointed() {
        let enabled = |dir| Facts {
            plugin_enabled: Some(true),
            marketplace_dir_exists: dir,
            ..facts(PreciseStatus::Auto)
        };
        assert_eq!(launch_action(&enabled(Some(false))), Action::Repoint);
        assert_eq!(launch_action(&enabled(Some(true))), Action::Nothing);
        assert_eq!(launch_action(&enabled(None)), Action::Nothing);
    }

    #[test]
    fn off_does_nothing() {
        let enabled = Facts {
            plugin_enabled: Some(true),
            ..facts(PreciseStatus::Off)
        };
        assert_eq!(launch_action(&enabled), Action::Nothing);
        assert_eq!(launch_action(&facts(PreciseStatus::Off)), Action::Nothing);
    }
}
