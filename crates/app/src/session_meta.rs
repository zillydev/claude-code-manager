//! Claude-manager-local session organization: global colored tags, per-session
//! alias/tags/note. Stored as JSON at `<config-dir>/session-meta.json` —
//! NEVER inside claude's session `.jsonl` (a live `claude` writes those files,
//! and the old in-file `custom-title` rename desynced for exactly that reason).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Global tag registry: `name` is the identity, `color` a hex like "#539bf5".
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// Per-session metadata keyed by session id. BTreeMap for stable file diffs.
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub color: String,
    /// Retired from every chooser (filter menu, editor suggestions, find
    /// autocomplete) but still applied wherever it was; unarchive in Manage
    /// tags. MUST mirror the frontend Tag — a field missing here is silently
    /// dropped by serde on save (how bookmarks were lost once).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Bookmarked message uuids (the turn's head uuid), insertion order.
    /// MUST mirror the frontend's SessionEntry: this struct is what
    /// set_session_meta deserializes into, so any frontend field missing here
    /// is silently dropped by serde — the write "succeeds", the broadcast
    /// echoes the stripped meta back, and the UI watches its change revert.
    /// That is exactly how bookmarks were lost for a week.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<String>,
}

fn default_version() -> u32 {
    1
}

impl Default for SessionMeta {
    fn default() -> Self {
        SessionMeta { version: default_version(), tags: Vec::new(), sessions: BTreeMap::new() }
    }
}

pub fn meta_path() -> PathBuf {
    crate::config::config_dir().join("session-meta.json")
}

/// Read + parse. Missing or malformed → defaults (a malformed file is left
/// intact so the user can fix it). Partial JSON merges over field defaults.
fn load_from(path: &Path) -> SessionMeta {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => SessionMeta::default(),
    }
}

fn save_to(path: &Path, meta: &SessionMeta) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Load the store. A genuinely absent file is created with defaults.
pub fn load() -> SessionMeta {
    let path = meta_path();
    if !path.exists() {
        let meta = SessionMeta::default();
        let _ = save_to(&path, &meta);
        return meta;
    }
    load_from(&path)
}

pub fn save(meta: &SessionMeta) -> std::io::Result<()> {
    save_to(&meta_path(), meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cm-meta-test-{name}")).join("session-meta.json")
    }

    #[test]
    fn roundtrip_save_load() {
        let path = tmp("roundtrip");
        let mut meta = SessionMeta::default();
        meta.tags.push(Tag { name: "wip".into(), color: "#e5534b".into(), archived: false });
        meta.sessions.insert(
            "abc".into(),
            SessionEntry {
                alias: Some("auth refactor".into()),
                tags: vec!["wip".into()],
                note: Some("blocked on X".into()),
                bookmarks: vec!["00000000-0000-0000-0000-0000000000b1".into()],
            },
        );
        save_to(&path, &meta).unwrap();
        assert_eq!(load_from(&path), meta);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_default_empty() {
        let path = tmp("missing-xyz");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(load_from(&path), SessionMeta::default());
    }

    #[test]
    fn malformed_file_is_default() {
        let path = tmp("malformed");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ nope").unwrap();
        assert_eq!(load_from(&path), SessionMeta::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn partial_json_merges_defaults() {
        let path = tmp("partial");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Only a tag registry; version + sessions default. Entry fields default too.
        // NB: r##…## because the hex color contains `"#`, which would terminate r#…#.
        std::fs::write(
            &path,
            r##"{ "tags": [{"name":"wip","color":"#e5534b"}], "sessions": { "abc": {} } }"##,
        )
        .unwrap();
        let meta = load_from(&path);
        assert_eq!(meta.version, 1);
        assert_eq!(meta.tags.len(), 1);
        let e = &meta.sessions["abc"];
        assert_eq!(e.alias, None);
        assert!(e.tags.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn atomic_save_leaves_no_tmp() {
        let path = tmp("atomic");
        save_to(&path, &SessionMeta::default()).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn empty_entry_fields_are_skipped_in_json() {
        let json = serde_json::to_string(&SessionEntry {
            alias: None,
            tags: vec![],
            note: None,
            bookmarks: vec![],
        })
        .unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn archived_tag_flag_round_trips_and_defaults_false() {
        let json = r##"{ "tags": [{"name":"old","color":"#111","archived":true},{"name":"wip","color":"#222"}] }"##;
        let meta: SessionMeta = serde_json::from_str(json).unwrap();
        assert!(meta.tags[0].archived);
        assert!(!meta.tags[1].archived, "absent ⇒ active (existing meta reads unchanged)");
        let back = serde_json::to_string(&meta).unwrap();
        assert!(back.contains("\"archived\":true"), "{back}");
        assert_eq!(back.matches("archived").count(), 1, "false is not written: {back}");
    }

    #[test]
    fn bookmarks_survive_the_deserialize_round_trip() {
        // Regression: this struct is what set_session_meta deserializes into,
        // and a frontend field missing here is silently dropped — the write
        // "succeeds", the broadcast echoes stripped meta, and the UI watches
        // its toggle revert. Bookmarks were lost exactly this way.
        let json = r##"{ "sessions": { "abc": { "bookmarks": ["u1", "u2"] } } }"##;
        let meta: SessionMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.sessions["abc"].bookmarks, vec!["u1", "u2"]);
        let back = serde_json::to_string(&meta).unwrap();
        assert!(back.contains("bookmarks"), "must serialize back out: {back}");
    }
}
