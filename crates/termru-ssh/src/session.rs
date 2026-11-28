//! Interactive SSH session: connection setup, authentication, PTY shell,
//! keepalives, and the attach/replay data protocol.
//!
//! ## Concurrency model
//!
//! One tokio task (the *actor*) owns the russh handle and the **read half** of
//! the shell channel. Two small helper tasks own the **write half**:
//!
//! * the *input pump* drains an ordered, bounded input queue into the channel
//!   (`Session::write` / `Session::try_write` feed it). It may block for a
//!   long time waiting for the remote's receive window — and that is fine,
//!   because
//! * the actor never awaits anything russh-side: it keeps draining
//!   `ChannelReadHalf::wait`, which is what lets russh process the window
//!   adjustments the input pump is waiting for. (Before the split, a large
//!   paste made the actor block in `data_bytes` while the remote echoed the
//!   data back; russh's bounded channel filled, its loop stalled, the window
//!   adjust was never processed, and everything deadlocked.)
//! * the *resize pump* forwards the latest requested size (a `watch`, so only
//!   the newest value matters) and is independent of the input pump: a resize
//!   never queues behind a blocked write.
//!
//! ## Route, forwards and setup
//!
//! `connect_and_start` connects every hop of the route (`jump.rs`), then sets
//! the shell channel up (environment, agent forwarding, terminal, shell). The
//! actor also owns the connections to the jump hosts and the port forwards
//! (`forward.rs`); the forwards start when `Ready` has been announced and are
//! stopped, with their listening sockets closed, before the session reports
//! its end.
//!
//! ## Output
//!
//! Output flows to one consumer through a bounded channel. The actor **never
//! drops its consumer because it is slow**: events wait in a small pending
//! queue (consecutive data chunks are merged), reading from the channel pauses
//! when that queue is full — plain flow control, the TCP window fills — and
//! commands such as `Close` stay responsive throughout. The consumer is only
//! forgotten when it is really gone (its receiver was dropped); the session
//! then keeps running and keeps filling the replay ring, so a view that
//! attaches later still gets the scrollback.
//!
//! ## Dead-connection detection
//!
//! russh's keepalive machinery is the single source of truth: if nothing is
//! received for the keepalive interval a keepalive is sent, and after
//! [`DEFAULT_KEEPALIVE_MAX`] unanswered ones the transport is torn down. The
//! actor then sees the channel end **without** an EOF/close from the remote
//! and reports a retriable network failure. A close or EOF *from the remote*
//! (shell exited, killed by a signal, `logout`) is final and never retried.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use russh::client::{self, ChannelOpenHandle, Handle, Handler, Msg};
use russh::{ChannelMsg, ChannelReadHalf, ChannelWriteHalf};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::auth::{MAX_BANNER_BYTES, sanitize_banner};
use crate::config::{AgentForward, ConnectParams, ForwardSpec, HostKeyPromptRequest};
use crate::dial::Budget;
use crate::error::SshError;
use crate::forward::{ForwardRuntime, ForwardStatus, RemoteForwards};
use crate::jump::{Progress, TargetExtras, close_all};
use crate::known_hosts::{Evaluation, KnownHosts};
use crate::reconnect::ReconnectPolicy;
use crate::replay::ReplayRing;

pub const DEFAULT_KEEPALIVE_MAX: usize = 3;
const REPLAY_CHUNK: usize = 32 * 1024;
const CONTROL_QUEUE: usize = 128;
const MAX_REPLAY_DEFAULT: usize = 512 * 1024;
/// Ordered input chunks that may wait for the remote to accept them.
const INPUT_QUEUE: usize = 4096;
/// Queued input chunks are merged up to this size before they hit the wire.
const WRITE_COALESCE: usize = 32 * 1024;
/// Events held back for a slow consumer before reading pauses.
const MAX_PENDING_EVENTS: usize = 32;
/// Consecutive data chunks are merged up to this size while queued.
const COALESCE_LIMIT: usize = 64 * 1024;
/// How long the final events (`Failed`/`Closed`) may wait for the consumer.
const FINAL_FLUSH_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound for the polite part of tearing the connection down.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// Events flowing from the session to its consumer.
#[derive(Debug)]
pub enum SessionEvent {
    Status(SessionStatus),
    /// Raw bytes from the remote shell (stdout and stderr merged by SSH).
    Data(Vec<u8>),
    /// Text the server sent during authentication (the SSH "banner": legal
    /// notices, "authorized use only"). Already stripped of everything a
    /// terminal would interpret (see `sanitize_banner`), with `\n` line
    /// endings; it arrives before `Ready` and before any shell output, also
    /// when authentication then fails.
    Banner(String),
    /// The remote process finished (before the channel closed).
    Exit {
        code: Option<i32>,
    },
    /// What became of one of the host's port forwards. They start after
    /// `Ready` (so these events follow it), once per forward, and again after
    /// every reconnect; a forward that dies later reports `Failed`.
    Forward(ForwardStatus),
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
            // A failure on a jump host is what it is on that host.
            SshError::Hop { source, .. } => Self::of(source),
            SshError::AuthFailed { .. }
            | SshError::AuthTimeout
            | SshError::AuthCancelled
            | SshError::AuthPromptTimeout
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
            | SshError::InputBacklog
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
    /// Binds a new consumer; it first receives the replay ring.
    Attach(mpsc::Sender<SessionEvent>),
    /// Snapshot of the replay ring **without** replacing the consumer (used
    /// to show a second view the same scrollback).
    Replay(oneshot::Sender<Vec<u8>>),
    /// The session is announced as ready: start its port forwards.
    StartForwards,
    Close,
}

