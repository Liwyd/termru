//! Session commands: open/retry/duplicate/close/write/resize.
//!
//! `session_open` returns immediately with a session id; the connection runs
//! in a background task and reports progress on the per-session
//! [`Channel`]. Secrets arrive here from the connection dialog, are used to
//! build `AuthChoice`, and are only retained in memory (inside the live
//! session's `ConnectParams`, zeroized on drop).

use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;
use tauri::ipc::Channel;
use termru_core::host::{AuthMethod, Host};
use termru_core::ids::HostId;
use termru_ssh::{AuthChoice, ConnectParams, ConnectTimeouts, SecretValue, SessionOptions};

use crate::app_error::CommandError;
use crate::state::sessions::spawn_connection;
use crate::state::{AppState, SessionInfo, SessionUpdate};

const MAX_DIMENSION: u16 = 500;

#[derive(Clone, Default, Deserialize)]
pub struct SessionSecrets {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

impl std::fmt::Debug for SessionSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print secret material, even in logs.
        f.debug_struct("SessionSecrets")
            .field("password", &self.password.as_ref().map(|_| "[redacted]"))
            .field(
                "passphrase",
                &self.passphrase.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

#[derive(Clone, Deserialize)]
pub struct SessionGeometry {
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

impl std::fmt::Debug for SessionGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionGeometry")
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .finish()
    }
}

fn default_cols() -> u16 {
    80
}

fn default_rows() -> u16 {
    24
}

#[derive(Clone, Deserialize)]
pub struct SessionOpenOptions {
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
    #[serde(default)]
    pub secrets: SessionSecrets,
}

impl std::fmt::Debug for SessionOpenOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionOpenOptions")
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("secrets", &self.secrets)
            .finish()
    }
}

fn validate_geometry(cols: u16, rows: u16) -> Result<(), CommandError> {
    if !(2..=MAX_DIMENSION).contains(&cols) || !(1..=MAX_DIMENSION).contains(&rows) {
        return Err(CommandError::new(format!(
            "invalid terminal size {cols}x{rows} (allowed: 2-500 columns, 1-500 rows)"
        )));
    }
    Ok(())
}

/// Resolves the host's auth method into a concrete [`AuthChoice`] using the
/// per-connection secrets supplied by the UI (never persisted).
pub(crate) fn build_auth(
    host: &Host,
    store: &termru_store::Store,
    secrets: &SessionSecrets,
) -> Result<AuthChoice, CommandError> {
    match &host.auth {
        AuthMethod::Password => {
            let password = secrets.password.clone().ok_or_else(|| {
                CommandError::new(format!("host \"{}\" requires a password", host.name))
            })?;
            Ok(AuthChoice::Password {
                password: SecretValue::new(password),
            })
        }
        AuthMethod::PrivateKey { identity_id } => {
            let repos = store.repos();
            let identity = repos
                .identities()
                .get(*identity_id)?
                .ok_or_else(|| CommandError::new("identity no longer exists"))?;
            let key_path = repos.identities().binding(*identity_id)?.ok_or_else(|| {
                CommandError::new(format!(
                    "identity \"{}\" has no private key bound on this device",
                    identity.name
                ))
            })?;
            Ok(AuthChoice::PrivateKey {
                key_path: PathBuf::from(key_path),
                passphrase: secrets.passphrase.clone().map(SecretValue::new),
            })
        }
        AuthMethod::Agent => Ok(AuthChoice::Agent {
            socket: std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from),
        }),
    }
}

fn build_params(state: &AppState, host: &Host, auth: AuthChoice) -> ConnectParams {
    ConnectParams {
        host: host.address.clone(),
        port: host.port,
        username: host.username.clone(),
        auth,
        term: host.term.clone(),
        keepalive_seconds: host.keepalive_seconds,
        timeouts: ConnectTimeouts::default(),
        known_hosts_path: state.known_hosts_path.clone(),
        host_key_prompt: state.host_keys.clone(),
    }
}

