//! Interactive SSH session: connection setup, authentication, PTY shell,
//! keepalives, and the attach/replay data protocol.
//!
//! ## Concurrency model
//!
//! One tokio task (the *actor*) owns the russh handle and channel. All
//! commands (write/resize/attach/close) and all SSH messages funnel through
//! it, which makes ordering trivially correct: an attaching terminal always
//! receives the replay buffer before any live bytes.
//!
//! Output flows to at most one attached consumer (the pane currently showing
//! this session) through a bounded channel with a send timeout. If the UI
//! stalls beyond the timeout, the session **detaches** and keeps running —
//! data continues into a bounded replay buffer instead of blocking the SSH
//! transport (flow control remains intact because reads stop being polled and
//! the TCP window fills naturally).
//!
//! ## Dead-connection detection
//!
//! russh's built-in keepalive machinery is configured: if nothing is received
//! for `keepalive_seconds`, a keepalive is sent; after
//! [`DEFAULT_KEEPALIVE_MAX`] unanswered keepalives the connection is closed
//! and the session reports a network failure (which the app may reconnect).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh::client::{self, Config as ClientConfig, Handle, Handler, Msg};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::config::{AuthChoice, ConnectParams, HostKeyPromptRequest};
use crate::error::SshError;
use crate::known_hosts::{Evaluation, KnownHosts};
use crate::reconnect::ReconnectPolicy;

pub const DEFAULT_KEEPALIVE_MAX: usize = 3;
const REPLAY_CHUNK: usize = 32 * 1024;
const EVENT_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_QUEUE: usize = 128;
const MAX_REPLAY_DEFAULT: usize = 512 * 1024;