/// Handle to a live session. Cheap to clone; all clones address the same
/// actor task.
#[derive(Debug, Clone)]
pub struct Session {
    cmd: mpsc::Sender<SessionCommand>,
    /// Ordered input queue drained by the input pump.
    input: mpsc::Sender<Vec<u8>>,
    /// Latest requested terminal size, applied by the resize pump.
    size: Arc<watch::Sender<(u16, u16)>>,
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
    /// host-key prompt, authentication, shell setup) by setting `cancel` to
    /// `true`. No auto-reconnect: one attempt, then the error surfaces.
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
        // Only now: the forwards' statuses must come after `Ready`.
        let _ = session.cmd.send(SessionCommand::StartForwards).await;
        Ok(session)
    }

    pub(crate) async fn connect_and_start(
        params: ConnectParams,
        options: SessionOptions,
        events: mpsc::Sender<SessionEvent>,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Session, SshError> {
        let prompt_active = Arc::new(AtomicBool::new(false));
        let (banner_tx, mut banner_rx) = mpsc::unbounded_channel::<String>();
        let remote_forwards = Arc::new(RemoteForwards::default());

        // --- every hop: transport, key exchange + host-key decision,
        // authentication (see the `jump` module) ---
        let chain = {
            let mut progress = Progress {
                events: &events,
                cancel,
                prompt_active: &prompt_active,
                banner_tx: &banner_tx,
                banner_rx: &mut banner_rx,
            };
            crate::jump::connect_chain(
                &params,
                TargetExtras {
                    remote_forwards: remote_forwards.clone(),
                    agent_forward: params.setup.agent_forward.clone(),
                },
                &mut progress,
            )
            .await?
        };
        let crate::jump::Chain {
            target: handle,
            jump: jump_handles,
        } = chain;

        // --- interactive shell (the target's budget, same cancellation) ---
        // The whole chain is closed again when this fails.
        let shell = open_shell(&handle, &params, &options, cancel).await;
        let (channel, replies) = match shell {
            Ok(opened) => opened,
            Err(error) => {
                close_all(std::slice::from_ref(&handle)).await;
                close_all(&jump_handles).await;
                return Err(error);
            }
        };

        // Reading and writing are separated from here on (see module docs).
        let (reader, writer) = channel.split();
        let writer = Arc::new(writer);
        let (cmd_tx, cmd_rx) = mpsc::channel(CONTROL_QUEUE);
        let (input_tx, input_rx) = mpsc::channel(INPUT_QUEUE);
        let (size_tx, size_rx) = watch::channel((options.cols, options.rows));
        // The startup command is the first thing in the (ordered) input
        // queue: it reaches the shell before anything the user types. An
        // interactive shell takes Enter as carriage return, like a terminal
        // sends it.
        if let Some(command) = &params.setup.startup_command {
            let mut line = command.clone().into_bytes();
            line.push(b'\r');
            // The queue is empty and has room for thousands of chunks.
            let _ = input_tx.try_send(line);
        }
        let (forward_tx, forward_rx) = mpsc::unbounded_channel();
        let actor = Actor {
            handle: Arc::new(handle),
            jump_handles,
            reader,
            writer: writer.clone(),
            _pumps: [
                AbortOnDrop(tokio::spawn(input_pump(writer.clone(), input_rx))),
                AbortOnDrop(tokio::spawn(resize_pump(writer, size_rx))),
            ],
            events: Some(events),
            pending: VecDeque::new(),
            ring: ReplayRing::new(options.replay_limit),
            cmd_rx,
            exit_code: None,
            exit_emitted: false,
            eof_seen: false,
            replies,
            forward_specs: params.setup.forwards.clone(),
            remote_forwards,
            forwards: None,
            forward_tx,
            forward_rx,
        };
        tokio::spawn(actor.run());

        Ok(Session {
            cmd: cmd_tx,
            input: input_tx,
            size: Arc::new(size_tx),
        })
    }

    /// Queues `data` for the remote shell, waiting while the (large) input
    /// queue is full. Chunks are delivered strictly in queue order.
    pub async fn write(&self, data: Vec<u8>) -> Result<(), SshError> {
        self.input
            .send(data)
            .await
            .map_err(|_| SshError::SessionClosing)
    }

    /// Non-blocking [`Session::write`]: the chunk is enqueued **now**, in the
    /// caller's order, or rejected with [`SshError::InputBacklog`] when the
    /// remote has stopped accepting input and the queue is full. Used by
    /// synchronous callers (the desktop `session_write` command) that must
    /// not reorder or block.
    pub fn try_write(&self, data: Vec<u8>) -> Result<(), SshError> {
        use mpsc::error::TrySendError;
        match self.input.try_send(data) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(SshError::InputBacklog),
            Err(TrySendError::Closed(_)) => Err(SshError::SessionClosing),
        }
    }

    /// Requests a new terminal size. Only the latest request matters and it
    /// never waits behind pending input.
    pub fn try_resize(&self, cols: u16, rows: u16) -> Result<(), SshError> {
        self.size
            .send((cols, rows))
            .map_err(|_| SshError::SessionClosing)
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), SshError> {
        self.try_resize(cols, rows)
    }

    /// Binds the session's output to `consumer`. The previous consumer (if
    /// any) stops receiving; the new one first receives the replay buffer.
    pub async fn attach(&self, consumer: mpsc::Sender<SessionEvent>) -> Result<(), SshError> {
        self.cmd
            .send(SessionCommand::Attach(consumer))
            .await
            .map_err(|_| SshError::SessionClosing)
    }

    /// Snapshot of the bounded replay buffer.
    ///
    /// Unlike [`Session::attach`], this does **not** replace the active
    /// consumer: the session keeps streaming to whoever is attached while
    /// another view receives the same scrollback. (A caller that must also
    /// register a live view without a gap keeps its own [`ReplayRing`] next to
    /// the view list, as the desktop session manager does.)
    pub async fn replay_snapshot(&self) -> Result<Vec<u8>, SshError> {
        let (tx, rx) = oneshot::channel();
        self.cmd
            .send(SessionCommand::Replay(tx))
            .await
            .map_err(|_| SshError::SessionClosing)?;
        rx.await.map_err(|_| SshError::SessionClosing)
    }

    /// Graceful local close: EOF + channel close + disconnect. Does not wait
    /// for queued input (a blocked write must not keep a closed tab alive).
    pub async fn close(&self) -> Result<(), SshError> {
        // The actor exits even if the command queue is full, so use a best
        // effort send: a closed queue already means the session is gone.
        let _ = self.cmd.send(SessionCommand::Close).await;
        Ok(())
    }
}

