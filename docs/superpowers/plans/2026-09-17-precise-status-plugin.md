# Precise Status Plugin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver precise session status as a Claude Code plugin bundled with the app, installed automatically (with a notice) for anyone who hasn't chosen, remembered when turned off, and migrated from today's `settings.json` hooks.

**Architecture:** The relay and its `hooks.json` live in `crates/app/plugin/`, a directory marketplace copied into the bundle's resources. A new `plugin.rs` drives `claude plugin …` through the CLI; a new `precise_status.rs` holds the pure launch decision, the launch thread and the Tauri commands; the choice (`unset|auto|on|off`) lives in `config.json` but is owned by the backend. The frontend's Settings toggle calls the new commands and a sidebar note announces an automatic install.

**Tech Stack:** Rust (Tauri 2, serde, std::process), React 19 + TypeScript, Vitest, Claude Code CLI 2.1.274.

**Spec:** `docs/superpowers/specs/2026-09-17-precise-status-plugin-design.md`

## Global Constraints

- Marketplace name `claude-code-manager`; plugin `ccm-status`; id `ccm-status@claude-code-manager`; scope `user`.
- Hook command: `/usr/bin/python3 "${CLAUDE_PLUGIN_ROOT}/relay.py"`; events and matchers exactly `hooks::HOOK_EVENTS`.
- Relay behaviour unchanged: same fields, same `hook-events.ndjson` path resolution, no prompt or message content.
- CLI via the real `claude` (never `claude-tracked`), each command limited to 30 s; errors carry the CLI's message.
- Choice values `unset` (default) / `auto` / `on` / `off`, JSON key `integrations.preciseStatus`; `set_config` keeps the on-disk value.
- Automatic launch actions only in release builds (`!cfg!(debug_assertions)`); developer tools = `xcode-select -p` exits 0.
- Notice copy: "Precise session status is on — ccm added its status plugin to Claude Code."; dismissal key `cm:preciseStatusNoticeDismissed`; event `precise-status-auto`.
- Gate: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test`, `npm --prefix crates/app/frontend run test`, `npm --prefix crates/app/frontend run build`, `make build`, `claude plugin validate crates/app/plugin`.
- Comments 1–2 lines, current state only. Commits authored `zillydev <zaidsharieff09@gmail.com>`, ending `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`. Never launch the app; never push; never touch the real `~/.claude` from tests.

---

### Task 1: The plugin files, bundled

**Files:**
- Create: `crates/app/plugin/.claude-plugin/marketplace.json`, `crates/app/plugin/plugins/ccm-status/.claude-plugin/plugin.json`, `crates/app/plugin/plugins/ccm-status/hooks/hooks.json`, `crates/app/plugin/plugins/ccm-status/relay.py`
- Modify: `crates/app/tauri.conf.json` (`bundle.resources`)
- Test: `crates/app/src/hooks.rs` (tests module)

**Interfaces:**
- Produces: the bundled directory `<resource_dir>/plugin` (a marketplace), used by Task 2's `plugin::install`.

- [ ] **Step 1: Stub `hooks.json` with no hooks and write the failing test**

`crates/app/plugin/plugins/ccm-status/hooks/hooks.json`:

```json
{ "hooks": {} }
```

In `hooks.rs` tests:

```rust
    #[test]
    fn the_bundled_plugin_registers_exactly_our_hook_events() {
        let bundled: Value = serde_json::from_str(include_str!("../plugin/plugins/ccm-status/hooks/hooks.json")).unwrap();
        let hooks = bundled["hooks"].as_object().expect("hooks object");
        let mut got: Vec<(String, Option<String>, String)> = hooks
            .iter()
            .flat_map(|(event, groups)| {
                groups.as_array().unwrap().iter().map(move |g| {
                    let cmd = g["hooks"][0]["command"].as_str().unwrap().to_string();
                    (event.clone(), g["matcher"].as_str().map(str::to_string), cmd)
                })
            })
            .collect();
        got.sort();
        let mut want: Vec<(String, Option<String>, String)> = HOOK_EVENTS
            .iter()
            .map(|(e, m)| (e.to_string(), m.map(str::to_string), PLUGIN_RELAY_COMMAND.to_string()))
            .collect();
        want.sort();
        assert_eq!(got, want);
    }
