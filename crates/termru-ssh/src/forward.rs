//! Port forwarding: `-L`, `-R` and `-D`.
//!
//! The forwards of a session start once it is ready (the session actor starts
//! them when `Ready` has been announced), live exactly as long as the connection and are
//! started again by a reconnect: they belong to the session actor, which
//! aborts them (and waits until their sockets are closed) before it reports
//! the session's end, so a reconnect can bind the same ports at once.
//!
//! * **Local** (`-L`): listen here; for each accepted socket open a
//!   `direct-tcpip` channel to the target and copy both ways.
//! * **Remote** (`-R`): ask the server to listen (`tcpip-forward`); the
//!   session's handler answers each `forwarded-tcpip` channel the server opens
//!   by connecting to the local target (see [`RemoteForwards`]).
//! * **Dynamic** (`-D`): listen here as a SOCKS5 proxy (no authentication,
//!   `CONNECT` only; IPv4, IPv6 and domain names), opening a `direct-tcpip`
//!   channel for whatever the client asks for. Domain names are resolved by
//!   the server, as `ssh -D` does.
//!
//! Without an explicit bind address a listener is bound to loopback. A
//! listener that is reachable from the network is an explicit choice, and a
//! SOCKS proxy that is (no authentication!) is the user's responsibility;
//! both are logged.
//!
//! Every forward reports its state ([`ForwardStatus`]) as a session event:
//! listening (with the address actually bound) or failed, with the reason in
//! words.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::ChannelOpenFailure;
use russh::client::{ChannelOpenHandle, Handle, Msg};
use termru_core::ForwardKind;
use termru_core::validate::strip_brackets;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};

use crate::config::ForwardSpec;
use crate::session::ClientHandler;

/// Simultaneous connections one forward serves. A client that opens more is
/// turned away (its socket is closed at once) instead of piling up channels.
pub const MAX_CONNECTIONS_PER_FORWARD: usize = 128;
/// How long the server may take to confirm a `direct-tcpip` channel.
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a SOCKS client may take to say what it wants.
const SOCKS_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the local side of a remote forward may take to accept.
const LOCAL_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Pause after a failed `accept`, so a persistent error cannot spin.
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(100);

/// What became of one forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardStatus {
    /// The caller's name for the forward ([`ForwardSpec::id`]).
    pub id: u32,
    pub kind: ForwardKind,
    pub state: ForwardState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardState {
    /// The listener is up: where it is bound (here for `local`/`dynamic`, on
    /// the server for `remote`).
    Listening { address: String, port: u16 },
    /// The forward could not be set up (or died); `reason` says why.
    Failed { reason: String },
}

type StatusTx = mpsc::UnboundedSender<ForwardStatus>;

fn report(status: &StatusTx, spec: &ForwardSpec, state: ForwardState) {
    // The receiver is gone when the session is: nothing to tell.
    let _ = status.send(ForwardStatus {
        id: spec.id,
        kind: spec.kind,
        state,
    });
}

/// Where the connections of the server's `forwarded-tcpip` channels go.
///
/// Shared between the forwards (which register a target once the server
/// agreed to listen) and the session's handler (which receives the channel
/// opens). Keyed by the port the server listens on: a server reports the
/// address it matched in several spellings (`localhost`, `127.0.0.1`, empty),
/// the port is the part that identifies the forward.
#[derive(Default)]
pub(crate) struct RemoteForwards {
    targets: Mutex<HashMap<u32, (String, u16)>>,
}

impl RemoteForwards {
    fn register(&self, listen_port: u32, host: String, port: u16) {
        if let Ok(mut targets) = self.targets.lock() {
            targets.insert(listen_port, (host, port));
        }
    }

    pub(crate) fn lookup(&self, listen_port: u32) -> Option<(String, u16)> {
        self.targets.lock().ok()?.get(&listen_port).cloned()
    }
}

/// The running forwards of a session.
pub(crate) struct ForwardRuntime {
    tasks: Vec<JoinHandle<()>>,
}

impl ForwardRuntime {
    /// Stops every forward and waits until its listener is closed.
    pub(crate) async fn shutdown(mut self) {
        let tasks = std::mem::take(&mut self.tasks);
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            // `Cancelled` is the expected answer.
            let _ = task.await;
        }
    }
}