/// A channel request whose reply is still to come, in the order the
/// requests were sent (the server answers in that order).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Request {
    Env(String),
    AgentForward,
    Pty,
    Shell,
}

/// Opens the session channel and sets it up: environment, agent forwarding,
/// terminal, shell — each under the connect budget and the cancellation.
/// Returns the channel and the replies still to read from it.
async fn open_shell(
    handle: &Handle<ClientHandler>,
    params: &ConnectParams,
    options: &SessionOptions,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(russh::Channel<Msg>, VecDeque<Request>), SshError> {
    let mut budget = Budget::new(params.timeouts.connect);
    let connect_timeout = || SshError::ConnectTimeout {
        host: params.host.clone(),
        port: params.port,
        seconds: params.timeouts.connect.as_secs(),
    };
    let channel = budget
        .run(
            async {
                handle
                    .channel_open_session()
                    .await
                    .map_err(|e| SshError::Protocol(format!("open session channel: {e}")))
            },
            None,
            cancel,
            connect_timeout,
        )
        .await?;
    let mut replies = VecDeque::new();

    // Environment first: a server applies it to the shell that follows. Each
    // request asks for an answer, so a refusal (the usual case for a name
    // the server's `AcceptEnv` does not list) is noticed and logged — the
    // name only, never the value.
    for (name, value) in &params.setup.env {
        budget
            .run(
                async {
                    channel
                        .set_env(true, name.clone(), value.clone())
                        .await
                        .map_err(|e| SshError::Protocol(format!("set environment variable: {e}")))
                },
                None,
                cancel,
                connect_timeout,
            )
            .await?;
        replies.push_back(Request::Env(name.clone()));
    }
    if params.setup.agent_forward.is_some() {
        budget
            .run(
                async {
                    channel
                        .agent_forward(true)
                        .await
                        .map_err(|e| SshError::Protocol(format!("request agent forwarding: {e}")))
                },
                None,
                cancel,
                connect_timeout,
            )
            .await?;
        replies.push_back(Request::AgentForward);
    }
    budget
        .run(
            async {
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
                    .map_err(|e| SshError::Protocol(format!("terminal allocation failed: {e}")))
            },
            None,
            cancel,
            connect_timeout,
        )
        .await?;
    replies.push_back(Request::Pty);
    budget
        .run(
            async {
                channel
                    .request_shell(true)
                    .await
                    .map_err(|e| SshError::Protocol(format!("request shell: {e}")))
            },
            None,
            cancel,
            connect_timeout,
        )
        .await?;
    replies.push_back(Request::Shell);
    Ok((channel, replies))
}

// ---------- handshake helpers ----------

pub(crate) fn take_recorded(
    recorded: &Arc<std::sync::Mutex<Option<SshError>>>,
) -> Option<SshError> {
    recorded.lock().ok().and_then(|mut g| g.take())
}

pub(crate) fn map_connect_error(
    err: russh::Error,
    host: &str,
    port: u16,
    seconds: u64,
) -> SshError {
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

// ---------- host-key verification handler ----------

pub(crate) struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    prompt: Arc<dyn crate::config::HostKeyPrompt>,
    decision_budget: Duration,
    /// `true` while a human decision is pending; the connect budget pauses.
    prompt_active: Arc<AtomicBool>,
    recorded: Arc<std::sync::Mutex<Option<SshError>>>,
    banner: BannerSink,
    /// Where the channels of the server's remote forwards go. Only the
    /// target's handler is ever asked; a jump host's is empty.
    remote_forwards: Arc<RemoteForwards>,
    /// `Some` only on the target of a host that opted into agent forwarding:
    /// the only handler that answers an `auth-agent` channel.
    agent_forward: Option<AgentForward>,
}

/// Hands the server's authentication banner to the session, bounded.
pub(crate) struct BannerSink {
    tx: mpsc::UnboundedSender<String>,
    /// Bytes forwarded so far: a server may send banners forever.
    forwarded: usize,
}

impl BannerSink {
    pub(crate) fn new(tx: mpsc::UnboundedSender<String>) -> Self {
        Self { tx, forwarded: 0 }
    }

    fn offer(&mut self, raw: &str) {
        if self.forwarded >= MAX_BANNER_BYTES {
            return;
        }
        let text = sanitize_banner(raw);
        if text.trim().is_empty() {
            return;
        }
        self.forwarded += text.len();
        // The receiver is gone when the connect was abandoned: nothing to do.
        let _ = self.tx.send(text);
    }
}

pub(crate) async fn forward_banner(events: &mpsc::Sender<SessionEvent>, text: String) {
    let _ = events.send(SessionEvent::Banner(text)).await;
}

/// Runs blocking file work (known_hosts) off the async workers.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, SshError> + Send + 'static,
) -> Result<T, SshError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| SshError::Protocol(format!("known_hosts task failed: {error}")))?
}