/// Events flowing from the session to its consumer.
#[derive(Debug)]
pub enum SessionEvent {
    Status(SessionStatus),
    /// Raw bytes from the remote shell (stdout and stderr merged by SSH).
    Data(Vec<u8>),
    /// The remote process finished (before the channel closed).
    Exit {
        code: Option<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Connecting,
    Authenticating,
    Ready,
    /// A retriable failure occurred and a bounded reconnect is scheduled.
    Reconnecting {
        /// 1-based number of the attempt that just failed.
        attempt: u32,
        /// Backoff before the next attempt (jitter already applied).
        delay: Duration,
        /// Message of the failure that triggered the reconnect.
        last_error: String,
    },
    Closed {
        reason: String,
    },
    Failed {
        category: ErrorCategory,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Auth,
    HostKey,
    Network,
    Timeout,
    Protocol,
    Local,
}

impl ErrorCategory {
    pub fn of(error: &SshError) -> Self {
        match error {
            SshError::AuthFailed { .. }
            | SshError::AuthTimeout
            | SshError::AgentUnavailable
            | SshError::AgentNoKey
            | SshError::KeyLoadFailed { .. }
            | SshError::PassphraseRequired
            | SshError::WrongPassphrase { .. } => Self::Auth,
            SshError::HostKeyUnknown { .. }
            | SshError::HostKeyChanged { .. }
            | SshError::HostKeyRevoked { .. }
            | SshError::HostKeyRejected
            | SshError::HostKeyPromptTimeout => Self::HostKey,
            SshError::ConnectTimeout { .. } => Self::Timeout,
            SshError::DnsFailure { .. }
            | SshError::ConnectFailed { .. }
            | SshError::ConnectionClosed
            | SshError::Interrupted => Self::Network,
            SshError::Io(_)
            | SshError::NoPty
            | SshError::SessionClosing
            | SshError::ShellExited { .. } => Self::Local,
            SshError::Protocol(_) => Self::Protocol,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub cols: u16,
    pub rows: u16,
    /// Bounded replay buffer (bytes) kept for re-attaching consumers.
    pub replay_limit: usize,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            replay_limit: MAX_REPLAY_DEFAULT,
        }
    }
}

#[derive(Debug)]
enum SessionCommand {
    Write(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Attach(mpsc::Sender<SessionEvent>),
    Close,
}

/// Handle to a live session. Cheap to clone; all clones address the same
/// actor task.
#[derive(Debug, Clone)]
pub struct Session {
    cmd: mpsc::Sender<SessionCommand>,
}

impl Session {
    /// Opens a connection (no auto-reconnect, not cancellable mid-flight).
    pub async fn open(
        params: ConnectParams,
        options: SessionOptions,
        events: mpsc::Sender<SessionEvent>,
    ) -> Result<Session, SshError> {
        let (_cancel_tx, cancel) = watch::channel(false);
        Self::connect(
            params,
            options,
            events,
            ReconnectPolicy::no_retries(),
            cancel,
        )
        .await
    }

    /// Opens a connection that can be aborted mid-flight (dial, handshake,
    /// host-key prompt, authentication) by setting `cancel` to `true`.
    /// No auto-reconnect: one attempt, then the error surfaces.
    pub async fn open_cancellable(
        params: ConnectParams,
        options: SessionOptions,
        events: mpsc::Sender<SessionEvent>,
        cancel: watch::Receiver<bool>,
    ) -> Result<Session, SshError> {
        Self::connect(
            params,
            options,
            events,
            ReconnectPolicy::no_retries(),
            cancel,
        )
        .await
    }

    /// Like [`Session::open_cancellable`], but retries retriable failures
    /// under `policy` (bounded backoff; see [`ReconnectPolicy`]). Cancellation
    /// aborts both in-flight attempts and backoff waits.
    pub async fn open_with_reconnect(
        params: ConnectParams,
        options: SessionOptions,
        events: mpsc::Sender<SessionEvent>,
        policy: ReconnectPolicy,
        cancel: watch::Receiver<bool>,
    ) -> Result<Session, SshError> {
        Self::connect(params, options, events, policy, cancel).await
    }

    async fn connect(
        params: ConnectParams,
        options: SessionOptions,
        events: mpsc::Sender<SessionEvent>,
        policy: ReconnectPolicy,
        cancel: watch::Receiver<bool>,
    ) -> Result<Session, SshError> {
        let session = crate::reconnect::connect_with_retry(
            policy,
            cancel.clone(),
            &events,
            |attempt_cancel| {
                let params = params.clone();
                let options = options.clone();
                let events = events.clone();
                async move {
                    let mut attempt_cancel = attempt_cancel;
                    Session::connect_and_start(params, options, events, &mut attempt_cancel).await
                }
            },
        )
        .await?;
        Self::finish_open(session, &events, &cancel).await
    }

    /// Emits `Ready`, or aborts a connection that completed after cancellation.
    async fn finish_open(
        session: Session,
        events: &mpsc::Sender<SessionEvent>,
        cancel: &watch::Receiver<bool>,
    ) -> Result<Session, SshError> {
        if *cancel.borrow() {
            let _ = session.close().await;
            let _ = events
                .send(SessionEvent::Status(SessionStatus::Closed {
                    reason: "connection cancelled".to_string(),
                }))
                .await;
            return Err(SshError::SessionClosing);
        }
        let _ = events
            .send(SessionEvent::Status(SessionStatus::Ready))
            .await;
        Ok(session)
    }

    pub(crate) async fn connect_and_start(
        params: ConnectParams,
        options: SessionOptions,
        events: mpsc::Sender<SessionEvent>,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Session, SshError> {
        let _ = events
            .send(SessionEvent::Status(SessionStatus::Connecting))
            .await;

        let known_hosts = Arc::new(KnownHosts::with_path(params.known_hosts_path.clone()));
        let prompt_seen = Arc::new(AtomicBool::new(false));
        let recorded: Arc<std::sync::Mutex<Option<SshError>>> =
            Arc::new(std::sync::Mutex::new(None));

        // --- resolve + dial (explicit timeouts, precise DNS errors) ---
        let stream = dial(&params.host, params.port, params.timeouts.connect, cancel).await?;

        // --- handshake (key exchange incl. host-key decision) ---
        let handler = ClientHandler {
            host: params.host.clone(),
            port: params.port,
            known_hosts,
            prompt: params.host_key_prompt.clone(),
            decision_budget: params.timeouts.host_key_decision,
            prompt_seen: prompt_seen.clone(),
            recorded: recorded.clone(),
        };
        let mut config = ClientConfig {
            // `SshId::Standard` writes the value verbatim and appends `\r\n`;
            // it MUST include the `SSH-2.0-` prefix or OpenSSH desyncs the
            // banner exchange (caught by the sshd integration suite).
            client_id: russh::SshId::Standard(std::borrow::Cow::Owned(format!(
                "SSH-2.0-Termru_{}",
                env!("CARGO_PKG_VERSION")
            ))),
            keepalive_interval: (params.keepalive_seconds > 0)
                .then(|| Duration::from_secs(u64::from(params.keepalive_seconds))),
            keepalive_max: DEFAULT_KEEPALIVE_MAX,
            ..ClientConfig::default()
        };
        config.inactivity_timeout = None;

        let handle = connect_with_deadline(
            client::connect_stream(Arc::new(config), stream, handler),
            &params,
            prompt_seen,
            recorded,
            cancel,
        )
        .await?;

        // --- authentication ---
        let _ = events
            .send(SessionEvent::Status(SessionStatus::Authenticating))
            .await;
        let mut handle = handle;
        tokio::select! {
            result = tokio::time::timeout(params.timeouts.auth, authenticate(&mut handle, &params)) => {
                result.map_err(|_| SshError::AuthTimeout)??
            }
            _ = crate::reconnect::cancelled(cancel) => return Err(SshError::SessionClosing),
        }

        // --- interactive shell ---
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Protocol(format!("open session channel: {e}")))?;
        channel
            .request_pty(
                true,
                &params.term,
                u32::from(options.cols),
                u32::from(options.rows),
                0,
                0,
                &[],
            )
            .await
            .map_err(|e| SshError::Protocol(format!("terminal allocation failed: {e}")))?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| SshError::Protocol(format!("request shell: {e}")))?;

        let (cmd_tx, cmd_rx) = mpsc::channel(CONTROL_QUEUE);
        let actor = Actor {
            handle,
            channel,
            events: Some(events),
            replay: std::collections::VecDeque::new(),
            replay_limit: options.replay_limit,
            cmd_rx,
            keepalive: (params.keepalive_seconds > 0).then(|| {
                tokio::time::interval(Duration::from_secs(u64::from(params.keepalive_seconds)))
            }),
            exit_code: None,
            exit_emitted: false,
        };
        tokio::spawn(actor.run());

        Ok(Session { cmd: cmd_tx })
    }

    pub async fn write(&self, data: Vec<u8>) -> Result<(), SshError> {
        self.cmd
            .send(SessionCommand::Write(data))
            .await
            .map_err(|_| SshError::SessionClosing)
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), SshError> {
        self.cmd
            .send(SessionCommand::Resize { cols, rows })
            .await
            .map_err(|_| SshError::SessionClosing)
    }

    /// Binds the session's output to `consumer`. The previous consumer (if
    /// any) stops receiving; the new one first receives the replay buffer.
    pub async fn attach(&self, consumer: mpsc::Sender<SessionEvent>) -> Result<(), SshError> {
        self.cmd
            .send(SessionCommand::Attach(consumer))
            .await
            .map_err(|_| SshError::SessionClosing)
    }

    /// Graceful local close: EOF + channel close + disconnect.
    pub async fn close(&self) -> Result<(), SshError> {
        // The actor exits even if the command queue is full, so use a best
        // effort send: a closed queue already means the session is gone.
        let _ = self.cmd.send(SessionCommand::Close).await;
        Ok(())
    }
}

// ---------- dial ----------

async fn dial(
    host: &str,
    port: u16,
    budget: Duration,
    cancel: &mut watch::Receiver<bool>,
) -> Result<TcpStream, SshError> {
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| SshError::DnsFailure {
            host: host.to_string(),
            detail: e.to_string(),
        })?
        .collect();
    if addrs.is_empty() {
        return Err(SshError::ConnectFailed {
            host: host.to_string(),
            port,
            detail: "hostname resolved to no addresses".to_string(),
        });
    }
    let deadline = Instant::now() + budget;
    let mut last_io_error: Option<std::io::Error> = None;
    let mut saw_timeout = false;
    for addr in addrs {
        tokio::select! {
            result = tokio::time::timeout_at(deadline, TcpStream::connect(addr)) => {
                match result {
                    Ok(Ok(stream)) => return Ok(stream),
                    Ok(Err(e)) => last_io_error = Some(e),
                    Err(_) => saw_timeout = true,
                }
            }
            _ = crate::reconnect::cancelled(cancel) => return Err(SshError::SessionClosing),
        }
    }
    if let Some(err) = last_io_error {
        Err(SshError::ConnectFailed {
            host: host.to_string(),
            port,
            detail: err.to_string(),
        })
    } else if saw_timeout {
        Err(SshError::ConnectTimeout {
            host: host.to_string(),
            port,
            seconds: budget.as_secs(),
        })
    } else {
        Err(SshError::ConnectFailed {
            host: host.to_string(),
            port,
            detail: "no address accepted the connection".to_string(),
        })
    }
}

// ---------- handshake with adaptive deadline ----------

#[allow(clippy::too_many_arguments)]
async fn connect_with_deadline(
    future: impl std::future::Future<Output = Result<Handle<ClientHandler>, russh::Error>>,
    params: &ConnectParams,
    prompt_seen: Arc<AtomicBool>,
    recorded: Arc<std::sync::Mutex<Option<SshError>>>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Handle<ClientHandler>, SshError> {
    let interactive = params.host_key_prompt.is_interactive();
    let host = &params.host;
    let port = params.port;
    let timeouts = &params.timeouts;
    tokio::pin!(future);
    let mut deadline = Instant::now() + timeouts.connect;
    let mut extended = false;
    loop {
        tokio::select! {
            result = &mut future => {
                    return match result {
                    Ok(handle) => Ok(handle),
                    Err(err) => Err(take_recorded(&recorded).unwrap_or_else(|| {
                        map_connect_error(err, host, port, timeouts.connect.as_secs())
                    })),
                };
            }
            _ = crate::reconnect::cancelled(cancel) => {
                return Err(SshError::SessionClosing);
            }
            _ = tokio::time::sleep_until(deadline) => {
                if interactive && !extended && prompt_seen.load(Ordering::SeqCst) {
                    // A human may be looking at a trust dialog; grant the
                    // decision budget on top of the connect budget once.
                    extended = true;
                    deadline = Instant::now() + timeouts.host_key_decision;
                    continue;
                }
                return Err(
                    take_recorded(&recorded).unwrap_or(SshError::ConnectTimeout {
                        host: host.to_string(),
                        port,
                        seconds: timeouts.connect.as_secs(),
                    }),
                );
            }
        }
    }
}

fn take_recorded(recorded: &Arc<std::sync::Mutex<Option<SshError>>>) -> Option<SshError> {
    recorded.lock().ok().and_then(|mut g| g.take())
}

fn map_connect_error(err: russh::Error, host: &str, port: u16, seconds: u64) -> SshError {
    match &err {
        russh::Error::IO(io) => SshError::ConnectFailed {
            host: host.to_string(),
            port,
            detail: io.to_string(),
        },
        russh::Error::ConnectionTimeout | russh::Error::Elapsed(_) => SshError::ConnectTimeout {
            host: host.to_string(),
            port,
            seconds,
        },
        _ => SshError::Protocol(format!("connection failed: {err}")),
    }
}

// ---------- authentication ----------

async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    params: &ConnectParams,
) -> Result<(), SshError> {
    let username = params.username.clone();
    match &params.auth {
        AuthChoice::Password { password } => {
            let result = handle
                .authenticate_password(username, password.expose())
                .await
                .map_err(|e| SshError::Protocol(format!("password auth failed: {e}")))?;
            if result.success() {
                return Ok(());
            }
            Err(auth_rejected("password", &result))
        }
        AuthChoice::PrivateKey {
            key_path,
            passphrase,
        } => {
            let key = load_key(key_path, passphrase.as_ref())?;
            // SHA-256 for RSA (legacy SHA-1 is disabled by modern servers);
            // russh ignores the hash algorithm for non-RSA keys.
            let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), Some(HashAlg::Sha256));
            let result = handle
                .authenticate_publickey(username, key_with_alg)
                .await
                .map_err(|e| SshError::Protocol(format!("publickey auth failed: {e}")))?;
            if result.success() {
                return Ok(());
            }
            Err(auth_rejected("publickey", &result))
        }
        AuthChoice::Agent { socket } => {
            let mut agent = connect_agent(socket.as_deref()).await?;
            let identities = agent
                .request_identities()
                .await
                .map_err(|e| SshError::Protocol(format!("agent query failed: {e}")))?;
            if identities.is_empty() {
                return Err(SshError::AgentNoKey);
            }
            let offered = identities.len();
            for identity in identities {
                let public = identity.public_key().into_owned();
                let attempt = handle
                    .authenticate_publickey_with(username.clone(), public, None, &mut agent)
                    .await;
                match attempt {
                    Ok(result) if result.success() => return Ok(()),
                    Ok(_) => continue,
                    Err(err) => {
                        return Err(SshError::Protocol(format!("agent signing failed: {err}")));
                    }
                }
            }
            Err(SshError::AuthFailed {
                method: "publickey (agent)",
                detail: format!("the server rejected all {offered} key(s) offered by the agent"),
            })
        }
    }
}

