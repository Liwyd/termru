//! Secret management commands (OS credential store).
//!
//! The frontend never keeps a secret longer than needed: it hands the value
//! to the backend, which either uses it for the connection or stores it in
//! the platform credential store.

use tauri::State;
use termru_core::ids::HostId;

use crate::app_error::CommandError;
use crate::secrets::{SecretKind, SecretStore};
use crate::state::AppState;

/// Stores a secret for a host in the OS credential store.
#[tauri::command]
pub fn secret_set(
    state: State<'_, AppState>,
    host_id: HostId,
    kind: String,
    value: String,
) -> Result<(), CommandError> {
    let _ = &state;
    let kind = SecretKind::parse(&kind)
        .ok_or_else(|| CommandError::new(format!("unknown secret kind '{kind}'")))?;
    if value.is_empty() {
        return Err(CommandError::new("refusing to store an empty secret"));
    }
    SecretStore::set(&host_id.to_string(), kind, &value)
}

/// Removes a stored secret.
#[tauri::command]
pub fn secret_delete(
    state: State<'_, AppState>,
    host_id: HostId,
    kind: String,
) -> Result<(), CommandError> {
    let _ = &state;
    let kind = SecretKind::parse(&kind)
        .ok_or_else(|| CommandError::new(format!("unknown secret kind '{kind}'")))?;
    SecretStore::delete(&host_id.to_string(), kind)
}

/// Whether a secret is stored for a host.
#[tauri::command]
pub fn secret_has(
    state: State<'_, AppState>,
    host_id: HostId,
    kind: String,
) -> Result<bool, CommandError> {
    let _ = &state;
    let kind = SecretKind::parse(&kind)
        .ok_or_else(|| CommandError::new(format!("unknown secret kind '{kind}'")))?;
    Ok(SecretStore::has(&host_id.to_string(), kind))
}
