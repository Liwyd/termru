//! Host + identity CRUD. All validation runs in-app (`Host::validate`)
//! before any local write.

use tauri::State;
use termru_core::host::Host;
use termru_core::identity::Identity;
use termru_core::ids::{GroupId, HostId, IdentityId, TagId};
use termru_core::taxonomy::{Group, Tag};

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

/// A recently-used host as the frontend sees it: the host profile next to
/// the last connection time (ms since the epoch) for the "Last session"
/// column of the hosts table.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RecentHost {
    pub host: Host,
    pub last_used_at: i64,
}

/// Recently connected hosts (most recent first).
#[tauri::command]
pub fn recent_list(state: State<'_, AppState>) -> Result<Vec<RecentHost>, CommandError> {
    Ok(hosts(&state)
        .recents()
        .list()?
        .into_iter()
        .map(|entry| RecentHost {
            host: entry.host,
            last_used_at: entry.last_used_at,
        })
        .collect())
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

/// The folders (groups) of the hosts.
#[tauri::command]
pub fn group_list(state: State<'_, AppState>) -> Result<Vec<Group>, CommandError> {
    Ok(hosts(&state).groups().list()?)
}

#[tauri::command]
pub fn group_save(state: State<'_, AppState>, mut group: Group) -> Result<Group, CommandError> {
    group.validate()?;
    hosts(&state).groups().save(&mut group)?;
    Ok(group)
}

#[tauri::command]
pub fn group_delete(state: State<'_, AppState>, id: GroupId) -> Result<(), CommandError> {
    hosts(&state).groups().soft_delete(id)?;
    Ok(())
}

/// The tags of the hosts.
#[tauri::command]
pub fn tag_list(state: State<'_, AppState>) -> Result<Vec<Tag>, CommandError> {
    Ok(hosts(&state).tags().list()?)
}

#[tauri::command]
pub fn tag_save(state: State<'_, AppState>, mut tag: Tag) -> Result<Tag, CommandError> {
    tag.validate()?;
    hosts(&state).tags().save(&mut tag)?;
    Ok(tag)
}

#[tauri::command]
pub fn tag_delete(state: State<'_, AppState>, id: TagId) -> Result<(), CommandError> {
    hosts(&state).tags().soft_delete(id)?;
    Ok(())
}

/// The device-local key bindings: which key file each identity uses on this
/// machine. The paths never leave the device (they are not part of the sync
/// payload), but the user has to see them to know which key an identity
/// refers to.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBinding {
    pub identity_id: IdentityId,
    pub key_path: String,
}

#[tauri::command]
pub fn identity_bindings(state: State<'_, AppState>) -> Result<Vec<IdentityBinding>, CommandError> {
    let bindings = hosts(&state).identities().bindings()?;
    let mut out: Vec<IdentityBinding> = bindings
        .into_iter()
        .map(|(identity_id, key_path)| IdentityBinding {
            identity_id,
            key_path,
        })
        .collect();
    out.sort_by_key(|binding| binding.identity_id.to_string());
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_host_wire_shape_carries_the_last_used_at_next_to_the_host() {
        let host = Host::new("web".into(), "web.example.com".into(), "deploy".into());
        let dto = RecentHost {
            host,
            last_used_at: 1_760_000_000_000,
        };
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["last_used_at"], 1_760_000_000_000_i64);
        assert_eq!(json["host"]["name"], "web");
        assert_eq!(json["host"]["address"], "web.example.com");
    }
}
