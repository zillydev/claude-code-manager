//! Files a shell command changed, as Claude Code records them beside its result (`toolUseResult.bashEditDiff`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileChanges {
    #[serde(default)]
    pub files: Vec<FileChange>,
    /// Changed files Claude Code left out of `files`.
    #[serde(default, rename(deserialize = "moreFiles"))]
    pub more: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileChange {
    #[serde(rename(deserialize = "filePath"))]
    pub path: String,
    /// The command created the file rather than changing it.
    #[serde(default)]
    pub created: bool,
    #[serde(default)]
    pub hunks: Vec<Hunk>,
}

/// One unified-diff hunk; each line keeps its ` `, `+` or `-` prefix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct Hunk {
    pub old_start: u64,
    pub old_lines: u64,
    pub new_start: u64,
    pub new_lines: u64,
    pub lines: Vec<String>,
}

/// The file changes a tool result line records, or `None` when it records none.
pub fn from_tool_use_result(value: Option<&serde_json::Value>) -> Option<FileChanges> {
    let diff = value?.get("bashEditDiff")?;
    let changes = FileChanges::deserialize(diff).ok()?;
    (!changes.files.is_empty() || changes.more > 0).then_some(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result_with(diff: serde_json::Value) -> serde_json::Value {
        json!({ "stdout": "done", "stderr": "", "interrupted": false, "bashEditDiff": diff })
    }

    #[test]
    fn reads_each_changed_file_and_its_hunks() {
        let v = result_with(json!({
            "files": [{
                "filePath": "/repo/CHANGELOG.md",
                "hunks": [{ "oldStart": 8, "oldLines": 2, "newStart": 8, "newLines": 3, "lines": [" a", "+b", " c"] }]
            }],
            "moreFiles": 0,
            "changedFiles": ["/repo/CHANGELOG.md"]
        }));
        let changes = from_tool_use_result(Some(&v)).expect("changes");
        assert_eq!(changes.more, 0);
        assert_eq!(
            changes.files,
            vec![FileChange {
                path: "/repo/CHANGELOG.md".into(),
                created: false,
                hunks: vec![Hunk {
                    old_start: 8,
                    old_lines: 2,
                    new_start: 8,
                    new_lines: 3,
                    lines: vec![" a".into(), "+b".into(), " c".into()],
                }],
            }]
        );
    }

    #[test]
    fn marks_a_created_file() {
        let v = result_with(json!({
            "files": [{
                "filePath": "/repo/new.rs",
                "created": true,
                "hunks": [{ "oldStart": 0, "oldLines": 0, "newStart": 1, "newLines": 1, "lines": ["+fn main() {}"] }]
            }],
            "moreFiles": 0
        }));
        assert!(from_tool_use_result(Some(&v)).expect("changes").files[0].created);
    }

    #[test]
    fn a_shared_record_without_files_is_none() {
        let v = result_with(json!({ "files": [], "moreFiles": 0, "shared": true }));
        assert_eq!(from_tool_use_result(Some(&v)), None);
    }

    #[test]
    fn only_files_left_out_still_counts() {
        let v = result_with(json!({ "files": [], "moreFiles": 3 }));
        assert_eq!(from_tool_use_result(Some(&v)).map(|c| c.more), Some(3));
    }

    #[test]
    fn a_result_without_a_bash_diff_is_none() {
        assert_eq!(from_tool_use_result(None), None);
        assert_eq!(from_tool_use_result(Some(&json!("Error: exit 1"))), None);
        let edit = json!({ "filePath": "/repo/a.rs", "structuredPatch": [] });
        assert_eq!(from_tool_use_result(Some(&edit)), None);
    }

    #[test]
    fn a_malformed_bash_diff_is_none() {
        let v = result_with(json!({ "files": "nope" }));
        assert_eq!(from_tool_use_result(Some(&v)), None);
    }

    #[test]
    fn serializes_for_the_frontend_in_snake_case() {
        let v = result_with(json!({
            "files": [{ "filePath": "/a", "hunks": [{ "oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["-x", "+y"] }] }],
            "moreFiles": 2
        }));
        let out = serde_json::to_value(from_tool_use_result(Some(&v)).expect("changes")).unwrap();
        assert_eq!(out["more"], 2);
        assert_eq!(out["files"][0]["path"], "/a");
        assert_eq!(out["files"][0]["created"], false);
        assert_eq!(out["files"][0]["hunks"][0]["new_start"], 1);
    }
}