fn auth_rejected(method: &'static str, result: &russh::client::AuthResult) -> SshError {
    let detail = match result {
        russh::client::AuthResult::Failure {
            remaining_methods,
            partial_success,
        } => {
            if *partial_success {
                "the server accepted this step but requires additional authentication (multi-factor is not yet supported)"
                    .to_string()
            } else {
                format!(
                    "the server rejected the credentials (methods still offered: {remaining_methods:?})"
                )
            }
        }
        _ => "the server rejected the credentials".to_string(),
    };
    SshError::AuthFailed { method, detail }
}

fn load_key(
    path: &std::path::Path,
    passphrase: Option<&crate::config::SecretValue>,
) -> Result<russh::keys::PrivateKey, SshError> {
    let path_str = path.display().to_string();
    let passphrase = passphrase.map(|p| p.expose().to_string());
    match russh::keys::load_secret_key(path, passphrase.as_deref()) {
        Ok(key) => Ok(key),
        Err(err) => Err(classify_key_error(err, &path_str, passphrase.is_some())),
    }
}

fn classify_key_error(err: russh::keys::Error, path: &str, passphrase_given: bool) -> SshError {
    use russh::keys::Error as KeyError;
    if matches!(err, KeyError::KeyIsEncrypted) {
        return if passphrase_given {
            SshError::WrongPassphrase {
                path: path.to_string(),
            }
        } else {
            SshError::PassphraseRequired
        };
    }
    let detail = err.to_string();
    let lower = detail.to_lowercase();
    let looks_crypto = lower.contains("decrypt")
        || lower.contains("passphrase")
        || lower.contains("password")
        || lower.contains("crypt");
    if looks_crypto {
        if passphrase_given {
            SshError::WrongPassphrase {
                path: path.to_string(),
            }
        } else {
            SshError::PassphraseRequired
        }
    } else {
        SshError::KeyLoadFailed {
            path: path.to_string(),
            detail,
        }
    }
}