impl Drop for ForwardRuntime {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Starts every forward of `specs` on the connection `handle`.
pub(crate) fn start(
    handle: &Arc<Handle<ClientHandler>>,
    specs: Vec<ForwardSpec>,
    remote: &Arc<RemoteForwards>,
    status: &StatusTx,
) -> ForwardRuntime {
    let tasks = specs
        .into_iter()
        .map(|spec| {
            let handle = handle.clone();
            let remote = remote.clone();
            let status = status.clone();
            tokio::spawn(async move {
                match spec.kind {
                    ForwardKind::Local => run_local(&spec, handle, &status).await,
                    ForwardKind::Dynamic => run_dynamic(&spec, handle, &status).await,
                    ForwardKind::Remote => run_remote(&spec, &handle, &remote, &status).await,
                }
            })
        })
        .collect();
    ForwardRuntime { tasks }
}

// ---------- listening here ----------

/// Binds the local listener of a `local`/`dynamic` forward.
async fn bind(spec: &ForwardSpec) -> Result<TcpListener, String> {
    let address = match spec.bind_address.as_deref().map(strip_brackets) {
        None => "127.0.0.1",
        // `localhost` may resolve to either family; the loopback listener of
        // a forward is IPv4, which is what a client dials for `localhost`
        // first on the platforms that matter.
        Some(name) if name.eq_ignore_ascii_case("localhost") => "127.0.0.1",
        Some(address) => address,
    };
    let listener = TcpListener::bind((address, spec.bind_port))
        .await
        .map_err(|error| {
            format!(
                "cannot listen on {address}:{}: {}",
                spec.bind_port,
                describe_io(&error)
            )
        })?;
    if let Ok(local) = listener.local_addr()
        && !local.ip().is_loopback()
    {
        // Not an error: the user asked for it. It deserves a trace.
        tracing::warn!(
            forward = spec.id,
            address = %local,
            "a forward listens on a non-loopback address: anything that can reach it can use the tunnel"
        );
    }
    Ok(listener)
}

fn describe_io(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => "the port is already in use".to_string(),
        std::io::ErrorKind::PermissionDenied => {
            "permission denied (ports below 1024 need extra privileges)".to_string()
        }
        std::io::ErrorKind::AddrNotAvailable => {
            "that address does not belong to this machine".to_string()
        }
        _ => error.to_string(),
    }
}

/// Accepts connections on `listener` for as long as the task lives and runs
/// `serve` for each, up to [`MAX_CONNECTIONS_PER_FORWARD`] at a time.
async fn accept_loop<F, Fut>(spec: &ForwardSpec, listener: TcpListener, serve: F)
where
    F: Fn(TcpStream, SocketAddr) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS_PER_FORWARD));
    // Dropped with the supervisor: aborts every connection of the forward.
    let mut connections: JoinSet<()> = JoinSet::new();
    loop {
        let (socket, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::debug!(forward = spec.id, %error, "accept failed");
                tokio::time::sleep(ACCEPT_ERROR_PAUSE).await;
                continue;
            }
        };
        // Reap what has ended, so the set does not grow without bound.
        while connections.try_join_next().is_some() {}
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            tracing::debug!(forward = spec.id, %peer, "too many connections; turned one away");
            continue;
        };
        let serving = serve(socket, peer);
        connections.spawn(async move {
            serving.await;
            drop(permit);
        });
    }
}

async fn run_local(spec: &ForwardSpec, handle: Arc<Handle<ClientHandler>>, status: &StatusTx) {
    let (Some(host), Some(port)) = (spec.target_host.clone(), spec.target_port) else {
        report(
            status,
            spec,
            ForwardState::Failed {
                reason: "a local forward needs a target host and port".to_string(),
            },
        );
        return;
    };
    let listener = match bind(spec).await {
        Ok(listener) => listener,
        Err(reason) => {
            report(status, spec, ForwardState::Failed { reason });
            return;
        }
    };
    report_listening(status, spec, &listener);
    let id = spec.id;
    accept_loop(spec, listener, move |mut socket, peer| {
        let handle = handle.clone();
        let host = strip_brackets(&host).to_string();
        async move {
            match open_channel(&handle, &host, port, peer).await {
                Ok(channel) => {
                    let mut stream = channel.into_stream();
                    pipe(id, &mut socket, &mut stream).await;
                }
                Err(reason) => {
                    tracing::debug!(forward = id, %peer, target = %host, port, %reason, "could not open the forward's channel");
                }
            }
        }
    })
    .await;
}