impl ClientHandler {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        host: String,
        port: u16,
        known_hosts: Arc<KnownHosts>,
        prompt: Arc<dyn crate::config::HostKeyPrompt>,
        decision_budget: Duration,
        prompt_active: Arc<AtomicBool>,
        recorded: Arc<std::sync::Mutex<Option<SshError>>>,
        banner: BannerSink,
        remote_forwards: Arc<RemoteForwards>,
        agent_forward: Option<AgentForward>,
    ) -> Self {
        Self {
            host,
            port,
            known_hosts,
            prompt,
            decision_budget,
            prompt_active,
            recorded,
            banner,
            remote_forwards,
            agent_forward,
        }
    }

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

    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        self.banner.offer(banner);
        Ok(())
    }

    /// The server opens a channel for a connection to one of our remote
    /// forwards (`-R`): connect to its local target, then confirm. A channel
    /// for a port nobody asked for is refused.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        match self.remote_forwards.lookup(connected_port) {
            Some(target) => {
                tokio::spawn(crate::forward::serve_remote(channel, reply, target));
            }
            None => {
                tracing::debug!(
                    address = connected_address,
                    port = connected_port,
                    "refusing a forwarded channel nobody asked for"
                );
                reply
                    .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                    .await;
            }
        }
        Ok(())
    }

    /// The server asks for our agent (`ForwardAgent`). Only the target of a
    /// host that opted in answers; a jump host never gets one, and neither
    /// does a server that opens the channel without having been asked.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        match &self.agent_forward {
            Some(agent) => {
                tokio::spawn(crate::forward::serve_agent(
                    channel,
                    reply,
                    agent.socket.clone(),
                ));
            }
            None => {
                tracing::warn!("a server opened an agent channel that was not requested; refused");
                reply
                    .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                    .await;
            }
        }
        Ok(())
    }

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

        // The known_hosts file is read and written synchronously: not on an
        // async worker.
        let evaluation = {
            let known_hosts = self.known_hosts.clone();
            let (host, port, key) = (self.host.clone(), self.port, public_key.clone());
            blocking(move || known_hosts.evaluate(&host, port, &key)).await
        };
        let evaluation = match evaluation {
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
                // A host we know under other key types that now shows a
                // type we have never seen is worth a louder question than
                // a host we have never seen at all.
                let other_key_types = {
                    let known_hosts = self.known_hosts.clone();
                    let (host, port) = (self.host.clone(), self.port);
                    blocking(move || known_hosts.known_key_types(&host, port))
                        .await
                        .unwrap_or_default()
                };
                let request = HostKeyPromptRequest::Unknown {
                    host: self.host.clone(),
                    port: self.port,
                    key: crate::known_hosts::HostKeyEntry {
                        key_type,
                        fingerprint: fingerprint.clone(),
                    },
                    other_key_types,
                };
                let accepted = self.ask(request).await;
                if !accepted {
                    self.record(SshError::HostKeyRejected);
                    return Ok(false);
                }
                self.trust(public_key, false).await
            }
            Evaluation::Changed { old, new } => {
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
                self.trust(public_key, true).await
            }
        }
    }
}