async fn connect_agent(
    socket: Option<&std::path::Path>,
) -> Result<AgentClient<Box<dyn AgentStream + Send + Unpin>>, SshError> {
    #[cfg(unix)]
    {
        let connected = match socket {
            Some(path) => AgentClient::connect_uds(path).await,
            None => AgentClient::connect_env().await,
        };
        match connected {
            Ok(client) => Ok(client.dynamic()),
            Err(_) => Err(SshError::AgentUnavailable),
        }
    }
    #[cfg(windows)]
    {
        // Windows: pageant has no socket path; an explicit socket is ignored.
        let _ = socket;
        match AgentClient::connect_pageant().await {
            Ok(client) => Ok(client.dynamic()),
            Err(_) => Err(SshError::AgentUnavailable),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = socket;
        Err(SshError::AgentUnavailable)
    }
}

// ---------- host-key verification handler ----------

struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    prompt: Arc<dyn crate::config::HostKeyPrompt>,
    decision_budget: Duration,
    prompt_seen: Arc<AtomicBool>,
    recorded: Arc<std::sync::Mutex<Option<SshError>>>,
}

impl ClientHandler {
    fn record(&self, error: SshError) {
        if let Ok(mut slot) = self.recorded.lock() {
            if slot.is_none() {
                *slot = Some(error);
            }
        }
    }
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public_key = match key {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => key,
            russh::keys::PublicKeyOrCertificate::Certificate(_) => {
                self.record(SshError::Protocol(
                    "OpenSSH host certificates are not supported in this version".to_string(),
                ));
                return Ok(false);
            }
        };

