use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Derive a human label for a project directory name.
///
/// Use the last `-`-separated component of the encoded dir name — this is
/// almost always the original project's basename even though the rest of the
/// path is lossy (e.g. `-Users-zillydev-my-Dev-Personal-claude-manager` →
/// `manager`). When the caller has access to the real `cwd` from a session
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

/// Default `~/.claude` root.
pub fn default_claude_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude"))
}

/// `<root>/projects` — where session jsonl files live.
pub fn projects_dir(root: &Path) -> PathBuf {
    root.join("projects")
}

/// Encode a real cwd into Claude Code's project directory name (replace `/` with `-`).
/// This is the inverse Claude Code applies; it is lossy for paths containing real
/// `-`, but matches CC's own encoding so the resulting dir is the one CC reads.
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.replace('/', "-")
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
    fn fallback_label_uses_last_component() {
        // For a project whose actual basename has a hyphen (e.g. claude-manager),
        // the lossy fallback can only recover the trailing word — which is
        // why callers should prefer label_from_cwd whenever possible.
        assert_eq!(
            fallback_label("-Users-zillydev-my-Dev-Personal-claude-manager"),
            "manager"
        );
        assert_eq!(fallback_label("-Users-zillydev"), "zillydev");
    }

    #[test]
    fn label_from_real_cwd() {
        assert_eq!(
            label_from_cwd("/Users/zillydev/my/Dev/Personal/claude-manager"),
            "claude-manager"
        );
        assert_eq!(label_from_cwd("/tmp/x"), "x");
    }

    #[test]
    fn encode_then_session_path() {
        let dir = encode_project_dir("/Users/zillydev/my/Dev/Personal/claude-manager");
        assert_eq!(dir, "-Users-zillydev-my-Dev-Personal-claude-manager");

        let root = std::path::Path::new("/tmp/.claude");
        let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap();
        let p = session_file_path(root, "/Users/x/proj", id);
        assert_eq!(
            p,
            std::path::PathBuf::from("/tmp/.claude/projects/-Users-x-proj/00000000-0000-0000-0000-0000000000aa.jsonl")
        );
    }
}