fn report_listening(status: &StatusTx, spec: &ForwardSpec, listener: &TcpListener) {
    let (address, port) = listener.local_addr().map_or_else(
        |_| {
            (
                spec.bind_address
                    .clone()
                    .unwrap_or_else(|| "127.0.0.1".to_string()),
                spec.bind_port,
            )
        },
        |local| (local.ip().to_string(), local.port()),
    );
    report(status, spec, ForwardState::Listening { address, port });
}

/// Opens a `direct-tcpip` channel on behalf of the peer at `peer`.
async fn open_channel(
    handle: &Handle<ClientHandler>,
    host: &str,
    port: u16,
    peer: SocketAddr,
) -> Result<russh::Channel<Msg>, OpenError> {
    let open = handle.channel_open_direct_tcpip(
        host.to_string(),
        u32::from(port),
        peer.ip().to_string(),
        u32::from(peer.port()),
    );
    match tokio::time::timeout(OPEN_TIMEOUT, open).await {
        Err(_) => Err(OpenError::TimedOut),
        Ok(Ok(channel)) => Ok(channel),
        Ok(Err(russh::Error::ChannelOpenFailure(reason))) => Err(OpenError::Refused(reason)),
        Ok(Err(error)) => Err(OpenError::Other(error.to_string())),
    }
}

#[derive(Debug)]
enum OpenError {
    TimedOut,
    Refused(ChannelOpenFailure),
    Other(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => f.write_str("the server did not answer in time"),
            Self::Refused(ChannelOpenFailure::AdministrativelyProhibited) => f.write_str(
                "the server does not allow this forward (AllowTcpForwarding / PermitOpen)",
            ),
            Self::Refused(ChannelOpenFailure::ConnectFailed) => {
                f.write_str("the server could not connect to the target")
            }
            Self::Refused(other) => write!(f, "the server refused the channel ({other:?})"),
            Self::Other(detail) => f.write_str(detail),
        }
    }
}

/// Copies both ways until either side ends.
async fn pipe<A, B>(forward: u32, a: &mut A, b: &mut B)
where
    A: AsyncRead + AsyncWrite + Unpin + ?Sized,
    B: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    if let Err(error) = tokio::io::copy_bidirectional(a, b).await {
        // A peer that resets the connection is the normal end of many
        // sessions.
        tracing::debug!(forward, %error, "forwarded connection ended with an error");
    }
}

// ---------- dynamic (SOCKS5) ----------

async fn run_dynamic(spec: &ForwardSpec, handle: Arc<Handle<ClientHandler>>, status: &StatusTx) {
    let listener = match bind(spec).await {
        Ok(listener) => listener,
        Err(reason) => {
            report(status, spec, ForwardState::Failed { reason });
            return;
        }
    };
    report_listening(status, spec, &listener);
    let id = spec.id;
    accept_loop(spec, listener, move |mut socket, peer| {
        let handle = handle.clone();
        async move {
            let target = match tokio::time::timeout(SOCKS_TIMEOUT, socks::negotiate(&mut socket))
                .await
            {
                Ok(Ok(target)) => target,
                Ok(Err(error)) => {
                    tracing::debug!(forward = id, %peer, %error, "SOCKS request refused");
                    return;
                }
                Err(_) => {
                    tracing::debug!(forward = id, %peer, "SOCKS client did not finish its request");
                    return;
                }
            };
            match open_channel(&handle, &target.host, target.port, peer).await {
                Ok(channel) => {
                    if socks::reply(&mut socket, socks::SUCCEEDED).await.is_err() {
                        return;
                    }
                    let mut stream = channel.into_stream();
                    pipe(id, &mut socket, &mut stream).await;
                }
                Err(error) => {
                    tracing::debug!(forward = id, %peer, target = %target.host, port = target.port, %error, "SOCKS target not reachable");
                    let _ = socks::reply(&mut socket, socks::reply_code_for(&error)).await;
                }
            }
        }
    })
    .await;
}