        let evaluation = match self.known_hosts.evaluate(&self.host, self.port, public_key) {
            Ok(evaluation) => evaluation,
            Err(error) => {
                self.record(error);
                return Ok(false);
            }
        };

        match evaluation {
            Evaluation::Known { .. } => Ok(true),
            Evaluation::Revoked { .. } => {
                self.record(SshError::HostKeyRevoked {
                    host: self.host.clone(),
                    port: self.port,
                });
                Ok(false)
            }
            Evaluation::Unknown {
                fingerprint,
                key_type,
            } => {
                self.prompt_seen.store(true, Ordering::SeqCst);
                let request = HostKeyPromptRequest::Unknown {
                    host: self.host.clone(),
                    port: self.port,
                    key: crate::known_hosts::HostKeyEntry {
                        key_type,
                        fingerprint: fingerprint.clone(),
                    },
                };
                let accepted = self.ask(request).await;
                if !accepted {
                    self.record(SshError::HostKeyRejected);
                    return Ok(false);
                }
                match self
                    .known_hosts
                    .trust(&self.host, self.port, public_key, false)
                {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        self.record(error);
                        Ok(false)
                    }
                }
            }
            Evaluation::Changed { old, new } => {
                self.prompt_seen.store(true, Ordering::SeqCst);
                let request = HostKeyPromptRequest::Changed {
                    host: self.host.clone(),
                    port: self.port,
                    old: old.clone(),
                    new: new.clone(),
                };
                let accepted = self.ask(request).await;
                if !accepted {
                    self.record(SshError::HostKeyChanged {
                        host: self.host.clone(),
                        port: self.port,
                        old_fingerprint: old.fingerprint,
                        new_fingerprint: new.fingerprint,
                    });
                    return Ok(false);
                }
                match self
                    .known_hosts
                    .trust(&self.host, self.port, public_key, true)
                {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        self.record(error);
                        Ok(false)
                    }
                }
            }
        }
    }
}