```

and, next to `HOOK_EVENTS`:

```rust
/// How the bundled plugin runs its relay: python by absolute path, so the bundle needn't keep an execute bit.
pub const PLUGIN_RELAY_COMMAND: &str = r#"/usr/bin/python3 "${CLAUDE_PLUGIN_ROOT}/relay.py""#;
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p claude-code-manager-app the_bundled_plugin`
Expected: FAIL — `got` is empty.

- [ ] **Step 3: Write the plugin**

`hooks.json` — one group per event, generated once and checked in:

```json
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "StopFailure": [{ "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "PreToolUse": [{ "matcher": "AskUserQuestion|ExitPlanMode", "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "PostToolUse": [{ "matcher": "AskUserQuestion|ExitPlanMode", "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }],
    "Notification": [{ "hooks": [{ "type": "command", "command": "/usr/bin/python3 \"${CLAUDE_PLUGIN_ROOT}/relay.py\"" }] }]
  }
}
```

`relay.py`: the body of `hooks::RELAY_SCRIPT` verbatim, with its second line reading `# claude-code-manager hook relay (bundled with the app as the ccm-status plugin).`

`plugin.json`:

```json
{
  "name": "ccm-status",
  "version": "1.0.0",
  "description": "Reports each session's state (running, needs you, your turn) to Claude Code Manager.",
  "author": { "name": "zillydev" },
  "homepage": "https://github.com/zillydev/claude-code-manager"
}
```

`marketplace.json`:

```json
{
  "name": "claude-code-manager",
  "owner": { "name": "zillydev" },
  "description": "The status plugin bundled with Claude Code Manager.",
  "plugins": [
    { "name": "ccm-status", "source": "./plugins/ccm-status", "description": "Reports each session's state to Claude Code Manager." }
  ]
}
```

`tauri.conf.json`, inside `"bundle"` after `"icon"`:

```json
    "resources": ["plugin/"]
```

- [ ] **Step 4: Verify tests, validation, and that the build copies the dot-folders**

Run: `cargo test -p claude-code-manager-app the_bundled_plugin` → PASS.
Run: `claude plugin validate crates/app/plugin && claude plugin validate crates/app/plugin/plugins/ccm-status` → passes (warnings allowed).
Run: `cargo build -p claude-code-manager-app && find target/debug/plugin -type f` → lists both `.claude-plugin/*.json`, `hooks/hooks.json` and `relay.py`. If a `.claude-plugin` file is missing, replace the list with an explicit map of each file (`"plugin/.claude-plugin/marketplace.json": "plugin/.claude-plugin/marketplace.json"`, …) and re-check.

- [ ] **Step 5: Commit**

```bash
git add crates/app/plugin crates/app/tauri.conf.json crates/app/src/hooks.rs
git commit -m "feat(app): bundle the hook relay as a Claude Code plugin"
```

---

### Task 2: Driving `claude plugin` from Rust

**Files:**
- Create: `crates/app/src/plugin.rs`
- Modify: `crates/app/src/terminal.rs` (extract `find_claude`), `crates/app/src/lib.rs` (`mod plugin;`)

**Interfaces:**
- Produces: `pub const PLUGIN_ID: &str`, `pub const MARKETPLACE: &str`, `pub fn install_steps(dir: &Path) -> Vec<Vec<String>>`, `pub fn uninstall_steps() -> Vec<Vec<String>>`, `pub fn enabled_in(list_json: &str) -> Result<bool, String>`, `pub fn marketplace_path_in(json: &str) -> Result<Option<PathBuf>, String>`, `pub fn enabled() -> Result<bool, String>`, `pub fn marketplace_path() -> Result<Option<PathBuf>, String>`, `pub fn install(dir: &Path) -> Result<(), String>`, `pub fn uninstall() -> Result<(), String>`, `pub fn claude_found() -> bool`, `pub fn developer_tools_present() -> bool`; in terminal.rs `pub(crate) fn find_claude() -> Result<Entrypoint, String>`.

- [ ] **Step 1: Extract `find_claude` in terminal.rs**

Move everything in `resolve_entrypoint` after the `claude-tracked` wrapper block into:

```rust
/// The real `claude` executable and the PATH it needs, never the `claude-tracked` wrapper.
pub(crate) fn find_claude() -> Result<Entrypoint, String> {
    // (the login PATH / interactive PATH / fallback-dir search and the error, moved verbatim)
}
```

and end `resolve_entrypoint` with `find_claude()`. Run `cargo test -p claude-code-manager-app terminal` → all pass (pure move).

- [ ] **Step 2: Write `plugin.rs` with stubs and failing tests**

```rust
//! The bundled `ccm-status` plugin, installed and inspected through Claude Code's own `claude plugin` CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MARKETPLACE: &str = "claude-code-manager";
pub const PLUGIN_ID: &str = "ccm-status@claude-code-manager";
const CLI_TIMEOUT: Duration = Duration::from_secs(30);

pub fn install_steps(dir: &Path) -> Vec<Vec<String>> {
    let _ = dir;
    Vec::new()
}

pub fn uninstall_steps() -> Vec<Vec<String>> {
    Vec::new()
}

/// Whether `claude plugin list --json` shows our plugin installed and enabled.
pub fn enabled_in(list_json: &str) -> Result<bool, String> {
    let _ = list_json;
    Ok(false)
}

/// Our marketplace's recorded directory in `claude plugin marketplace list --json`.
pub fn marketplace_path_in(json: &str) -> Result<Option<PathBuf>, String> {
    let _ = json;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn install_adds_the_bundled_marketplace_then_the_plugin() {
        assert_eq!(
            install_steps(Path::new("/App.app/Contents/Resources/plugin")),
            vec![
                cli(&["plugin", "marketplace", "add", "/App.app/Contents/Resources/plugin"]),
                cli(&["plugin", "install", PLUGIN_ID, "--scope", "user", "--json"]),
            ]
        );
    }

    #[test]
    fn uninstall_removes_the_plugin_then_the_marketplace() {
        assert_eq!(
            uninstall_steps(),
            vec![cli(&["plugin", "uninstall", PLUGIN_ID, "--json"]), cli(&["plugin", "marketplace", "remove", MARKETPLACE])]
        );
    }

    #[test]
    fn reads_whether_our_plugin_is_enabled() {
        let on = r#"[{"id":"superpowers@claude-plugins-official","enabled":true},{"id":"ccm-status@claude-code-manager","enabled":true}]"#;
        let off = r#"[{"id":"ccm-status@claude-code-manager","enabled":false}]"#;
        assert_eq!(enabled_in(on), Ok(true));
        assert_eq!(enabled_in(off), Ok(false));
        assert_eq!(enabled_in("[]"), Ok(false));
        assert!(enabled_in("Not logged in").is_err());
    }

    #[test]
    fn reads_our_marketplace_path() {
        let json = r#"[{"name":"claude-plugins-official","source":"github"},{"name":"claude-code-manager","source":"directory","path":"/App.app/Contents/Resources/plugin"}]"#;
        assert_eq!(marketplace_path_in(json), Ok(Some(PathBuf::from("/App.app/Contents/Resources/plugin"))));
        assert_eq!(marketplace_path_in("[]"), Ok(None));
        assert!(marketplace_path_in("{").is_err());
    }
}
```

Add `mod plugin;` to lib.rs, keeping the module list alphabetical.

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p claude-code-manager-app plugin::`
Expected: FAIL on all four (empty steps, `Ok(false)`/`Ok(None)` where others are expected, no error for bad JSON).

- [ ] **Step 4: Implement the pure parts and the CLI runner**

```rust
fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

pub fn install_steps(dir: &Path) -> Vec<Vec<String>> {
    vec![
        args(&["plugin", "marketplace", "add", &dir.to_string_lossy()]),
        args(&["plugin", "install", PLUGIN_ID, "--scope", "user", "--json"]),
    ]
}

pub fn uninstall_steps() -> Vec<Vec<String>> {
    vec![args(&["plugin", "uninstall", PLUGIN_ID, "--json"]), args(&["plugin", "marketplace", "remove", MARKETPLACE])]
}

fn parse_list(json: &str) -> Result<Vec<serde_json::Value>, String> {
    serde_json::from_str(json).map_err(|_| format!("unexpected output from claude: {}", json.trim()))
}

pub fn enabled_in(list_json: &str) -> Result<bool, String> {
    Ok(parse_list(list_json)?
        .iter()
        .any(|p| p["id"] == PLUGIN_ID && p["enabled"] == true))
}

pub fn marketplace_path_in(json: &str) -> Result<Option<PathBuf>, String> {
    Ok(parse_list(json)?
        .iter()
        .find(|m| m["name"] == MARKETPLACE)
        .and_then(|m| m["path"].as_str())
        .map(PathBuf::from))
}

/// Runs `claude <args>`, returning stdout; a non-zero exit, a timeout or a missing `claude` is an error.
fn run(cli_args: &[String]) -> Result<String, String> {
    let claude = crate::terminal::find_claude()?;
    let mut cmd = Command::new(&claude.program);
    cmd.args(cli_args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(path) = &claude.path {
        cmd.env("PATH", path);
    }
    let mut child = cmd.spawn().map_err(|e| format!("couldn't run claude: {e}"))?;
    let started = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if started.elapsed() > CLI_TIMEOUT {
            let _ = child.kill();
            return Err(format!("`claude {}` timed out", cli_args.join(" ")));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let msg = String::from_utf8_lossy(&out.stderr);
        Err(format!("`claude {}` failed: {}", cli_args.join(" "), if msg.trim().is_empty() { stdout.trim() } else { msg.trim() }))
    }
}

pub fn claude_found() -> bool {
    crate::terminal::find_claude().is_ok()
}

/// Apple's developer tools, without which `/usr/bin/python3` opens an install dialog instead of running.
pub fn developer_tools_present() -> bool {
    Command::new("/usr/bin/xcode-select").arg("-p").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

pub fn enabled() -> Result<bool, String> {
    enabled_in(&run(&args(&["plugin", "list", "--json"]))?)
}

pub fn marketplace_path() -> Result<Option<PathBuf>, String> {
    marketplace_path_in(&run(&args(&["plugin", "marketplace", "list", "--json"]))?)
}

pub fn install(dir: &Path) -> Result<(), String> {
    for step in install_steps(dir) {
        run(&step)?;
    }
    Ok(())
}

/// Removes the plugin and its marketplace; either already being gone is fine.
pub fn uninstall() -> Result<(), String> {
    let [plugin, market] = <[Vec<String>; 2]>::try_from(uninstall_steps()).expect("two steps");
    if let Err(e) = run(&plugin) {
        if enabled()? {
            return Err(e);
        }
    }
    if marketplace_path()?.is_some() {
        run(&market)?;
    }
    Ok(())
}
```

(The pipes are read after exit; `claude plugin` output is a few KB, well under the pipe buffer.)

- [ ] **Step 5: Run tests and clippy**

Run: `cargo test -p claude-code-manager-app plugin::` → 4 passed. Clippy waits for Task 4, where these functions gain callers.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/plugin.rs crates/app/src/terminal.rs crates/app/src/lib.rs
git commit -m "feat(app): install and inspect the status plugin through the claude CLI"
```

---

### Task 3: The choice and the launch decision

**Files:**
- Modify: `crates/app/src/config.rs` (enum, field, `with_backend_owned`, tests), `crates/app/src/commands.rs` (`set_config`)
- Create: `crates/app/src/precise_status.rs` (pure part + tests), `mod precise_status;` in lib.rs

**Interfaces:**
- Produces: `config::PreciseStatus { Unset, Auto, On, Off }` (serde lowercase, `Default = Unset`), `Integrations::precise_status`, `config::with_backend_owned(incoming: Config, on_disk: &Config) -> Config`, `precise_status::Facts { choice, legacy_hooks, claude_found, dev_tools, plugin_enabled: Option<bool>, marketplace_dir_exists: Option<bool> }`, `precise_status::Action { Nothing, Migrate, AutoInstall, MarkOn, MarkOff, Repoint }`, `precise_status::launch_action(&Facts) -> Action`.

- [ ] **Step 1: Failing config tests**

In config.rs tests:

```rust
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
    fn a_window_saving_config_keeps_the_backend_owned_choice() {
        let mut on_disk = Config::default();
        on_disk.integrations.precise_status = PreciseStatus::Off;
        let mut incoming = Config::default();
        incoming.theme = "light".into();
        let saved = with_backend_owned(incoming, &on_disk);
        assert_eq!(saved.integrations.precise_status, PreciseStatus::Off);
        assert_eq!(saved.theme, "light");
    }
```

Add the enum (stub `with_backend_owned` returning `incoming`), field `#[serde(default)] pub precise_status: PreciseStatus` in `Integrations`, and update the `Integrations` doc comment to: `/// Integrations with other tools. Precise status is owned by the backend (see precise_status.rs); the rest are opt-in.`

```rust
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
```

Run: `cargo test -p claude-code-manager-app config::` → the keep-choice test FAILS (stub returns `Unset`).

- [ ] **Step 2: Implement and wire `set_config`**

```rust
/// `incoming` from a window, with the fields only the backend may change taken from disk.
pub fn with_backend_owned(mut incoming: Config, on_disk: &Config) -> Config {
    incoming.integrations.precise_status = on_disk.integrations.precise_status;
    incoming
}
```

In `commands::set_config`: `let config = crate::config::with_backend_owned(config, &crate::config::load());` before saving. Run config tests → pass.

- [ ] **Step 3: Failing `launch_action` tests**

`precise_status.rs`:

```rust
//! Precise session status: the bundled plugin's lifecycle, decided at launch and toggled from Settings.

use crate::config::PreciseStatus;

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
    /// Install the plugin, then remove the legacy entries; choice becomes `on`.
    Migrate,
    /// Install the plugin; choice becomes `auto`, and windows are told.
    AutoInstall,
    /// Already enabled (from a dev build or by hand); record `on`.
    MarkOn,
    /// Removed or disabled outside the app; record `off` and leave it.
    MarkOff,
    /// Enabled, but its marketplace directory is gone; re-add it from this bundle.
    Repoint,
}

pub fn launch_action(f: &Facts) -> Action {
    let _ = f;
    Action::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(choice: PreciseStatus) -> Facts {
        Facts { choice, legacy_hooks: false, claude_found: true, dev_tools: true, plugin_enabled: Some(false), marketplace_dir_exists: None }
    }

    #[test]
    fn legacy_hooks_migrate_whatever_the_choice() {
        for choice in [PreciseStatus::Unset, PreciseStatus::On, PreciseStatus::Off] {
            assert_eq!(launch_action(&Facts { legacy_hooks: true, ..facts(choice) }), Action::Migrate);
        }
        assert_eq!(launch_action(&Facts { legacy_hooks: true, claude_found: false, ..facts(PreciseStatus::Unset) }), Action::Nothing);
    }

    #[test]
    fn unset_installs_automatically_when_it_can() {
        assert_eq!(launch_action(&facts(PreciseStatus::Unset)), Action::AutoInstall);
        assert_eq!(launch_action(&Facts { dev_tools: false, ..facts(PreciseStatus::Unset) }), Action::Nothing);
        assert_eq!(launch_action(&Facts { claude_found: false, ..facts(PreciseStatus::Unset) }), Action::Nothing);
        assert_eq!(launch_action(&Facts { plugin_enabled: None, ..facts(PreciseStatus::Unset) }), Action::Nothing);
    }

    #[test]
    fn unset_but_already_enabled_is_recorded_as_on() {
        assert_eq!(launch_action(&Facts { plugin_enabled: Some(true), ..facts(PreciseStatus::Unset) }), Action::MarkOn);
    }

    #[test]
    fn a_plugin_removed_outside_the_app_stays_off() {
        assert_eq!(launch_action(&facts(PreciseStatus::Auto)), Action::MarkOff);
        assert_eq!(launch_action(&facts(PreciseStatus::On)), Action::MarkOff);
        assert_eq!(launch_action(&Facts { plugin_enabled: None, ..facts(PreciseStatus::On) }), Action::Nothing);
    }

    #[test]
    fn an_enabled_plugin_whose_marketplace_moved_is_repointed() {
        let enabled = |dir| Facts { plugin_enabled: Some(true), marketplace_dir_exists: dir, ..facts(PreciseStatus::Auto) };
        assert_eq!(launch_action(&enabled(Some(false))), Action::Repoint);
        assert_eq!(launch_action(&enabled(Some(true))), Action::Nothing);
        assert_eq!(launch_action(&enabled(None)), Action::Nothing);
    }

    #[test]
    fn off_does_nothing() {
        assert_eq!(launch_action(&Facts { plugin_enabled: Some(true), ..facts(PreciseStatus::Off) }), Action::Nothing);
        assert_eq!(launch_action(&facts(PreciseStatus::Off)), Action::Nothing);
    }
}
```

Run: `cargo test -p claude-code-manager-app precise_status` → the non-`Nothing` expectations FAIL.

- [ ] **Step 4: Implement**

```rust
pub fn launch_action(f: &Facts) -> Action {
    if f.legacy_hooks {
        return if f.claude_found { Action::Migrate } else { Action::Nothing };
    }
    match (f.choice, f.plugin_enabled) {
        (PreciseStatus::Off, _) | (_, None) => Action::Nothing,
        (PreciseStatus::Unset, Some(true)) => Action::MarkOn,
        (PreciseStatus::Unset, Some(false)) if f.claude_found && f.dev_tools => Action::AutoInstall,
        (PreciseStatus::Unset, Some(false)) => Action::Nothing,
        (PreciseStatus::Auto | PreciseStatus::On, Some(false)) => Action::MarkOff,
        (PreciseStatus::Auto | PreciseStatus::On, Some(true)) if f.marketplace_dir_exists == Some(false) => Action::Repoint,
        (PreciseStatus::Auto | PreciseStatus::On, Some(true)) => Action::Nothing,
    }
}
```

Note the `(_, None)` arm must come after the `Off` arm but before the rest; `claude_found: false` makes `plugin list` impossible, so the launch thread reports `plugin_enabled: None` in that case.

Run: `cargo test -p claude-code-manager-app precise_status config::` → all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/config.rs crates/app/src/commands.rs crates/app/src/precise_status.rs crates/app/src/lib.rs
git commit -m "feat(app): remember the precise status choice and decide what launch does"
```

---

### Task 4: Launch thread, commands, and retiring the settings.json installer

**Files:**
- Modify: `crates/app/src/precise_status.rs` (launch thread, commands), `crates/app/src/hooks.rs` (remove `RELAY_SCRIPT`, `relay_path`, `install`, `uninstall`, `installed`; add `legacy_installed`, `remove_legacy`; `install_into` becomes test-only), `crates/app/src/commands.rs` (remove `hooks_*`), `crates/app/src/lib.rs` (register commands, start thread)

**Interfaces:**
- Consumes: Task 2's `plugin::*`, Task 3's `launch_action`, `Facts`, `Action`, `PreciseStatus`.
- Produces: commands `precise_status_choice() -> PreciseStatus`, `precise_status_enabled() -> Result<bool, String>` (async), `precise_status_enable(app) -> Result<(), String>` (async), `precise_status_disable() -> Result<(), String>` (async); event `precise-status-auto` (no payload); `pub fn start(app: &AppHandle)`.

- [ ] **Step 1: hooks.rs keeps only what migration needs**

Remove `RELAY_NAME`'s doc mention of installing, keep `RELAY_NAME` (it identifies legacy entries). Delete `RELAY_SCRIPT`, `relay_path`, `install`, `uninstall`, `installed`. Mark `install_into` `#[cfg(test)]` (its round-trip test still pins `uninstall_from` as an exact inverse). Add:

```rust
/// Our entries from before the plugin, still in `~/.claude/settings.json`.
pub fn legacy_installed() -> bool {
    read_settings().map(|s| is_installed(&s)).unwrap_or(false)
}

/// Removes those entries, leaving everyone else's hooks and settings untouched.
pub fn remove_legacy() -> Result<(), String> {
    edit_settings(uninstall_from)
}
```

Update the module doc's "installed into `~/.claude/settings.json`, opt-in" to "shipped as the bundled `ccm-status` Claude Code plugin".

- [ ] **Step 2: Launch thread and commands in precise_status.rs**

```rust
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

fn bundled_plugin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().resource_dir().map(|d| d.join("plugin")).map_err(|e| e.to_string())
}

fn set_choice(choice: PreciseStatus) -> Result<(), String> {
    let mut cfg = crate::config::load();
    cfg.integrations.precise_status = choice;
    crate::config::save(&cfg).map_err(|e| e.to_string())
}

fn gather() -> Facts {
    let claude_found = crate::plugin::claude_found();
    let plugin_enabled = if claude_found { crate::plugin::enabled().ok() } else { None };
    let marketplace_dir_exists = if plugin_enabled == Some(true) {
        crate::plugin::marketplace_path().ok().map(|p| p.is_some_and(|p| p.is_dir()))
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

/// Applies the launch decision once, in the background; installed builds only, since a dev build would re-point the marketplace.
pub fn start(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(dir) = bundled_plugin_dir(&app) else { return };
        let _ = match launch_action(&gather()) {
            Action::Nothing => Ok(()),
            Action::Migrate => crate::plugin::install(&dir).and_then(|_| crate::hooks::remove_legacy()).and_then(|_| set_choice(PreciseStatus::On)),
            Action::AutoInstall => crate::plugin::install(&dir).and_then(|_| set_choice(PreciseStatus::Auto)).map(|_| {
                let _ = app.emit("precise-status-auto", ());
            }),
            Action::MarkOn => set_choice(PreciseStatus::On),
            Action::MarkOff => set_choice(PreciseStatus::Off),
            Action::Repoint => crate::plugin::install(&dir),
        };
    });
}

#[tauri::command]
pub fn precise_status_choice() -> PreciseStatus {
    crate::config::load().integrations.precise_status
}

#[tauri::command]
pub async fn precise_status_enabled() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(crate::plugin::enabled).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn precise_status_enable(app: AppHandle) -> Result<(), String> {
    if !crate::plugin::developer_tools_present() {
        return Err("Needs Apple's command line tools: run `xcode-select --install`, then try again.".into());
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
```

(`Action::Repoint` re-runs install: `marketplace add` re-points the name to the bundle and `install` of an already-installed plugin succeeds or reports already installed; if it returns an error for "already installed", call only the `marketplace add` step — check once in Step 4 against an isolated `CLAUDE_CONFIG_DIR`.)

- [ ] **Step 3: Wire lib.rs and commands.rs**

Remove the three `hooks_*` functions from commands.rs and from `generate_handler!`. Register `precise_status::precise_status_choice`, `precise_status_enabled`, `precise_status_enable`, `precise_status_disable`. In `setup`, after `update_check::start(app.handle());` add `precise_status::start(app.handle());`.

- [ ] **Step 4: Verify behaviour against an isolated Claude config**

Run: `cargo test -p claude-code-manager-app` and `cargo clippy --workspace --all-targets -- -D warnings` → clean.
In a scratch dir, with `CLAUDE_CONFIG_DIR` pointed at an empty scratch folder: run `claude plugin marketplace add <repo>/crates/app/plugin`, `claude plugin install ccm-status@claude-code-manager --scope user --json` twice (confirm the second call's exit status and message, for `Repoint`), `claude plugin list --json`, then `claude -p ok --model haiku` and check `hook-events.ndjson` under a scratch `CLAUDE_CODE_MANAGER_CONFIG_DIR` gains `SessionStart`/`UserPromptSubmit` lines. Adjust `Action::Repoint` per the second install's result.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/precise_status.rs crates/app/src/hooks.rs crates/app/src/commands.rs crates/app/src/lib.rs
git commit -m "feat(app): install the status plugin at launch and from Settings, migrating old hooks"
```

---

### Task 5: Settings toggle and the sidebar notice

**Files:**
- Create: `crates/app/frontend/src/lib/preciseStatus.ts`, `crates/app/frontend/src/lib/preciseStatus.test.ts`, `crates/app/frontend/src/hooks/usePreciseStatusChoice.ts`, `crates/app/frontend/src/components/PreciseStatusNote.tsx`
- Modify: `crates/app/frontend/src/components/Settings.tsx`, `crates/app/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: Task 4's commands and `precise-status-auto` event.
- Produces: `type PreciseStatusChoice = "unset" | "auto" | "on" | "off"`, `showsPreciseStatusNotice(choice: PreciseStatusChoice | null, dismissed: boolean): boolean`, `usePreciseStatusChoice(): PreciseStatusChoice | null`, `<PreciseStatusNote onOpenSettings={() => void} />`.

- [ ] **Step 1: Failing notice-visibility test**

`lib/preciseStatus.ts`:

```ts
export type PreciseStatusChoice = "unset" | "auto" | "on" | "off";

/** The sidebar announces an automatic install once: while the app enabled it on its own and it wasn't dismissed. */
export function showsPreciseStatusNotice(choice: PreciseStatusChoice | null, dismissed: boolean): boolean {
  void choice;
  void dismissed;
  return false;
}
```

`lib/preciseStatus.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { showsPreciseStatusNotice } from "./preciseStatus";

describe("showsPreciseStatusNotice", () => {
  it("shows after an automatic install until dismissed", () => {
    expect(showsPreciseStatusNotice("auto", false)).toBe(true);
    expect(showsPreciseStatusNotice("auto", true)).toBe(false);
  });

  it("doesn't show for a choice the user made, or before the backend answers", () => {
    for (const c of ["unset", "on", "off"] as const) expect(showsPreciseStatusNotice(c, false)).toBe(false);
    expect(showsPreciseStatusNotice(null, false)).toBe(false);
  });
});
```

Run: `npm --prefix crates/app/frontend run test -- preciseStatus` → first test FAILS. Implement `return choice === "auto" && !dismissed;` → passes.

- [ ] **Step 2: Hook and note**

`hooks/usePreciseStatusChoice.ts`:

```ts
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PreciseStatusChoice } from "../lib/preciseStatus";

/** The backend-owned precise status choice; `auto` arrives as an event when launch installs the plugin. */
export function usePreciseStatusChoice(): PreciseStatusChoice | null {
  const [choice, setChoice] = useState<PreciseStatusChoice | null>(null);
  useEffect(() => {
    const un = listen("precise-status-auto", () => setChoice("auto"));
    invoke<PreciseStatusChoice>("precise_status_choice")
      .then((c) => setChoice((cur) => cur ?? c))
      .catch(() => {});
    return () => {
      un.then((f) => f());
    };
  }, []);
  return choice;
}
```

`components/PreciseStatusNote.tsx` — same frame as `UpdateNote` (border-top, `px-3 py-2`, lucide `X` size 12):

```tsx
import { Activity, X } from "lucide-react";
import { usePersistentState } from "../hooks/usePersistentState";
import { usePreciseStatusChoice } from "../hooks/usePreciseStatusChoice";
import { showsPreciseStatusNotice } from "../lib/preciseStatus";

/** Foot of the sidebar: says once that the app turned precise status on by adding a plugin to Claude Code. */
export default function PreciseStatusNote({ onOpenSettings }: { onOpenSettings: () => void }) {
  const choice = usePreciseStatusChoice();
  const [dismissed, setDismissed] = usePersistentState("cm:preciseStatusNoticeDismissed", false);
  if (!showsPreciseStatusNotice(choice, dismissed)) return null;
  return (
    <div className="shrink-0 flex items-start gap-1.5 px-3 py-2 border-t border-[var(--color-border)] text-xs">
      <Activity size={13} strokeWidth={2} className="shrink-0 mt-px text-[var(--color-accent)]" />
      <p className="flex-1 min-w-0 text-[var(--color-text)]">
        Precise session status is on — ccm added its status plugin to Claude Code.{" "}
        <button type="button" onClick={onOpenSettings} className="text-[var(--color-accent)] underline underline-offset-2 cursor-pointer hover:opacity-80">
          Settings
        </button>
      </p>
      <button
        onClick={() => setDismissed(true)}
        title="Dismiss"
        aria-label="Dismiss precise status notice"
        className="shrink-0 flex items-center p-0.5 text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
      >
        <X size={12} strokeWidth={2} />
      </button>
    </div>
  );
}
```

(Confirm `Activity` exists in `node_modules/lucide-react/dist/esm/icons/activity.mjs`; otherwise use `CircleCheck`.)

Sidebar.tsx: import it and render `<PreciseStatusNote onOpenSettings={onOpenSettings} />` directly before `<UpdateNote />`.

- [ ] **Step 3: Settings toggle**

Replace the hooks state in Settings.tsx:

```tsx
  // The plugin's state lives in Claude Code, so ask it rather than mirror it.
  const [statusPlugin, setStatusPlugin] = useState<boolean | null>(null);
  const [statusPluginBusy, setStatusPluginBusy] = useState(false);
  const [statusPluginError, setStatusPluginError] = useState<string | null>(null);
  useEffect(() => {
    invoke<boolean>("precise_status_enabled").then(setStatusPlugin).catch((e) => setStatusPluginError(String(e)));
  }, []);
  const toggleStatusPlugin = (want: boolean) => {
    setStatusPluginError(null);
    setStatusPluginBusy(true);
    invoke(want ? "precise_status_enable" : "precise_status_disable")
      .then(() => setStatusPlugin(want))
      .catch((e) => setStatusPluginError(String(e)))
      .finally(() => setStatusPluginBusy(false));
  };
```

The `CheckRow` becomes `checked={statusPlugin ?? false}` / `onChange={toggleStatusPlugin}`; disable the checkbox while busy by adding an optional `disabled` prop to `CheckRow` (`<input … disabled={disabled} />`). The description paragraph becomes:

```tsx
          <div className="text-[10px] text-[var(--color-text-dim)] leading-relaxed">
            Adds the <code>ccm-status</code> plugin to Claude Code so Claude itself reports each session's
            state: a question or permission prompt shows as needing you the moment it appears, and a finished
            turn within seconds. Without it, status is inferred from session files and terminal output. Applies
            to sessions started after the change; remove it here or with <code>/plugin</code> in Claude Code.
          </div>
```

and `hooksError` references become `statusPluginError`.

- [ ] **Step 4: Build and test**

Run: `npm --prefix crates/app/frontend run test && npm --prefix crates/app/frontend run build` → all pass, `tsc` clean.

- [ ] **Step 5: Commit**

```bash
git add crates/app/frontend/src
git commit -m "feat(app): Settings drives the status plugin, and the sidebar announces an automatic install"
```

---

### Task 6: Docs and the full gate

**Files:**
- Modify: `FEATURES.md` (Integrations → precise status entry), `CHANGELOG.md` (new `## Unreleased` with **Changed**)

- [ ] **Step 1: FEATURES.md**

Rewrite the "Precise status via Claude Code hooks" entry's first sentence to: "**Precise status via Claude Code hooks** (on by default — Settings → Integrations) — the bundled `ccm-status` Claude Code plugin (a directory marketplace in the app's resources, installed through `claude plugin`) relays claude's own hook events …", keep the relay/fold/witness description, and add: "The installed app installs it at launch for anyone who hasn't chosen (when `claude` and Apple's developer tools are present) and says so once in the sidebar; turning it off — in Settings or with `/plugin` — is remembered in `integrations.preciseStatus`, which only the backend writes. Old `settings.json` hook entries migrate to the plugin. Deleting the app leaves the plugin inert rather than erroring. → `2026-09-17-precise-status-plugin-design`".

- [ ] **Step 2: CHANGELOG.md**

Insert above `## v0.1.3 — 2026-09-17`:

```markdown
## Unreleased

**Changed**

- **Precise session status is on by default, as a Claude Code plugin.** Claude reports each session's state itself — a question or permission prompt shows as needing you the moment it appears — through a small `ccm-status` plugin the app adds to Claude Code, and a note in the sidebar says so. Turn it off in Settings or with `/plugin`, and it stays off. If you had switched it on before, its hooks move out of `~/.claude/settings.json` into the plugin. It needs Apple's command line tools, and uninstalling the app leaves nothing that errors.

```

- [ ] **Step 3: Full gate**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test
npm --prefix crates/app/frontend run test
npm --prefix crates/app/frontend run build
make build
claude plugin validate crates/app/plugin
```

All must pass. Then `find target/release/plugin -type f` lists the four plugin files.

- [ ] **Step 4: Commit and hand over**

```bash
git add FEATURES.md CHANGELOG.md
git commit -m "docs: precise status plugin in FEATURES and CHANGELOG"
```

Tick the TODO in the user's working copy only (its line is uncommitted). Hand to the user for the manual test in the spec (`make install`, then launch the installed app).