/// A minimal SOCKS5 server side (RFC 1928): no authentication, `CONNECT`
/// only. Generic over the stream so it is tested without sockets.
pub(crate) mod socks {
    use super::{
        AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ChannelOpenFailure, OpenError,
    };

    pub(crate) const SUCCEEDED: u8 = 0x00;
    const GENERAL_FAILURE: u8 = 0x01;
    const NOT_ALLOWED: u8 = 0x02;
    const CONNECTION_REFUSED: u8 = 0x05;
    const COMMAND_NOT_SUPPORTED: u8 = 0x07;
    const ADDRESS_NOT_SUPPORTED: u8 = 0x08;

    /// What a client asked to connect to.
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct Target {
        /// A domain name, or an IP literal as text (IPv6 without brackets).
        pub host: String,
        pub port: u16,
    }

    #[derive(Debug, thiserror::Error)]
    pub(crate) enum SocksError {
        #[error("not a SOCKS5 client")]
        NotSocks5,
        #[error("the client offers no way to proceed without authentication")]
        NoAcceptableMethod,
        #[error("unsupported command {0:#04x} (only CONNECT)")]
        UnsupportedCommand(u8),
        #[error("unsupported address type {0:#04x}")]
        UnsupportedAddress(u8),
        #[error("the target name is not valid text")]
        BadName,
        #[error("{0}")]
        Io(#[from] std::io::Error),
    }

    /// Reads the greeting and the request, answering the greeting (and any
    /// refusal) on `stream`. On success the caller connects and then calls
    /// [`reply`].
    pub(crate) async fn negotiate<S>(stream: &mut S) -> Result<Target, SocksError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        // Greeting: VER NMETHODS METHODS...
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).await?;
        if head[0] != 5 {
            return Err(SocksError::NotSocks5);
        }
        let mut methods = vec![0u8; usize::from(head[1])];
        stream.read_exact(&mut methods).await?;
        if !methods.contains(&0x00) {
            stream.write_all(&[5, 0xFF]).await?;
            return Err(SocksError::NoAcceptableMethod);
        }
        stream.write_all(&[5, 0x00]).await?;

        // Request: VER CMD RSV ATYP ADDR PORT
        let mut request = [0u8; 4];
        stream.read_exact(&mut request).await?;
        if request[0] != 5 {
            return Err(SocksError::NotSocks5);
        }
        if request[1] != 0x01 {
            reply(stream, COMMAND_NOT_SUPPORTED).await?;
            return Err(SocksError::UnsupportedCommand(request[1]));
        }
        let host = match request[3] {
            0x01 => {
                let mut octets = [0u8; 4];
                stream.read_exact(&mut octets).await?;
                std::net::Ipv4Addr::from(octets).to_string()
            }
            0x03 => {
                let mut length = [0u8; 1];
                stream.read_exact(&mut length).await?;
                let mut name = vec![0u8; usize::from(length[0])];
                stream.read_exact(&mut name).await?;
                match String::from_utf8(name) {
                    Ok(name) if !name.is_empty() => name,
                    _ => {
                        reply(stream, ADDRESS_NOT_SUPPORTED).await?;
                        return Err(SocksError::BadName);
                    }
                }
            }
            0x04 => {
                let mut octets = [0u8; 16];
                stream.read_exact(&mut octets).await?;
                std::net::Ipv6Addr::from(octets).to_string()
            }
            other => {
                reply(stream, ADDRESS_NOT_SUPPORTED).await?;
                return Err(SocksError::UnsupportedAddress(other));
            }
        };
        let mut port = [0u8; 2];
        stream.read_exact(&mut port).await?;
        Ok(Target {
            host,
            port: u16::from_be_bytes(port),
        })
    }

    /// Sends the reply with `code`; the bound address is always `0.0.0.0:0`
    /// (the client has no use for the far side's address).
    pub(crate) async fn reply<S>(stream: &mut S, code: u8) -> std::io::Result<()>
    where
        S: AsyncWrite + Unpin,
    {
        stream
            .write_all(&[5, code, 0, 0x01, 0, 0, 0, 0, 0, 0])
            .await
    }

    /// The SOCKS reply for a channel that could not be opened.
    pub(super) fn reply_code_for(error: &OpenError) -> u8 {
        match error {
            OpenError::Refused(ChannelOpenFailure::AdministrativelyProhibited) => NOT_ALLOWED,
            OpenError::Refused(ChannelOpenFailure::ConnectFailed) => CONNECTION_REFUSED,
            OpenError::TimedOut | OpenError::Refused(_) | OpenError::Other(_) => GENERAL_FAILURE,
        }
    }
}

