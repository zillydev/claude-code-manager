use std::path::Path;
use std::sync::Mutex;

use claude_manager_core::session::summary::SessionSummary;
use claude_manager_core::store;

use crate::commands::SessionActivity;

/// App-global cache shared by every window: one discovery + one activity scan,
/// computed once (single-flight) and refreshed by the background poller.
pub struct Shared {
    data: Mutex<SharedData>,
    /// Serializes `discover()` so concurrent cold-cache callers don't duplicate it.
    compute: Mutex<()>,
}

#[derive(Default)]
struct SharedData {
    sessions: Option<Vec<SessionSummary>>,
    activity: Vec<SessionActivity>,
    file_sig: u64,
}

pub struct PollOutcome {
    pub activity: Vec<SessionActivity>,
    pub sessions_changed: bool,
}

impl Shared {
    pub fn new() -> Self {
        Shared { data: Mutex::new(SharedData::default()), compute: Mutex::new(()) }
    }

    /// Cached sessions; computes once under `compute` on a cold cache.
    pub fn sessions(&self, root: &Path) -> Result<Vec<SessionSummary>, String> {
        if let Some(s) = self.data.lock().unwrap().sessions.clone() {
            return Ok(s);
        }
        let _c = self.compute.lock().unwrap();
        if let Some(s) = self.data.lock().unwrap().sessions.clone() {
            return Ok(s); // another caller filled it while we waited
        }
        self.discover_into(root)
    }

    /// Force a re-discover (manual refresh), bypassing the cache.
    pub fn refresh_sessions(&self, root: &Path) -> Result<Vec<SessionSummary>, String> {
        let _c = self.compute.lock().unwrap();
        self.discover_into(root)
    }

    pub fn activity(&self) -> Vec<SessionActivity> {
        self.data.lock().unwrap().activity.clone()
    }

    /// One poller tick: refresh activity (always) and re-discover iff the dir
    /// signature changed. Returns what to broadcast.
    pub fn poll(&self, root: &Path) -> PollOutcome {
        let activity: Vec<SessionActivity> = store::scan_activity(root, 60)
            .into_iter()
            .map(|(id, t, awaiting)| SessionActivity {
                id: id.to_string(),
                last_event_at: t
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                awaiting_input: awaiting,
            })
            .collect();
        self.data.lock().unwrap().activity = activity.clone();

        let sig = store::dir_signature(root);
        let changed = self.data.lock().unwrap().file_sig != sig;
        let sessions_changed = if changed {
            let _c = self.compute.lock().unwrap();
            let _ = self.discover_into(root);
            true
        } else {
            false
        };
        PollOutcome { activity, sessions_changed }
    }

    /// Discover + record signature + store. Caller must hold `compute`.
    fn discover_into(&self, root: &Path) -> Result<Vec<SessionSummary>, String> {
        let disc = store::discover(root).map_err(|e| e.to_string())?;
        let sig = store::dir_signature(root);
        let mut d = self.data.lock().unwrap();
        d.sessions = Some(disc.sessions.clone());
        d.file_sig = sig;
        Ok(disc.sessions)
    }
}
