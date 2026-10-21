mod host_keys;
pub(crate) mod sessions;

pub use host_keys::HostKeyPrompts;
pub use sessions::{SessionInfo, SessionManager, SessionStatusDto, SessionUpdate};

use std::path::PathBuf;
use std::sync::Arc;

use tauri::AppHandle;
use termru_store::Store;

use crate::sync::SyncEngine;

/// Everything the command layer needs, managed by Tauri and shared by all
/// windows (single owner model: one store, one session manager).
pub struct AppState {
    pub store: Arc<Store>,
    pub sessions: Arc<SessionManager>,
    pub host_keys: Arc<HostKeyPrompts>,
    pub known_hosts_path: PathBuf,
    /// The cloud sync engine (off until the user signs in).
    pub sync: Arc<SyncEngine>,
}

impl AppState {
    pub fn new(store: Store, handle: AppHandle, data_dir: PathBuf) -> Self {
        let store = Arc::new(store);
        Self {
            known_hosts_path: data_dir.join("known_hosts"),
            sync: Arc::new(SyncEngine::new(store.clone())),
            store,
            sessions: Arc::new(SessionManager::new()),
            host_keys: Arc::new(HostKeyPrompts::new(handle)),
        }
    }
}
