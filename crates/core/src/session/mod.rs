pub mod edit;
pub mod event;
pub mod parse;
pub mod raw;
pub mod render;
pub mod summary;
pub mod surgery;
pub mod tree;

use std::path::PathBuf;
use std::time::SystemTime;

use uuid::Uuid;

use event::Event;

/// A fully-parsed session, used when the transcript view is open.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Session {
    pub project_label: String,
    pub id: Uuid,
    pub file_path: PathBuf,
    pub events: Vec<Event>,
    pub leaf: Option<Uuid>,
    #[allow(dead_code)]
    pub mtime: SystemTime,
    pub bytes_read: u64,
}

impl Session {
    pub fn recompute_leaf(&mut self) {
        self.leaf = tree::resolve_leaf(&self.events);
    }

    /// Events on the active conversation path, oldest first.
    pub fn current_path(&self) -> Vec<&Event> {
        match self.leaf {
            Some(l) => tree::current_path(&self.events, l),
            None => Vec::new(),
        }
    }
}