impl ClientHandler {
    /// Writes the trust decision to known_hosts (off the async workers).
    async fn trust(
        &self,
        key: &russh::keys::PublicKey,
        replace_existing: bool,
    ) -> Result<bool, russh::Error> {
        let known_hosts = self.known_hosts.clone();
        let (host, port, key) = (self.host.clone(), self.port, key.clone());
        match blocking(move || known_hosts.trust(&host, port, &key, replace_existing)).await {
            Ok(()) => Ok(true),
            Err(error) => {
                self.record(error);
                Ok(false)
            }
        }
    }

    async fn ask(&self, request: HostKeyPromptRequest) -> bool {
        // The connect deadline pauses while the user decides; the decision has
        // its own budget.
        self.prompt_active
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let outcome = tokio::time::timeout(self.decision_budget, self.prompt.decide(request)).await;
        self.prompt_active
            .store(false, std::sync::atomic::Ordering::SeqCst);
        match outcome {
            Ok(accepted) => accepted,
            Err(_) => {
                self.record(SshError::HostKeyPromptTimeout);
                false
            }
        }
    }
}

// ---------- write-side helper tasks ----------

/// Aborts its task when dropped, so helper tasks never outlive the actor.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Drains the ordered input queue into the channel.
///
/// Queued chunks are merged (up to [`WRITE_COALESCE`]) so a burst of small
/// writes becomes few packets, without ever reordering them. This task may
/// wait for the remote's window for as long as it takes; nothing else waits on
/// it.
async fn input_pump(writer: Arc<ChannelWriteHalf<Msg>>, mut input: mpsc::Receiver<Vec<u8>>) {
    while let Some(mut batch) = input.recv().await {
        while batch.len() < WRITE_COALESCE {
            match input.try_recv() {
                Ok(more) => batch.extend_from_slice(&more),
                Err(_) => break,
            }
        }
        if let Err(error) = writer.data_bytes(batch).await {
            // The transport is gone; the actor sees the channel end too and
            // reports it. Dropping `input` makes later writes fail fast.
            tracing::warn!("ssh input pump stopped: {error}");
            break;
        }
    }
}

/// Forwards the newest requested terminal size.
async fn resize_pump(writer: Arc<ChannelWriteHalf<Msg>>, mut size: watch::Receiver<(u16, u16)>) {
    while size.changed().await.is_ok() {
        let (cols, rows) = *size.borrow_and_update();
        if let Err(error) = writer
            .window_change(u32::from(cols), u32::from(rows), 0, 0)
            .await
        {
            tracing::warn!("resize request failed: {error}");
            break;
        }
    }
}

// ---------- session actor ----------

enum Tick {
    Command(Option<SessionCommand>),
    Message(Option<ChannelMsg>),
    /// A forward reported its state.
    Forward(Option<ForwardStatus>),
    /// A slot in the consumer's queue (`None`: the consumer is gone).
    Slot(Option<mpsc::OwnedPermit<SessionEvent>>),
}

/// How the actor's serve loop ended.
enum End {
    /// Final, not retriable: user close, shell exit, exit signal, remote close.
    Finished { reason: String },
    /// Transport loss: reported as a failure the app may reconnect from.
    Failed { error: SshError },
}

