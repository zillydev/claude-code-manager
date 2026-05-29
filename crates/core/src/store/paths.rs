use std::path::{Path, PathBuf};

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
}
