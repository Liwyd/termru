//! Host + identity CRUD. All validation runs in-app (`Host::validate`)
//! before any local write.

use tauri::State;
use termru_core::host::Host;
use termru_core::ids::{HostId, IdentityId};
use termru_core::identity::Identity;

use crate::app_error::CommandError;
use crate::state::AppState;

fn hosts(state: &AppState) -> termru_store::Repos<'_> {
    state.store.repos()
}

#[tauri::command]
pub fn host_list(state: State<'_, AppState>) -> Result<Vec<Host>, CommandError> {
    Ok(hosts(&state).hosts().list()?)
}

#[tauri::command]
pub fn host_save(state: State<'_, AppState>, mut host: Host) -> Result<Host, CommandError> {
    host.validate()?;
    hosts(&state).hosts().save(&mut host)?;
    Ok(host)
}

#[tauri::command]
pub fn host_delete(state: State<'_, AppState>, id: HostId) -> Result<(), CommandError> {
    hosts(&state).hosts().soft_delete(id)?;
    Ok(())
}

#[tauri::command]
pub fn host_set_favorite(
    state: State<'_, AppState>,
    id: HostId,
    favorite: bool,
) -> Result<(), CommandError> {
    hosts(&state).hosts().set_favorite(id, favorite)?;
    Ok(())
}

#[tauri::command]
pub fn identity_list(state: State<'_, AppState>) -> Result<Vec<Identity>, CommandError> {
    Ok(hosts(&state).identities().list()?)
}

#[tauri::command]
pub fn identity_save(
    state: State<'_, AppState>,
    mut identity: Identity,
) -> Result<Identity, CommandError> {
    identity.validate()?;
    hosts(&state).identities().save(&mut identity)?;
    Ok(identity)
}

#[tauri::command]
pub fn identity_delete(state: State<'_, AppState>, id: IdentityId) -> Result<(), CommandError> {
    hosts(&state).identities().soft_delete(id)?;
    Ok(())
}

/// Binds an identity to a device-local private key path (the key file is
/// never copied into the database).
#[tauri::command]
pub fn identity_bind(
    state: State<'_, AppState>,
    id: IdentityId,
    key_path: String,
) -> Result<(), CommandError> {
    let trimmed = key_path.trim();
    if trimmed.is_empty() {
        return Err(CommandError::new("key path must not be empty"));
    }
    if !std::path::Path::new(trimmed).is_absolute() {
        return Err(CommandError::new("key path must be absolute"));
    }
    hosts(&state).identities().bind(id, trimmed)?;
    Ok(())
}

#[tauri::command]
pub fn identity_unbind(state: State<'_, AppState>, id: IdentityId) -> Result<(), CommandError> {
    hosts(&state).identities().unbind(id)?;
    Ok(())
}
