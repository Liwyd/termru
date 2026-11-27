//! Secret management commands (OS credential store).
//!
//! The frontend never keeps a secret longer than needed: it hands the value
//! to the backend, which either uses it for the connection or stores it in
//! the platform credential store.
//!
//! The commands address a **host**. A password is stored for the host; a key
//! passphrase is stored for the host's *identity* (see `secrets.rs`), so the
//! frontend does not need to know which identity a host uses.
//!
//! The credential store (D-Bus, Keychain, Credential Manager) and SQLite can
//! block for a long time, so every command runs them on the blocking pool.

use tauri::State;
use termru_core::host::AuthMethod;
use termru_core::ids::{HostId, IdentityId};
use termru_store::Store;

use crate::app_error::CommandError;
use crate::secrets::{SecretKind, SecretStore};
use crate::state::AppState;

/// What a secret of a host is stored under.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// The host's own password.
    Password { host: String },
    /// The key passphrase of the host's identity. `host` is only used to
    /// find an entry an older version stored per host.
    Passphrase { identity: String, host: String },
}

/// The identity a host authenticates with, for a passphrase.
fn identity_of(store: &Store, host_id: HostId) -> Result<IdentityId, CommandError> {
    let host = store
        .repos()
        .hosts()
        .get(host_id)?
        .ok_or_else(|| CommandError::not_found("host", &host_id.to_string()))?;
    match host.auth {
        AuthMethod::PrivateKey { identity_id } => Ok(identity_id),
        _ => Err(CommandError::new(
            "this host does not authenticate with a key file, so it has no key passphrase",
        )),
    }
}

fn target(store: &Store, host_id: HostId, kind: &str) -> Result<Target, CommandError> {
    let kind = SecretKind::parse(kind)
        .ok_or_else(|| CommandError::new(format!("unknown secret kind '{kind}'")))?;
    Ok(match kind {
        SecretKind::Password => Target::Password {
            host: host_id.to_string(),
        },
        SecretKind::Passphrase => Target::Passphrase {
            identity: identity_of(store, host_id)?.to_string(),
            host: host_id.to_string(),
        },
    })
}

pub(super) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError::new(format!("the credential store call failed: {error}")))?
}

/// Stores a secret for a host in the OS credential store.
#[tauri::command]
pub async fn secret_set(
    state: State<'_, AppState>,
    host_id: HostId,
    kind: String,
    value: String,
) -> Result<(), CommandError> {
    if value.is_empty() {
        return Err(CommandError::new("refusing to store an empty secret"));
    }
    let store = state.store.clone();
    blocking(move || match target(&store, host_id, &kind)? {
        Target::Password { host } => SecretStore::set(&host, SecretKind::Password, &value),
        Target::Passphrase { identity, host } => {
            SecretStore::set_passphrase(&identity, &host, &value)
        }
    })
    .await
}

/// Removes a stored secret.
#[tauri::command]
pub async fn secret_delete(
    state: State<'_, AppState>,
    host_id: HostId,
    kind: String,
) -> Result<(), CommandError> {
    let store = state.store.clone();
    blocking(move || match target(&store, host_id, &kind)? {
        Target::Password { host } => SecretStore::delete(&host, SecretKind::Password),
        Target::Passphrase { identity, host } => SecretStore::delete_passphrase(&identity, &host),
    })
    .await
}

/// Whether a secret is stored for a host.
#[tauri::command]
pub async fn secret_has(
    state: State<'_, AppState>,
    host_id: HostId,
    kind: String,
) -> Result<bool, CommandError> {
    let store = state.store.clone();
    blocking(move || {
        Ok(match target(&store, host_id, &kind)? {
            Target::Password { host } => SecretStore::has(&host, SecretKind::Password),
            Target::Passphrase { identity, host } => SecretStore::has_passphrase(&identity, &host),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use termru_core::host::Host;
    use termru_core::identity::Identity;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&dir.path().join("secrets.sqlite")).expect("store");
        (dir, store)
    }

    fn saved_host(store: &Store, auth: AuthMethod) -> Host {
        let mut host = Host::new("web".into(), "web.test".into(), "deploy".into());
        host.auth = auth;
        store.repos().hosts().save(&mut host).expect("save host");
        host
    }

    /// Two hosts that use the same identity share one passphrase entry.
    #[test]
    fn a_passphrase_is_addressed_by_the_hosts_identity() {
        let (_dir, store) = temp_store();
        let mut identity = Identity::new("work key".into());
        store
            .repos()
            .identities()
            .save(&mut identity)
            .expect("save identity");
        let first = saved_host(
            &store,
            AuthMethod::PrivateKey {
                identity_id: identity.id,
            },
        );
        let second = saved_host(
            &store,
            AuthMethod::PrivateKey {
                identity_id: identity.id,
            },
        );

        let a = target(&store, first.id, "passphrase").expect("first");
        let b = target(&store, second.id, "passphrase").expect("second");
        match (a, b) {
            (
                Target::Passphrase {
                    identity: ia,
                    host: ha,
                },
                Target::Passphrase {
                    identity: ib,
                    host: hb,
                },
            ) => {
                assert_eq!(ia, identity.id.to_string());
                assert_eq!(ia, ib, "one identity, one entry");
                assert_ne!(ha, hb, "the host id only serves the migration fallback");
            }
            other => panic!("expected passphrase targets, got {other:?}"),
        }
    }

    #[test]
    fn a_password_stays_with_its_host() {
        let (_dir, store) = temp_store();
        let host = saved_host(&store, AuthMethod::Password);
        assert_eq!(
            target(&store, host.id, "password").expect("target"),
            Target::Password {
                host: host.id.to_string()
            }
        );
    }

    #[test]
    fn a_host_without_a_key_has_no_passphrase() {
        let (_dir, store) = temp_store();
        let host = saved_host(&store, AuthMethod::Agent);
        let error = target(&store, host.id, "passphrase").expect_err("no identity");
        assert!(error.message.contains("key file"), "{}", error.message);
        let error = target(&store, host.id, "nonsense").expect_err("unknown kind");
        assert!(
            error.message.contains("unknown secret kind"),
            "{}",
            error.message
        );
        // A passphrase needs the host's identity, so the host has to exist
        // (a password is stored under the host id without looking at it).
        let error = target(&store, HostId::new(), "passphrase").expect_err("unknown host");
        assert!(error.message.contains("no host"), "{}", error.message);
    }
}