// ---------- listening on the server ----------

async fn run_remote(
    spec: &ForwardSpec,
    handle: &Handle<ClientHandler>,
    remote: &RemoteForwards,
    status: &StatusTx,
) {
    let (Some(host), Some(port)) = (spec.target_host.clone(), spec.target_port) else {
        report(
            status,
            spec,
            ForwardState::Failed {
                reason: "a remote forward needs a target host and port".to_string(),
            },
        );
        return;
    };
    // OpenSSH's default: the server's loopback, unless it is configured to
    // honor a wider address (`GatewayPorts`).
    let address = spec
        .bind_address
        .as_deref()
        .map_or("localhost", strip_brackets)
        .to_string();
    match handle
        .tcpip_forward(address.clone(), u32::from(spec.bind_port))
        .await
    {
        Ok(allocated) => {
            // The server reports a port only when it chose one (port 0).
            let listen_port = if allocated == 0 {
                u32::from(spec.bind_port)
            } else {
                allocated
            };
            remote.register(listen_port, host, port);
            report(
                status,
                spec,
                ForwardState::Listening {
                    address,
                    port: u16::try_from(listen_port).unwrap_or(spec.bind_port),
                },
            );
            // The forward lives as long as the connection; the supervisor
            // is aborted with it.
            std::future::pending::<()>().await;
        }
        Err(russh::Error::RequestDenied) => report(
            status,
            spec,
            ForwardState::Failed {
                reason: format!(
                    "the server refused to listen on {address}:{} (port in use, or forwarding not allowed)",
                    spec.bind_port
                ),
            },
        ),
        Err(error) => report(
            status,
            spec,
            ForwardState::Failed {
                reason: format!("the listen request failed: {error}"),
            },
        ),
    }
}

/// Serves one channel the server opened for a remote forward: connect to the
/// local target first, then confirm the channel (so a target that is down is
/// a refused channel, not an accepted one that dies at once).
pub(crate) async fn serve_remote(
    channel: russh::Channel<Msg>,
    reply: ChannelOpenHandle,
    target: (String, u16),
) {
    let (host, port) = target;
    let connect = TcpStream::connect((strip_brackets(&host).to_string(), port));
    match tokio::time::timeout(LOCAL_CONNECT_TIMEOUT, connect).await {
        Ok(Ok(mut socket)) => {
            reply.accept().await;
            let mut stream = channel.into_stream();
            pipe(0, &mut socket, &mut stream).await;
        }
        Ok(Err(error)) => {
            tracing::debug!(target = %host, port, %error, "the target of a remote forward refused the connection");
            reply.reject(ChannelOpenFailure::ConnectFailed).await;
        }
        Err(_) => {
            tracing::debug!(target = %host, port, "the target of a remote forward did not answer");
            reply.reject(ChannelOpenFailure::ConnectFailed).await;
        }
    }
}

// ---------- agent forwarding ----------

/// Serves one `auth-agent` channel the server opened: connects to the local
/// agent and copies both ways. Refused when no agent is reachable.
pub(crate) async fn serve_agent(
    channel: russh::Channel<Msg>,
    reply: ChannelOpenHandle,
    socket: Option<PathBuf>,
) {
    match connect_agent(socket.as_deref()).await {
        Ok(mut agent) => {
            reply.accept().await;
            let mut stream = channel.into_stream();
            pipe(0, &mut agent, &mut stream).await;
        }
        Err(error) => {
            tracing::debug!(%error, "agent forwarding: the local agent is not reachable");
            reply.reject(ChannelOpenFailure::ConnectFailed).await;
        }
    }
}

trait AgentStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AgentStream for T {}

