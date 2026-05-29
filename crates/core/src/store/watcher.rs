use std::path::Path;
use std::sync::mpsc::Sender;

use anyhow::Result;
use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// A handle that keeps a notify watcher alive for the lifetime of the handle.
/// Drop to stop watching.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

/// Watch `path` for modify events. Each modify drops a `()` on `tx`. The
/// caller is responsible for debouncing + re-reading the file.
pub fn watch_file(path: &Path, tx: Sender<()>) -> Result<FileWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
        let Ok(ev) = res else { return };
        if matches!(
            ev.kind,
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
        ) {
            let _ = tx.send(());
        }
    })?;
    watcher.watch(path, RecursiveMode::NonRecursive)?;
    Ok(FileWatcher { _watcher: watcher })
}