/// Registers the session and spawns its connection task.
fn start_session(
    state: &AppState,
    host_id: HostId,
    params: ConnectParams,
    cols: u16,
    rows: u16,
    channel: Channel<SessionUpdate>,
) -> String {
    let options = SessionOptions {
        cols,
        rows,
        ..SessionOptions::default()
    };
    let (id, cancel_rx) = state.sessions.register(host_id, params.clone());
    spawn_connection(
        state.sessions.clone(),
        state.store.clone(),
        id.clone(),
        params,
        options,
        cancel_rx,
        channel,
    );
    id
}

#[tauri::command]
pub async fn session_open(
    state: State<'_, AppState>,
    host_id: HostId,
    options: SessionOpenOptions,
    channel: Channel<SessionUpdate>,
) -> Result<String, CommandError> {
    validate_geometry(options.cols, options.rows)?;
    let (host, auth) = {
        let store = &*state.store;
        let host = store
            .repos()
            .hosts()
            .get(host_id)?
            .ok_or_else(|| CommandError::not_found("host", &host_id.to_string()))?;
        if host.deleted {
            return Err(CommandError::new("host has been deleted"));
        }
        let auth = build_auth(&host, store, &options.secrets)?;
        (host, auth)
    };
    let params = build_params(&state, &host, auth);
    Ok(start_session(
        &state,
        host_id,
        params,
        options.cols,
        options.rows,
        channel,
    ))
}

#[tauri::command]
pub async fn session_retry(
    state: State<'_, AppState>,
    id: String,
    geometry: SessionGeometry,
    channel: Channel<SessionUpdate>,
) -> Result<(), CommandError> {
    validate_geometry(geometry.cols, geometry.rows)?;
    let (params, cancel_rx) = state.sessions.retry_handle(&id)?;
    state
        .sessions
        .set_status(&id, crate::state::SessionStatusDto::Connecting);
    let options = SessionOptions {
        cols: geometry.cols,
        rows: geometry.rows,
        ..SessionOptions::default()
    };
    spawn_connection(
        state.sessions.clone(),
        state.store.clone(),
        id,
        params,
        options,
        cancel_rx,
        channel,
    );
    Ok(())
}

#[tauri::command]
pub async fn session_duplicate(
    state: State<'_, AppState>,
    id: String,
    geometry: SessionGeometry,
    channel: Channel<SessionUpdate>,
) -> Result<String, CommandError> {
    validate_geometry(geometry.cols, geometry.rows)?;
    let info = state
        .sessions
        .info(&id)
        .ok_or_else(|| CommandError::not_found("session", &id))?;
    let params = state
        .sessions
        .params(&id)
        .ok_or_else(|| CommandError::not_found("session", &id))?;
    Ok(start_session(
        &state,
        info.host_id,
        params,
        geometry.cols,
        geometry.rows,
        channel,
    ))
}

#[tauri::command]
pub async fn session_close(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    state.sessions.close(&id).await
}

#[tauri::command]
pub async fn session_write(
    state: State<'_, AppState>,
    id: String,
    data: Vec<u8>,
) -> Result<(), CommandError> {
    state.sessions.write(&id, data).await
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), CommandError> {
    validate_geometry(cols, rows)?;
    state.sessions.resize(&id, cols, rows).await
}