#[cfg(unix)]
async fn connect_agent(socket: Option<&Path>) -> std::io::Result<Box<dyn AgentStream>> {
    let path = match socket {
        Some(path) => path.to_path_buf(),
        None => std::env::var_os("SSH_AUTH_SOCK")
            .map(PathBuf::from)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "SSH_AUTH_SOCK is not set")
            })?,
    };
    Ok(Box::new(tokio::net::UnixStream::connect(path).await?))
}

/// The OpenSSH agent of Windows listens on a named pipe (Pageant, the other
/// agent the engine can authenticate with, has no stream to proxy).
#[cfg(windows)]
async fn connect_agent(socket: Option<&Path>) -> std::io::Result<Box<dyn AgentStream>> {
    const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
    let pipe = socket
        .filter(|path| path.to_string_lossy().starts_with(r"\\.\pipe\"))
        .map_or_else(
            || std::ffi::OsString::from(OPENSSH_PIPE),
            |path| path.as_os_str().to_os_string(),
        );
    Ok(Box::new(
        tokio::net::windows::named_pipe::ClientOptions::new().open(pipe)?,
    ))
}

#[cfg(not(any(unix, windows)))]
async fn connect_agent(_socket: Option<&Path>) -> std::io::Result<Box<dyn AgentStream>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "agent forwarding is not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn negotiate_with(bytes: &[u8]) -> (Result<socks::Target, socks::SocksError>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        client.write_all(bytes).await.expect("write");
        client.shutdown().await.expect("shutdown");
        let outcome = socks::negotiate(&mut server).await;
        drop(server);
        let mut answered = Vec::new();
        client.read_to_end(&mut answered).await.expect("read");
        (outcome, answered)
    }

    fn greeting() -> Vec<u8> {
        vec![5, 1, 0]
    }

    #[tokio::test]
    async fn socks_connect_to_a_domain_name() {
        let mut bytes = greeting();
        bytes.extend([5, 1, 0, 3, 11]);
        bytes.extend(b"example.com");
        bytes.extend(443u16.to_be_bytes());
        let (outcome, answered) = negotiate_with(&bytes).await;
        assert_eq!(
            outcome.expect("valid request"),
            socks::Target {
                host: "example.com".into(),
                port: 443
            }
        );
        // The greeting was answered with "no authentication".
        assert_eq!(answered, vec![5, 0]);
    }

    #[tokio::test]
    async fn socks_connect_to_ipv4_and_ipv6() {
        let mut bytes = greeting();
        bytes.extend([5, 1, 0, 1, 192, 0, 2, 7]);
        bytes.extend(8080u16.to_be_bytes());
        let (outcome, _) = negotiate_with(&bytes).await;
        assert_eq!(
            outcome.expect("ipv4"),
            socks::Target {
                host: "192.0.2.7".into(),
                port: 8080
            }
        );

        let mut bytes = greeting();
        bytes.extend([5, 1, 0, 4]);
        bytes.extend(std::net::Ipv6Addr::LOCALHOST.octets());
        bytes.extend(22u16.to_be_bytes());
        let (outcome, _) = negotiate_with(&bytes).await;
        assert_eq!(
            outcome.expect("ipv6"),
            socks::Target {
                host: "::1".into(),
                port: 22
            }
        );
    }

    #[tokio::test]
    async fn socks_refuses_what_it_does_not_do() {
        // BIND (command 2) and UDP ASSOCIATE (3).
        for command in [2u8, 3] {
            let mut bytes = greeting();
            bytes.extend([5, command, 0, 1, 1, 2, 3, 4, 0, 80]);
            let (outcome, answered) = negotiate_with(&bytes).await;
            assert!(matches!(
                outcome,
                Err(socks::SocksError::UnsupportedCommand(c)) if c == command
            ));
            // method reply, then "command not supported".
            assert_eq!(&answered[..2], &[5, 0]);
            assert_eq!(answered[3], 7, "reply code: {answered:?}");
        }
        // Unknown address type.
        let mut bytes = greeting();
        bytes.extend([5, 1, 0, 9]);
        let (outcome, answered) = negotiate_with(&bytes).await;
        assert!(matches!(
            outcome,
            Err(socks::SocksError::UnsupportedAddress(9))
        ));
        assert_eq!(answered[3], 8, "address type not supported: {answered:?}");
        // An empty domain name.
        let mut bytes = greeting();
        bytes.extend([5, 1, 0, 3, 0, 0, 80]);
        let (outcome, _) = negotiate_with(&bytes).await;
        assert!(matches!(outcome, Err(socks::SocksError::BadName)));
    }

    #[tokio::test]
    async fn socks_needs_the_no_authentication_method() {
        let (outcome, answered) = negotiate_with(&[5, 1, 2]).await;
        assert!(matches!(
            outcome,
            Err(socks::SocksError::NoAcceptableMethod)
        ));
        assert_eq!(answered, vec![5, 0xFF]);
    }

    #[tokio::test]
    async fn socks_ignores_other_protocols_and_truncated_requests() {
        // An HTTP client talking to the SOCKS port.
        let (outcome, _) = negotiate_with(b"GET / HTTP/1.1\r\n\r\n").await;
        assert!(matches!(outcome, Err(socks::SocksError::NotSocks5)));
        // SOCKS4.
        let (outcome, _) = negotiate_with(&[4, 1, 0, 80, 1, 2, 3, 4, 0]).await;
        assert!(matches!(outcome, Err(socks::SocksError::NotSocks5)));
        // A request cut off in the middle of the address.
        let mut bytes = greeting();
        bytes.extend([5, 1, 0, 1, 10, 0]);
        let (outcome, _) = negotiate_with(&bytes).await;
        assert!(matches!(outcome, Err(socks::SocksError::Io(_))));
    }

    #[tokio::test]
    async fn socks_reply_has_the_fixed_layout() {
        let (mut client, mut server) = duplex(64);
        socks::reply(&mut server, socks::SUCCEEDED)
            .await
            .expect("reply");
        drop(server);
        let mut answered = Vec::new();
        client.read_to_end(&mut answered).await.expect("read");
        assert_eq!(answered, vec![5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn channel_refusals_map_to_socks_codes() {
        assert_eq!(
            socks::reply_code_for(&OpenError::Refused(
                ChannelOpenFailure::AdministrativelyProhibited
            )),
            0x02
        );
        assert_eq!(
            socks::reply_code_for(&OpenError::Refused(ChannelOpenFailure::ConnectFailed)),
            0x05
        );
        assert_eq!(socks::reply_code_for(&OpenError::TimedOut), 0x01);
    }

    #[test]
    fn remote_targets_are_found_by_listening_port() {
        let forwards = RemoteForwards::default();
        assert!(forwards.lookup(9000).is_none());
        forwards.register(9000, "127.0.0.1".into(), 3000);
        assert_eq!(forwards.lookup(9000), Some(("127.0.0.1".into(), 3000)));
        assert!(forwards.lookup(9001).is_none());
    }

    #[tokio::test]
    async fn binding_defaults_to_loopback_and_reports_why_it_failed() {
        let spec = ForwardSpec {
            id: 1,
            kind: ForwardKind::Local,
            bind_address: None,
            bind_port: 0,
            target_host: Some("db".into()),
            target_port: Some(1),
        };
        let listener = bind(&spec).await.expect("bind");
        let local = listener.local_addr().expect("addr");
        assert!(local.ip().is_loopback(), "{local}");

        // The same port again: a sentence, not an io error code.
        let taken = ForwardSpec {
            bind_port: local.port(),
            ..spec.clone()
        };
        let reason = bind(&taken).await.expect_err("port is taken");
        assert!(reason.contains("already in use"), "{reason}");
        assert!(reason.contains(&local.port().to_string()), "{reason}");

        // `localhost` and bracketed forms are understood.
        let named = ForwardSpec {
            bind_address: Some("localhost".into()),
            bind_port: 0,
            ..spec.clone()
        };
        assert!(
            bind(&named)
                .await
                .expect("localhost")
                .local_addr()
                .expect("a")
                .ip()
                .is_loopback()
        );
        let v6 = ForwardSpec {
            bind_address: Some("[::1]".into()),
            bind_port: 0,
            ..spec
        };
        // IPv6 loopback may be disabled on a build machine: only the
        // spelling is under test, so either outcome but a parse error is fine.
        if let Err(reason) = bind(&v6).await {
            assert!(!reason.contains("invalid"), "{reason}");
        }
    }
}
