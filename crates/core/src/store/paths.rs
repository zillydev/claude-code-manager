use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Derive a human label for a project directory name.
///
/// Use the last `-`-separated component of the encoded dir name — this is
/// almost always the original project's basename even though the rest of the
/// path is lossy (e.g. `-Users-alice-dev-my-cool-tool` → `tool`). When the caller has access to the real `cwd` from a session
/// event, prefer `label_from_cwd` instead.
pub fn fallback_label(name: &str) -> String {
    name.rsplit('-')
        .find(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string()
}

/// Last path component of an actual cwd string. This is the ideal label.
pub fn label_from_cwd(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| cwd.to_string())
}

/// Default Claude root: `$CLAUDE_CONFIG_DIR` when set and non-empty (as the `claude` CLI does), else `~/.claude`.
pub fn default_claude_root() -> Option<PathBuf> {
    claude_root_from(std::env::var_os("CLAUDE_CONFIG_DIR"), dirs::home_dir())
}

pub fn claude_root_from(config_dir: Option<std::ffi::OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match config_dir {
        Some(d) if !d.is_empty() => Some(PathBuf::from(d)),
        _ => home.map(|h| h.join(".claude")),
    }
}

/// `<root>/projects` — where session jsonl files live.
pub fn projects_dir(root: &Path) -> PathBuf {
    root.join("projects")
}

/// Encode a real cwd into Claude Code's project directory name (replace `/` with `-`).
/// Encode a cwd into Claude Code's project-dir name. CC replaces every
/// non-alphanumeric character with `-` (so `/`, `.`, `_`, space all collapse);
/// letters/digits and case are kept. Lossy (a real `-` is indistinguishable),
/// but matches CC's own encoding so the resulting dir is the one CC reads/writes.
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Full path to a session file for `cwd`/`id` under `<root>/projects/<encoded>/<id>.jsonl`.
pub fn session_file_path(root: &Path, cwd: &str, id: Uuid) -> PathBuf {
    projects_dir(root)
        .join(encode_project_dir(cwd))
        .join(format!("{}.jsonl", id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_root_prefers_config_dir() {
        assert_eq!(claude_root_from(Some("/sb".into()), Some("/h".into())), Some("/sb".into()));
    }

    #[test]
    fn claude_root_falls_back_to_home() {
        assert_eq!(claude_root_from(None, Some("/h".into())), Some("/h/.claude".into()));
    }

    #[test]
    fn claude_root_ignores_empty_config_dir() {
        assert_eq!(claude_root_from(Some("".into()), Some("/h".into())), Some("/h/.claude".into()));
    }

    #[test]
    fn fallback_label_uses_last_component() {
        // For a project whose actual basename has a hyphen (e.g. my-cool-tool),
        // the lossy fallback can only recover the trailing word — which is
        // why callers should prefer label_from_cwd whenever possible.
        assert_eq!(fallback_label("-Users-alice-dev-my-cool-tool"), "tool");
        assert_eq!(fallback_label("-Users-alice"), "alice");
    }

    #[test]
    fn label_from_real_cwd() {
        assert_eq!(
            label_from_cwd("/Users/alice/dev/my-cool-tool"),
            "my-cool-tool"
        );
        assert_eq!(label_from_cwd("/tmp/x"), "x");
    }

    #[test]
    fn encode_project_dir_matches_cc() {
        // CC maps every non-alphanumeric char to '-' (verified empirically).
        assert_eq!(encode_project_dir("/Users/x/my.app"), "-Users-x-my-app");
        assert_eq!(encode_project_dir("/tmp/a_b c"), "-tmp-a-b-c");
        assert_eq!(encode_project_dir("/Users/x/v1.2-rc"), "-Users-x-v1-2-rc");
        assert_eq!(
            encode_project_dir("/Users/alice/dev/my-cool-tool"),
            "-Users-alice-dev-my-cool-tool"
        );
    }

    #[test]
    fn encode_then_session_path() {
        let dir = encode_project_dir("/Users/alice/dev/my-cool-tool");
        assert_eq!(dir, "-Users-alice-dev-my-cool-tool");

        let root = std::path::Path::new("/tmp/.claude");
        let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap();
        let p = session_file_path(root, "/Users/x/proj", id);
        assert_eq!(
            p,
            std::path::PathBuf::from("/tmp/.claude/projects/-Users-x-proj/00000000-0000-0000-0000-0000000000aa.jsonl")
        );
    }
}
