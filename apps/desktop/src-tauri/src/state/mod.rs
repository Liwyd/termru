mod host_keys;
pub(crate) mod sessions;

pub use host_keys::HostKeyPrompts;
pub use sessions::{SessionInfo, SessionManager, SessionStatusDto, SessionUpdate};

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use termru_store::Store;

use crate::sync::SyncEngine;
use crate::sync::scheduler::SyncScheduler;

/// Name of the frontend event emitted when a sync cycle applied remote
/// changes (hosts, groups, prefs, ... may have changed under the UI).
pub const SYNC_APPLIED_EVENT: &str = "sync-applied";

/// Payload of the [`SYNC_APPLIED_EVENT`] event.
#[derive(Debug, Clone, Serialize)]
pub struct SyncApplied {
    /// How many remote changes were applied locally (always at least 1).
    pub received: usize,
}

/// Everything the command layer needs, managed by Tauri and shared by all
/// windows (single owner model: one store, one session manager).
pub struct AppState {
    pub store: Arc<Store>,
    pub sessions: Arc<SessionManager>,
    pub host_keys: Arc<HostKeyPrompts>,
    pub known_hosts_path: PathBuf,
    /// The cloud sync engine (off until the user signs in).
    pub sync: Arc<SyncEngine>,
    /// The background sync task.
    pub sync_scheduler: SyncScheduler,
}

impl AppState {
    pub fn new(store: Store, handle: AppHandle, data_dir: PathBuf) -> Self {
        let store = Arc::new(store);
        let sync = Arc::new(SyncEngine::new(store.clone()));
        // Both the background scheduler and `sync_now` go through
        // `SyncEngine::run_cycle`, so one hook covers every pull.
        let notifier = handle.clone();
        sync.set_applied_hook(Arc::new(move |received| {
            if let Err(error) = notifier.emit(SYNC_APPLIED_EVENT, SyncApplied { received }) {
                tracing::warn!(%error, "could not emit the sync-applied event");
            }
        }));
        // The background sync starts right away; it does nothing until the
        // user signs in.
        let sync_scheduler =
            crate::sync::scheduler::spawn(sync.clone(), crate::sync::scheduler::DEFAULT_INTERVAL);
        Self {
            known_hosts_path: data_dir.join("known_hosts"),
            sync,
            sync_scheduler,
            store,
            sessions: Arc::new(SessionManager::new()),
            host_keys: Arc::new(HostKeyPrompts::new(handle)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend listens for `sync-applied` and reads `received`.
    #[test]
    fn sync_applied_event_contract() {
        assert_eq!(SYNC_APPLIED_EVENT, "sync-applied");
        let json = serde_json::to_string(&SyncApplied { received: 3 }).unwrap();
        assert_eq!(json, r#"{"received":3}"#);
    }
}
