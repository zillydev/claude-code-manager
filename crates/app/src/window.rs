//! Multi-window helpers: allocate a fresh window label and spawn an
//! independent webview window loading the same frontend.

use std::collections::HashSet;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Return the first `win-N` label (N starting at 1) not already in `existing`.
/// The bootstrap window keeps its configured `main` label, so `win-N` never
/// collides with it. Reusing freed labels keeps numbers from growing without
/// bound as windows open and close.
pub fn next_window_label(existing: &HashSet<String>) -> String {
    (1..)
        .map(|n| format!("win-{n}"))
        .find(|label| !existing.contains(label))
        .expect("an unbounded range always yields a free label")
}

/// Open a new, independent Claude Manager window loading the same frontend.
/// Windows cascade slightly so they don't stack exactly on top of each other.
pub fn spawn_window(app: &AppHandle) {
    let existing: HashSet<String> = app.webview_windows().keys().cloned().collect();
    let label = next_window_label(&existing);
    let offset = existing.len() as f64 * 28.0;

    if let Err(e) = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("Claude Code Manager")
        .inner_size(1200.0, 800.0)
        .min_inner_size(800.0, 600.0)
        .position(120.0 + offset, 120.0 + offset)
        .resizable(true)
        .build()
    {
        eprintln!("failed to open new window: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_label_is_win_1() {
        let existing = HashSet::new();
        assert_eq!(next_window_label(&existing), "win-1");
    }

    #[test]
    fn skips_taken_labels_including_main() {
        let existing: HashSet<String> =
            ["main", "win-1", "win-2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(next_window_label(&existing), "win-3");
    }

    #[test]
    fn reuses_freed_label_in_the_middle() {
        // win-2 was closed; the next window reuses it instead of jumping to win-4.
        let existing: HashSet<String> =
            ["main", "win-1", "win-3"].iter().map(|s| s.to_string()).collect();
        assert_eq!(next_window_label(&existing), "win-2");
    }
}
