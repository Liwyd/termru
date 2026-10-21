//! Commands of the sync feature.
//!
//! The frontend never sees a token: it asks the backend to sign in, and the
//! backend keeps the tokens in the OS credential store.

use tauri::State;

use crate::app_error::CommandError;
use crate::state::AppState;
use crate::sync::client::SyncError;
use crate::sync::{ConflictView, SyncOutcome, SyncStatus};

/// The current state of the sync feature.
#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> Result<SyncStatus, CommandError> {
    Ok(state.sync.status()?)
}

/// Signs in to the sync server and stores the tokens in the credential store.
#[tauri::command]
pub async fn sync_sign_in(
    state: State<'_, AppState>,
    server_url: String,
    email: String,
    password: String,
    device_name: String,
) -> Result<SyncStatus, CommandError> {
    state
        .sync
        .sign_in(&server_url, &email, &password, &device_name)
        .await?;
    Ok(state.sync.status()?)
}

/// Forgets the tokens and the server configuration.
#[tauri::command]
pub fn sync_sign_out(state: State<'_, AppState>) -> Result<(), CommandError> {
    state.sync.sign_out()
}

/// Runs one sync cycle.
#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<SyncOutcome, CommandError> {
    match state.sync.run_cycle().await {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            // Keep the failure for the UI, but report it to the user too.
            state.sync.record_failure(&error.to_string());
            Err(error.into())
        }
    }
}

/// The open conflicts.
#[tauri::command]
pub fn sync_conflicts(state: State<'_, AppState>) -> Result<Vec<ConflictView>, CommandError> {
    Ok(state.sync.conflicts()?)
}

/// Resolves a conflict in favour of the local or the remote copy.
#[tauri::command]
pub fn sync_resolve_conflict(
    state: State<'_, AppState>,
    record_id: String,
    keep_local: bool,
) -> Result<(), CommandError> {
    state.sync.resolve_conflict(&record_id, keep_local)?;
    Ok(())
}

impl From<SyncError> for CommandError {
    fn from(value: SyncError) -> Self {
        CommandError::new(value.to_string())
    }
}