impl ClientHandler {
    async fn ask(&self, request: HostKeyPromptRequest) -> bool {
        match tokio::time::timeout(self.decision_budget, self.prompt.decide(request)).await {
            Ok(accepted) => accepted,
            Err(_) => {
                self.record(SshError::HostKeyPromptTimeout);
                false
            }
        }
    }
}

// ---------- session actor ----------

enum Tick {
    Command(Option<SessionCommand>),
    Message(Option<ChannelMsg>),
    Keepalive,
}

struct Actor {
    handle: Handle<ClientHandler>,
    channel: Channel<Msg>,
    events: Option<mpsc::Sender<SessionEvent>>,
    replay: std::collections::VecDeque<u8>,
    replay_limit: usize,
    cmd_rx: mpsc::Receiver<SessionCommand>,
    keepalive: Option<tokio::time::Interval>,
    exit_code: Option<i32>,
    exit_emitted: bool,
}

impl Actor {
    async fn run(mut self) {
        loop {
            let tick: Tick = tokio::select! {
                command = self.cmd_rx.recv() => Tick::Command(command),
                message = self.channel.wait() => Tick::Message(message),
                _ = keepalive_tick(&mut self.keepalive) => Tick::Keepalive,
            };
            match tick {
                Tick::Command(None) => {
                    // All handles dropped: shut down.
                    self.finish("session closed").await;
                    break;
                }
                Tick::Command(Some(command)) => match command {
                    SessionCommand::Write(data) => {
                        if let Err(error) = self.channel.data_bytes(data).await {
                            self.fail(SshError::Protocol(format!("write failed: {error}")))
                                .await;
                            break;
                        }
                    }
                    SessionCommand::Resize { cols, rows } => {
                        if let Err(error) = self
                            .channel
                            .window_change(u32::from(cols), u32::from(rows), 0, 0)
                            .await
                        {
                            tracing::warn!("resize request failed: {error}");
                        }
                    }
                    SessionCommand::Attach(consumer) => {
                        self.events = Some(consumer);
                        self.flush_replay().await;
                    }
                    SessionCommand::Close => {
                        self.finish("closed by user").await;
                        break;
                    }
                },
                Tick::Message(None) => {
                    // No exit status: the transport or channel died under us.
                    // Classified as a retriable network failure so the app can
                    // auto-reconnect; a clean shell exit always arrives as
                    // `ExitStatus` first.
                    if self.exit_code.is_none() {
                        self.fail(SshError::ConnectionClosed).await;
                    } else {
                        let reason = format!(
                            "remote shell exited with status {}",
                            self.exit_code.unwrap_or_default()
                        );
                        self.finish(&reason).await;
                    }
                    break;
                }
                Tick::Message(Some(message)) => {
                    match message {
                        ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                            self.remember(&data);
                            self.emit(SessionEvent::Data(data.to_vec())).await;
                        }
                        ChannelMsg::ExitStatus { exit_status } => {
                            let code = i32::try_from(exit_status).ok();
                            self.exit_code = code;
                            self.emit_exit(code).await;
                        }
                        ChannelMsg::ExitSignal {
                            core_dumped,
                            error_message,
                            signal_name,
                            lang_tag: _,
                        } => {
                            // `ExitSignal` is never followed by an
                            // `ExitStatus`, so by the invariant above this is
                            // an abrupt session death: reported as a retriable
                            // network failure (so the reconnect policy can act
                            // on e.g. an sshd restart) with the signal detail
                            // as the user-facing message.
                            self.exit_code = None;
                            self.emit_exit(None).await;
                            let note = format!(
                                "remote process terminated by signal {signal_name:?}{}: {error_message}",
                                if core_dumped { " (core dumped)" } else { "" }
                            );
                            self.fail_with(SshError::ConnectionClosed, note).await;
                            break;
                        }
                        ChannelMsg::Eof => {
                            // Remote closed its side's output; more messages
                            // (e.g. exit status) may still arrive.
                        }
                        ChannelMsg::Close => {
                            if self.exit_code.is_none() {
                                self.fail(SshError::ConnectionClosed).await;
                            } else {
                                let reason = format!(
                                    "remote shell exited with status {}",
                                    self.exit_code.unwrap_or_default()
                                );
                                self.finish(&reason).await;
                            }
                            break;
                        }
                        _ => {}
                    }
                }
                Tick::Keepalive => {
                    if let Err(error) = self.handle.send_keepalive(true).await {
                        tracing::warn!("ssh keepalive failed: {error}");
                        self.fail(SshError::Interrupted).await;
                        break;
                    }
                }
            }
        }
    }

    fn remember(&mut self, data: &[u8]) {
        self.replay.extend(data.iter().copied());
        while self.replay.len() > self.replay_limit {
            self.replay.pop_front();
        }
    }

    async fn emit(&mut self, event: SessionEvent) {
        let Some(consumer) = self.events.clone() else {
            return;
        };
        match tokio::time::timeout(EVENT_SEND_TIMEOUT, consumer.send(event)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                self.events = None;
            }
            Err(_) => {
                tracing::warn!("session output consumer stalled; detaching (session continues)");
                self.events = None;
            }
        }
    }

    async fn flush_replay(&mut self) {
        let Some(consumer) = self.events.clone() else {
            return;
        };
        let snapshot: Vec<u8> = self.replay.iter().copied().collect();
        for chunk in snapshot.chunks(REPLAY_CHUNK) {
            match tokio::time::timeout(
                EVENT_SEND_TIMEOUT,
                consumer.send(SessionEvent::Data(chunk.to_vec())),
            )
            .await
            {
                Ok(Ok(())) => {}
                _ => {
                    self.events = None;
                    return;
                }
            }
        }
    }

    async fn emit_exit(&mut self, code: Option<i32>) {
        if self.exit_emitted {
            return;
        }
        self.exit_emitted = true;
        self.emit(SessionEvent::Exit { code }).await;
    }

    async fn fail(&mut self, error: SshError) {
        let message = error.to_string();
        self.fail_with(error, message).await;
    }

    /// Like [`Self::fail`], but with an explicit user-facing message (the
    /// classification still comes from `error`).
    async fn fail_with(&mut self, error: SshError, message: String) {
        let category = ErrorCategory::of(&error);
        self.emit(SessionEvent::Status(SessionStatus::Failed {
            category,
            message: message.clone(),
        }))
        .await;
        self.shutdown_connection().await;
        self.emit(SessionEvent::Status(SessionStatus::Closed {
            reason: message,
        }))
        .await;
    }

    async fn finish(&mut self, reason: &str) {
        self.shutdown_connection().await;
        self.emit_exit(self.exit_code).await;
        self.emit(SessionEvent::Status(SessionStatus::Closed {
            reason: reason.to_string(),
        }))
        .await;
    }

    async fn shutdown_connection(&mut self) {
        let _ = self.channel.eof().await;
        let _ = self.channel.close().await;
        let _ = self
            .handle
            .disconnect(
                russh::Disconnect::ByApplication,
                "Termru session closed",
                "en",
            )
            .await;
    }
}

