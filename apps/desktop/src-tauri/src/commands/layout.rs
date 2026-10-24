//! Session-layout persistence.
//!
//! The desktop restores the *layout* (which hosts were open, in which order)
//! after a restart. Live SSH sessions are never claimed to survive: a
//! restored tab is a placeholder the user reconnects on demand (secrets are
//! not stored, so an automatic reconnect is impossible by design).

use tauri::State;
use termru_core::ids::WorkspaceId;
use termru_core::workspace::WorkspaceDoc;

use crate::app_error::CommandError;
use crate::state::AppState;

/// Fixed id of the workspace document that holds the current session layout.
fn layout_workspace_id() -> WorkspaceId {
    WorkspaceId::from(uuid::Uuid::from_u128(
        0x7a5f_1c34_2b6e_4f18_9a2d_0c5f_7b1e_4d90,
    ))
}

const LAYOUT_NAME: &str = "session layout";

/// Stores the layout document (validated before writing).
pub fn save_layout(store: &termru_store::Store, doc: WorkspaceDoc) -> Result<(), CommandError> {
    doc.validate()?;
    store
        .repos()
        .workspaces()
        .save(layout_workspace_id(), LAYOUT_NAME, &doc)?;
    Ok(())
}

/// Loads the stored layout document, if any.
pub fn load_layout(store: &termru_store::Store) -> Result<Option<WorkspaceDoc>, CommandError> {
    Ok(store.repos().workspaces().get(layout_workspace_id())?)
}

#[tauri::command]
pub fn layout_save(state: State<'_, AppState>, doc: WorkspaceDoc) -> Result<(), CommandError> {
    save_layout(&state.store, doc)
}

#[tauri::command]
pub fn layout_load(state: State<'_, AppState>) -> Result<Option<WorkspaceDoc>, CommandError> {
    load_layout(&state.store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use termru_core::ids::{HostId, PaneId, TabId, WindowId};
    use termru_core::workspace::{PaneNode, TabLayout, WindowLayout};

    fn temp_store() -> (tempfile::TempDir, termru_store::Store) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = termru_store::Store::open(&dir.path().join("layout.sqlite")).expect("store");
        (dir, store)
    }

    fn doc_with_host() -> WorkspaceDoc {
        WorkspaceDoc {
            version: termru_core::workspace::WORKSPACE_DOC_VERSION,
            windows: vec![WindowLayout {
                id: WindowId::new(),
                tabs: vec![TabLayout {
                    id: TabId::new(),
                    title: Some("web-1".to_string()),
                    root: PaneNode::Leaf {
                        pane: PaneId::new(),
                        host_id: Some(HostId::new()),
                    },
                    active_pane: None,
                }],
                active_tab: None,
                bounds: None,
            }],
            active_window: None,
            last_route: None,
        }
    }

    #[test]
    fn layout_round_trips_through_the_store() {
        let (_dir, store) = temp_store();
        assert!(load_layout(&store).expect("load").is_none());

        let doc = doc_with_host();
        save_layout(&store, doc.clone()).expect("save");
        let loaded = load_layout(&store)
            .expect("load")
            .expect("document present");
        assert_eq!(loaded, doc);
    }

    #[test]
    fn invalid_layout_is_rejected_before_writing() {
        let (_dir, store) = temp_store();
        let mut doc = doc_with_host();
        doc.version = 0; // invalid version
        let error = save_layout(&store, doc).expect_err("must be rejected");
        assert!(error.message.contains("version"), "{}", error.message);
        assert!(load_layout(&store).expect("load").is_none());
    }
}
