//! The bundled `ccm-status` plugin, installed and inspected through Claude Code's own `claude plugin` CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MARKETPLACE: &str = "claude-code-manager";
pub const PLUGIN_ID: &str = "ccm-status@claude-code-manager";
const CLI_TIMEOUT: Duration = Duration::from_secs(30);

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
    vec![
        args(&["plugin", "uninstall", PLUGIN_ID, "--json"]),
        args(&["plugin", "marketplace", "remove", MARKETPLACE]),
    ]
}

fn parse_list(json: &str) -> Result<Vec<serde_json::Value>, String> {
    serde_json::from_str(json)
        .map_err(|_| format!("unexpected output from claude: {}", json.trim()))
}

/// Whether `claude plugin list --json` shows our plugin installed and enabled.
pub fn enabled_in(list_json: &str) -> Result<bool, String> {
    Ok(parse_list(list_json)?
        .iter()
        .any(|p| p["id"] == PLUGIN_ID && p["enabled"] == true))
}

/// Our marketplace's recorded directory in `claude plugin marketplace list --json`.
pub fn marketplace_path_in(json: &str) -> Result<Option<PathBuf>, String> {
    Ok(parse_list(json)?
        .iter()
        .find(|m| m["name"] == MARKETPLACE)
        .and_then(|m| m["path"].as_str())
        .map(PathBuf::from))
}

/// Runs `claude <args>` and returns stdout; a non-zero exit, a timeout or a missing `claude` is an error.
fn run(cli_args: &[String]) -> Result<String, String> {
    let claude = crate::terminal::find_claude()?;
    let mut cmd = Command::new(&claude.program);
    cmd.args(cli_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = &claude.path {
        cmd.env("PATH", path);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("couldn't run claude: {e}"))?;
    let started = Instant::now();
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if started.elapsed() > CLI_TIMEOUT {
            let _ = child.kill();
            return Err(format!("`claude {}` timed out", cli_args.join(" ")));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Read after exit: `claude plugin` prints a few KB, well under the pipe buffer.
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let reason = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    Err(format!("`claude {}` failed: {reason}", cli_args.join(" ")))
}

pub fn claude_found() -> bool {
    crate::terminal::find_claude().is_ok()
}

/// Apple's developer tools, without which `/usr/bin/python3` opens an install dialog instead of running.
pub fn developer_tools_present() -> bool {
    Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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
    let [plugin, marketplace] = <[Vec<String>; 2]>::try_from(uninstall_steps()).expect("two steps");
    if let Err(e) = run(&plugin) {
        if enabled()? {
            return Err(e);
        }
    }
    if marketplace_path()?.is_some() {
        run(&marketplace)?;
    }
    Ok(())
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
                cli(&[
                    "plugin",
                    "marketplace",
                    "add",
                    "/App.app/Contents/Resources/plugin"
                ]),
                cli(&["plugin", "install", PLUGIN_ID, "--scope", "user", "--json"]),
            ]
        );
    }

    #[test]
    fn uninstall_removes_the_plugin_then_the_marketplace() {
        assert_eq!(
            uninstall_steps(),
            vec![
                cli(&["plugin", "uninstall", PLUGIN_ID, "--json"]),
                cli(&["plugin", "marketplace", "remove", MARKETPLACE]),
            ]
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
        assert_eq!(
            marketplace_path_in(json),
            Ok(Some(PathBuf::from("/App.app/Contents/Resources/plugin")))
        );
        assert_eq!(marketplace_path_in("[]"), Ok(None));
        assert!(marketplace_path_in("{").is_err());
    }
}