async fn keepalive_tick(keepalive: &mut Option<tokio::time::Interval>) {
    match keepalive {
        Some(interval) => {
            interval.tick().await;
        }
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConnectTimeouts;

    #[test]
    fn error_categories_are_stable() {
        assert_eq!(
            ErrorCategory::of(&SshError::AuthFailed {
                method: "password",
                detail: String::new()
            }),
            ErrorCategory::Auth
        );
        assert_eq!(
            ErrorCategory::of(&SshError::HostKeyRejected),
            ErrorCategory::HostKey
        );
        assert_eq!(
            ErrorCategory::of(&SshError::ConnectTimeout {
                host: "h".into(),
                port: 22,
                seconds: 5
            }),
            ErrorCategory::Timeout
        );
        assert_eq!(
            ErrorCategory::of(&SshError::Interrupted),
            ErrorCategory::Network
        );
    }

    #[test]
    fn secret_value_is_redacted() {
        let secret = crate::config::SecretValue::new("hunter2".to_string());
        let debug = format!("{secret:?}");
        assert!(!debug.contains("hunter2"));
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn auth_failures_are_not_retriable() {
        let error = SshError::AuthFailed {
            method: "password",
            detail: "no".into(),
        };
        assert!(!error.is_retriable());
        assert!(error.is_auth_failure());
        assert!(SshError::Interrupted.is_retriable());
    }

    #[tokio::test]
    async fn dial_reports_dns_failure_for_invalid_name() {
        let (_tx, mut cancel) = watch::channel(false);
        let error = dial(
            "definitely.invalid.invalid",
            22,
            Duration::from_secs(3),
            &mut cancel,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, SshError::DnsFailure { .. }),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn dial_aborts_immediately_when_cancelled() {
        let (_cancel_tx, mut cancel) = watch::channel(true);
        let started = Instant::now();
        // Unroutable test address: black-holes SYNs in most environments, so
        // only cancellation can end this quickly.
        let result = dial("10.255.255.1", 22, Duration::from_secs(60), &mut cancel).await;
        assert!(matches!(result, Err(SshError::SessionClosing)));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "cancel must beat the 60s budget"
        );
    }

    #[tokio::test]
    async fn open_without_network_fails_with_actionable_error() {
        let (tx, mut rx) = mpsc::channel(16);
        let params = ConnectParams {
            host: "127.0.0.1".into(),
            // RFC 5737 TEST-NET-1 port 1: nothing listens; connect refused/timeout.
            port: 1,
            username: "nobody".into(),
            auth: crate::config::AuthChoice::Password {
                password: crate::config::SecretValue::new("x".to_string()),
            },
            term: "xterm-256color".into(),
            keepalive_seconds: 0,
            timeouts: ConnectTimeouts {
                connect: Duration::from_secs(5),
                auth: Duration::from_secs(5),
                host_key_decision: Duration::from_secs(5),
            },
            known_hosts_path: std::path::PathBuf::from("/nonexistent/known_hosts"),
            host_key_prompt: Arc::new(crate::config::RejectUnknownHostKeys),
        };
        let error = Session::open(params, SessionOptions::default(), tx)
            .await
            .unwrap_err();
        assert!(
            matches!(error, SshError::ConnectFailed { .. }),
            "got {error:?}"
        );
        let mut saw_failed = false;
        let mut saw_closed = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                SessionEvent::Status(SessionStatus::Failed { .. }) => saw_failed = true,
                SessionEvent::Status(SessionStatus::Closed { .. }) => saw_closed = true,
                _ => {}
            }
        }
        assert!(saw_failed && saw_closed, "terminal statuses emitted");
    }
}