#[tauri::command]
pub fn session_list(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, CommandError> {
    Ok(state.sessions.list())
}

/// Answers a pending host-key trust prompt (rejects unknown ids).
#[tauri::command]
pub fn host_key_prompt_resolve(
    state: State<'_, AppState>,
    id: String,
    trust: bool,
) -> Result<bool, CommandError> {
    Ok(state.host_keys.resolve(&id, trust))
}

#[cfg(test)]
mod tests {
    use super::*;
    use termru_core::identity::Identity;
    use termru_core::ids::IdentityId;

    fn temp_store() -> (tempfile::TempDir, termru_store::Store) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = termru_store::Store::open(&dir.path().join("test.sqlite")).expect("store");
        (dir, store)
    }

    fn host_with(auth: AuthMethod) -> Host {
        let mut host = Host::new("web-1".into(), "web-1.test".into(), "deploy".into());
        host.auth = auth;
        host
    }

    #[test]
    fn geometry_bounds_are_enforced() {
        assert!(validate_geometry(80, 24).is_ok());
        assert!(validate_geometry(1, 24).is_err());
        assert!(validate_geometry(80, 0).is_err());
        assert!(validate_geometry(501, 24).is_err());
        assert!(validate_geometry(80, 501).is_err());
        let message = validate_geometry(0, 0).unwrap_err().message;
        assert!(message.contains("invalid terminal size"), "{message}");
    }

    #[test]
    fn password_auth_requires_a_supplied_password() {
        let (_dir, store) = temp_store();
        let host = host_with(AuthMethod::Password);
        let missing = build_auth(&host, &store, &SessionSecrets::default());
        let error = missing.expect_err("password must be required");
        assert!(error.message.contains("requires a password"));
        assert!(!error.message.contains("secret"));

        let supplied = build_auth(
            &host,
            &store,
            &SessionSecrets {
                password: Some("hunter2".into()),
                passphrase: None,
            },
        )
        .expect("auth builds");
        match supplied {
            AuthChoice::Password { password } => assert_eq!(password.expose(), "hunter2"),
            other => panic!("wrong variant: {}", other.method_name()),
        }
    }

    #[test]
    fn agent_auth_never_panics_without_ssh_auth_sock() {
        let (_dir, store) = temp_store();
        let host = host_with(AuthMethod::Agent);
        let choice = build_auth(&host, &store, &SessionSecrets::default()).expect("agent builds");
        assert!(matches!(choice, AuthChoice::Agent { .. }));
    }

    #[test]
    fn private_key_auth_requires_a_bound_key_and_identity() {
        let (_dir, store) = temp_store();
        let missing_identity = host_with(AuthMethod::PrivateKey {
            identity_id: IdentityId::new(),
        });
        let error = build_auth(&missing_identity, &store, &SessionSecrets::default())
            .expect_err("identity must exist");
        assert!(
            error.message.contains("no longer exists"),
            "{}",
            error.message
        );

        let mut identity = Identity::new("default key".into());
        store
            .repos()
            .identities()
            .save(&mut identity)
            .expect("save identity");
        let unbound = host_with(AuthMethod::PrivateKey {
            identity_id: identity.id,
        });
        let error =
            build_auth(&unbound, &store, &SessionSecrets::default()).expect_err("must be bound");
        assert!(
            error.message.contains("no private key bound"),
            "{}",
            error.message
        );

        store
            .repos()
            .identities()
            .bind(identity.id, "/home/user/.ssh/id_ed25519")
            .expect("bind");
        let choice = build_auth(
            &unbound,
            &store,
            &SessionSecrets {
                password: None,
                passphrase: Some("open-sesame".into()),
            },
        )
        .expect("auth builds");
        match choice {
            AuthChoice::PrivateKey {
                key_path,
                passphrase,
            } => {
                assert_eq!(key_path, PathBuf::from("/home/user/.ssh/id_ed25519"));
                assert_eq!(passphrase.expect("passphrase").expose(), "open-sesame");
            }
            other => panic!("wrong variant: {}", other.method_name()),
        }
    }

    #[test]
    fn secrets_never_serialize_into_debug_output() {
        let secrets = SessionSecrets {
            password: Some("top-secret".into()),
            passphrase: Some("also-secret".into()),
        };
        let debug = format!("{secrets:?}");
        assert!(!debug.contains("top-secret"), "{debug}");
        assert!(!debug.contains("also-secret"), "{debug}");
    }
}