struct Actor {
    /// Shared with the forwards, which open channels on it.
    handle: Arc<Handle<ClientHandler>>,
    /// The connections to the jump hosts (first hop first): the target's
    /// connection runs through them.
    jump_handles: Vec<Handle<ClientHandler>>,
    reader: ChannelReadHalf,
    writer: Arc<ChannelWriteHalf<Msg>>,
    _pumps: [AbortOnDrop; 2],
    /// `None` once the consumer's receiver was dropped.
    events: Option<mpsc::Sender<SessionEvent>>,
    /// Events the consumer has not accepted yet (bounded, see
    /// [`MAX_PENDING_EVENTS`]).
    pending: VecDeque<SessionEvent>,
    ring: ReplayRing,
    cmd_rx: mpsc::Receiver<SessionCommand>,
    exit_code: Option<i32>,
    exit_emitted: bool,
    /// The remote sent EOF on the channel.
    eof_seen: bool,
    /// Channel requests that have not been answered yet, oldest first.
    replies: VecDeque<Request>,
    /// Forwards to start after `Ready`, and what they share with the handler.
    forward_specs: Vec<ForwardSpec>,
    remote_forwards: Arc<RemoteForwards>,
    forwards: Option<ForwardRuntime>,
    forward_tx: mpsc::UnboundedSender<ForwardStatus>,
    forward_rx: mpsc::UnboundedReceiver<ForwardStatus>,
}

impl Actor {
    async fn run(mut self) {
        let end = self.serve().await;
        self.conclude(end).await;
    }

    async fn serve(&mut self) -> End {
        loop {
            let tick = tokio::select! {
                command = self.cmd_rx.recv() => Tick::Command(command),
                status = self.forward_rx.recv() => Tick::Forward(status),
                slot = reserve_slot(self.events.clone()),
                    if self.events.is_some() && !self.pending.is_empty() => Tick::Slot(slot),
                // Reading pauses only while the consumer is `MAX_PENDING_EVENTS`
                // behind: that is flow control, not a dropped consumer.
                message = self.reader.wait(), if self.pending.len() < MAX_PENDING_EVENTS => {
                    Tick::Message(message)
                }
            };
            match tick {
                Tick::Command(None) => {
                    // All handles dropped: shut down.
                    return End::Finished {
                        reason: "session closed".to_string(),
                    };
                }
                Tick::Command(Some(SessionCommand::Close)) => {
                    return End::Finished {
                        reason: "closed by user".to_string(),
                    };
                }
                Tick::Command(Some(SessionCommand::Attach(consumer))) => self.attach(consumer),
                Tick::Command(Some(SessionCommand::StartForwards)) => self.start_forwards(),
                Tick::Forward(Some(status)) => self.queue(SessionEvent::Forward(status)),
                // The actor holds a sender itself: the channel never ends
                // while it runs.
                Tick::Forward(None) => {}
                Tick::Command(Some(SessionCommand::Replay(tx))) => {
                    let _ = tx.send(self.ring.snapshot());
                }
                Tick::Slot(Some(permit)) => {
                    if let Some(event) = self.pending.pop_front() {
                        permit.send(event);
                    }
                }
                Tick::Slot(None) => {
                    // The consumer's receiver is gone. The session keeps
                    // running (the ring keeps the output for a later view);
                    // whoever owns the session decides whether to close it.
                    tracing::debug!("session output consumer went away");
                    self.events = None;
                    self.pending.clear();
                }
                Tick::Message(None) => return self.channel_ended(),
                Tick::Message(Some(message)) => {
                    if let Some(end) = self.on_message(message) {
                        return end;
                    }
                }
            }
        }
    }

    /// The channel's message stream ended without a `Close` message.
    ///
    /// Only a transport that disappeared under us (reset, keepalive timeout,
    /// I/O error) looks like this *without* an EOF or exit status first; that
    /// is the one retriable case. After the remote said it is done, the end of
    /// the stream is just the end.
    fn channel_ended(&self) -> End {
        end_of_stream(self.exit_code, self.eof_seen)
    }

    fn remote_end_reason(&self) -> String {
        remote_end_reason(self.exit_code)
    }

    fn on_message(&mut self, message: ChannelMsg) -> Option<End> {
        match message {
            ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                self.ring.push(&data);
                self.queue_data(&data);
            }
            ChannelMsg::ExitStatus { exit_status } => {
                let code = i32::try_from(exit_status).ok();
                self.exit_code = code;
                self.queue_exit(code);
            }
            ChannelMsg::ExitSignal {
                core_dumped,
                error_message,
                signal_name,
                lang_tag: _,
            } => {
                // The shell was killed by a signal (`kill -9 $$`, OOM killer,
                // admin action). That is an answer from a healthy server, not
                // a lost connection: final, never retried.
                self.queue_exit(None);
                let mut reason = format!(
                    "the remote process was terminated by signal {}{}",
                    signal_label(&signal_name),
                    if core_dumped { " (core dumped)" } else { "" }
                );
                if !error_message.trim().is_empty() {
                    reason.push_str(": ");
                    reason.push_str(error_message.trim());
                }
                return Some(End::Finished { reason });
            }
            ChannelMsg::Eof => {
                // The remote finished its output; an exit status may still
                // follow. From here on the end of the stream is final.
                self.eof_seen = true;
            }
            ChannelMsg::Success => {
                self.replies.pop_front();
            }
            ChannelMsg::Failure => self.request_refused(),
            ChannelMsg::Close => {
                return Some(End::Finished {
                    reason: self.remote_end_reason(),
                });
            }
            _ => {}
        }
        None
    }

    /// The server said no to the oldest unanswered request.
    ///
    /// A refused environment variable is the normal case (the server's
    /// `AcceptEnv` decides): only the name is logged, at debug level. The
    /// refusals that change what the user gets are louder.
    fn request_refused(&mut self) {
        match self.replies.pop_front() {
            Some(Request::Env(name)) => {
                tracing::debug!(variable = %name, "the server did not accept the environment variable");
            }
            Some(Request::AgentForward) => {
                tracing::warn!("the server refused agent forwarding");
            }
            Some(Request::Pty) => tracing::warn!("the server refused the terminal request"),
            Some(Request::Shell) => tracing::warn!("the server refused to start the shell"),
            None => tracing::debug!("the server refused a request nobody sent"),
        }
    }

    /// Starts the host's forwards (once; `Ready` was just announced).
    fn start_forwards(&mut self) {
        if self.forwards.is_some() || self.forward_specs.is_empty() {
            return;
        }
        let specs = std::mem::take(&mut self.forward_specs);
        self.forwards = Some(crate::forward::start(
            &self.handle,
            specs,
            &self.remote_forwards,
            &self.forward_tx,
        ));
    }

    /// Queues output for the consumer, merging with the previous chunk.
    fn queue_data(&mut self, data: &[u8]) {
        if self.events.is_none() {
            return;
        }
        if let Some(SessionEvent::Data(tail)) = self.pending.back_mut()
            && tail.len() + data.len() <= COALESCE_LIMIT
        {
            tail.extend_from_slice(data);
            return;
        }
        self.pending.push_back(SessionEvent::Data(data.to_vec()));
    }

    fn queue_exit(&mut self, code: Option<i32>) {
        if self.exit_emitted {
            return;
        }
        self.exit_emitted = true;
        self.queue(SessionEvent::Exit { code });
    }

    fn queue(&mut self, event: SessionEvent) {
        if self.events.is_some() {
            self.pending.push_back(event);
        }
    }

    /// Binds a new consumer: scrollback first, then everything that follows.
    fn attach(&mut self, consumer: mpsc::Sender<SessionEvent>) {
        // Undelivered output is already part of the ring; keeping it queued
        // would show it twice. Statuses/exits stay queued.
        self.pending
            .retain(|event| !matches!(event, SessionEvent::Data(_)));
        let snapshot = self.ring.snapshot();
        for chunk in snapshot.chunks(REPLAY_CHUNK).rev() {
            self.pending.push_front(SessionEvent::Data(chunk.to_vec()));
        }
        self.events = Some(consumer);
    }

    /// Tears the connection down, then delivers the final events.
    async fn conclude(&mut self, end: End) {
        // The forwards go first and are waited for: their listening sockets
        // are closed before anybody hears that the session ended, so a
        // reconnect can bind the same ports.
        if let Some(forwards) = self.forwards.take() {
            forwards.shutdown().await;
        }
        self.shutdown_connection().await;
        match end {
            End::Finished { reason } => {
                self.queue_exit(self.exit_code);
                self.queue(SessionEvent::Status(SessionStatus::Closed { reason }));
            }
            End::Failed { error } => {
                let message = error.to_string();
                self.queue(SessionEvent::Status(SessionStatus::Failed {
                    category: ErrorCategory::of(&error),
                    message: message.clone(),
                }));
                self.queue(SessionEvent::Status(SessionStatus::Closed {
                    reason: message,
                }));
            }
        }
        self.flush_pending().await;
    }

    /// Delivers what is still queued — the last events (`Failed`, `Closed`)
    /// matter most. A consumer that never reads cannot keep the task alive
    /// forever, hence the bound.
    async fn flush_pending(&mut self) {
        let Some(consumer) = self.events.clone() else {
            return;
        };
        let deadline = tokio::time::Instant::now() + FINAL_FLUSH_TIMEOUT;
        while let Some(event) = self.pending.pop_front() {
            match tokio::time::timeout_at(deadline, consumer.send(event)).await {
                Ok(Ok(())) => {}
                _ => {
                    tracing::warn!("session consumer did not take the final events");
                    break;
                }
            }
        }
    }

    async fn shutdown_connection(&mut self) {
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
            let _ = self.writer.eof().await;
            let _ = self.writer.close().await;
            let _ = self
                .handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "Termru session closed",
                    "en",
                )
                .await;
        })
        .await;
        // The target's connection ran through the jump hosts: they go last.
        close_all(&self.jump_handles).await;
    }
}

/// Verdict for a channel message stream that ended without a `Close`.
fn end_of_stream(exit_code: Option<i32>, eof_seen: bool) -> End {
    if exit_code.is_some() || eof_seen {
        End::Finished {
            reason: remote_end_reason(exit_code),
        }
    } else {
        End::Failed {
            error: SshError::ConnectionClosed,
        }
    }
}

fn remote_end_reason(exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) => format!("remote shell exited with status {code}"),
        None => "the remote host closed the session".to_string(),
    }
}

/// A slot in the consumer's queue; never resolves when there is no consumer.
async fn reserve_slot(
    events: Option<mpsc::Sender<SessionEvent>>,
) -> Option<mpsc::OwnedPermit<SessionEvent>> {
    match events {
        Some(sender) => sender.reserve_owned().await.ok(),
        None => std::future::pending().await,
    }
}

fn signal_label(signal: &russh::Sig) -> String {
    use russh::Sig;
    match signal {
        Sig::ABRT => "ABRT".to_string(),
        Sig::ALRM => "ALRM".to_string(),
        Sig::FPE => "FPE".to_string(),
        Sig::HUP => "HUP".to_string(),
        Sig::ILL => "ILL".to_string(),
        Sig::INT => "INT".to_string(),
        Sig::KILL => "KILL".to_string(),
        Sig::PIPE => "PIPE".to_string(),
        Sig::QUIT => "QUIT".to_string(),
        Sig::SEGV => "SEGV".to_string(),
        Sig::TERM => "TERM".to_string(),
        Sig::USR1 => "USR1".to_string(),
        Sig::Custom(name) => name.clone(),
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
        assert_eq!(
            ErrorCategory::of(&SshError::InputBacklog),
            ErrorCategory::Local
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

    /// Regression test: only a stream that vanished *without* the remote
    /// saying it was done is a retriable failure; after EOF or an exit status
    /// the end of the stream is final (it used to be retried in a loop).
    #[test]
    fn only_a_silent_stream_end_is_retriable() {
        match end_of_stream(None, false) {
            End::Failed { error } => assert!(error.is_retriable()),
            End::Finished { .. } => panic!("a vanished transport must be a failure"),
        }
        match end_of_stream(None, true) {
            End::Finished { reason } => assert!(reason.contains("closed the session")),
            End::Failed { .. } => panic!("EOF from the remote is final"),
        }
        match end_of_stream(Some(3), false) {
            End::Finished { reason } => assert!(reason.contains("status 3")),
            End::Failed { .. } => panic!("an exit status is final"),
        }
    }

    #[test]
    fn signal_names_are_readable() {
        assert_eq!(signal_label(&russh::Sig::KILL), "KILL");
        assert_eq!(signal_label(&russh::Sig::Custom("XCPU".into())), "XCPU");
    }

    #[tokio::test]
    async fn open_without_network_fails_with_actionable_error() {
        let (tx, mut rx) = mpsc::channel(16);
        let params = ConnectParams {
            host: "127.0.0.1".into(),
            // Nothing listens on port 1: connect refused.
            port: 1,
            username: "nobody".into(),
            auth: vec![crate::config::AuthChoice::Password {
                password: crate::config::SecretValue::new("x".to_string()),
            }],
            term: "xterm-256color".into(),
            keepalive_seconds: 0,
            timeouts: ConnectTimeouts {
                connect: Duration::from_secs(5),
                auth: Duration::from_secs(5),
                host_key_decision: Duration::from_secs(5),
                ..ConnectTimeouts::default()
            },
            known_hosts_path: std::path::PathBuf::from("/nonexistent/known_hosts"),
            extra_known_hosts: Vec::new(),
            host_key_prompt: Arc::new(crate::config::RejectUnknownHostKeys),
            auth_prompt: Arc::new(crate::config::NoAuthPrompts),
            setup: crate::config::SessionSetup::default(),
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
