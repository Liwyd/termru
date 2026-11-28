//! Connecting through jump hosts (`ProxyJump`).
//!
//! The first hop is dialed like any host. Every later hop (and finally the
//! target) is reached through a `direct-tcpip` channel opened on the hop
//! before it, and that channel, turned into a stream, carries a complete SSH
//! connection of its own (`russh::client::connect_stream`). Nothing about the
//! inner connection is shared with the outer one:
//!
//! * **every hop is verified** — its own host-key check (known_hosts, the
//!   read-only extra files, the trust prompt) against the address *that hop
//!   was asked to reach*, so a jump host cannot vouch for what lies behind it;
//! * **every hop authenticates** with its own chain and its own username;
//! * each hop has its own keepalive, which matters most for the last one:
//!   the keepalives of the tunneled connection travel inside the outer ones.
//!
//! One cancellation signal covers the whole chain: it aborts a dial, a
//! handshake, a prompt, an authentication or a channel open on any hop at
//! once. Each hop runs under its own connect budget and authentication
//! timeout (a human decision is never charged to either, see `dial`), so the
//! worst case is that of every hop in turn, and an error names the hop it
//! happened on ([`SshError::Hop`]).
//!
//! A jump host is only a relay: it gets no environment, no forwards, no
//! startup input and, above all, **never the agent** — the target alone may
//! be allowed to use the user's agent.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Config as ClientConfig, Handle};
use termru_core::validate::strip_brackets;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};

use crate::auth::{AuthContext, authenticate};
use crate::config::{AgentForward, ConnectParams};
use crate::dial::{Budget, dial};
use crate::error::SshError;
use crate::forward::RemoteForwards;
use crate::known_hosts::{KnownHosts, preferred_host_key_algorithms};
use crate::session::{
    BannerSink, ClientHandler, DEFAULT_KEEPALIVE_MAX, SessionEvent, SessionStatus, blocking,
    forward_banner, map_connect_error, take_recorded,
};

/// Upper bound for the polite part of closing connections that were opened
/// for a connect that did not complete.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// What a connect attempt shares across its hops.
pub(crate) struct Progress<'a> {
    pub events: &'a mpsc::Sender<SessionEvent>,
    pub cancel: &'a mut watch::Receiver<bool>,
    /// Raised while a human decides about a host key or answers a question;
    /// the clocks of the hop being connected stand still.
    pub prompt_active: &'a Arc<AtomicBool>,
    pub banner_tx: &'a mpsc::UnboundedSender<String>,
    pub banner_rx: &'a mut mpsc::UnboundedReceiver<String>,
}

/// What only the final hop gets.
pub(crate) struct TargetExtras {
    pub remote_forwards: Arc<RemoteForwards>,
    pub agent_forward: Option<AgentForward>,
}

/// The connections of an established chain.
pub(crate) struct Chain {
    /// The connection to the target (the last one).
    pub target: Handle<ClientHandler>,
    /// The connections to the jump hosts, first hop first. They carry the
    /// target's connection and must stay open as long as it does.
    pub jump: Vec<Handle<ClientHandler>>,
}

/// Connects and authenticates every hop and then the target.
///
/// An error of a jump host is wrapped in [`SshError::Hop`]; one of the target
/// is returned as it is (it names the target already). Cancellation is
/// `SessionClosing`, unwrapped, so the retry loop recognizes it. Whatever was
/// opened before a failure is closed.
pub(crate) async fn connect_chain(
    params: &ConnectParams,
    extras: TargetExtras,
    progress: &mut Progress<'_>,
) -> Result<Chain, SshError> {
    let hops = &params.setup.jump;
    let total = hops.len();
    let mut jump: Vec<Handle<ClientHandler>> = Vec::with_capacity(total);

    for (position, hop) in hops.iter().enumerate() {
        match connect_hop(&hop.params, jump.last(), None, progress).await {
            Ok(handle) => jump.push(handle),
            Err(error) => {
                close_all(&jump).await;
                return Err(match error {
                    SshError::SessionClosing => SshError::SessionClosing,
                    source => SshError::Hop {
                        index: position + 1,
                        total,
                        label: hop.label.clone(),
                        source: Box::new(source),
                    },
                });
            }
        }
    }

    match connect_hop(params, jump.last(), Some(extras), progress).await {
        Ok(target) => Ok(Chain { target, jump }),
        Err(error) => {
            close_all(&jump).await;
            Err(error)
        }
    }
}

/// Disconnects `handles`, best effort and bounded.
pub(crate) async fn close_all(handles: &[Handle<ClientHandler>]) {
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, async {
        for handle in handles.iter().rev() {
            let _ = handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "Termru session closed",
                    "en",
                )
                .await;
        }
    })
    .await;
}

/// One hop: transport (dial, or a channel through `via`), key exchange with
/// the host-key decision, and authentication.
async fn connect_hop(
    hop: &ConnectParams,
    via: Option<&Handle<ClientHandler>>,
    extras: Option<TargetExtras>,
    progress: &mut Progress<'_>,
) -> Result<Handle<ClientHandler>, SshError> {
    let _ = progress
        .events
        .send(SessionEvent::Status(SessionStatus::Connecting))
        .await;
    let host = strip_brackets(&hop.host).to_string();
    let keepalive = hop.effective_keepalive();
    let known_hosts = Arc::new(
        KnownHosts::with_path(hop.known_hosts_path.clone())
            .with_readonly(hop.extra_known_hosts.clone()),
    );
    let recorded: Arc<Mutex<Option<SshError>>> = Arc::new(Mutex::new(None));

    // One deadline for DNS + TCP + key exchange of this hop. Time spent on a
    // host-key prompt is not charged (the budget pauses), and authentication
    // has its own timeout below.
    let mut budget = Budget::new(hop.timeouts.connect);
    let connect_timeout = || SshError::ConnectTimeout {
        host: host.clone(),
        port: hop.port,
        seconds: hop.timeouts.connect.as_secs(),
    };

    // Ask for the key types we already know this host under first: a server
    // with several host keys then presents one we can verify, instead of its
    // own favourite that would look like a new host.
    let known_types = {
        let known_hosts = known_hosts.clone();
        let (host, port) = (host.clone(), hop.port);
        blocking(move || known_hosts.known_key_types(&host, port))
            .await
            .unwrap_or_default()
    };
    let (remote_forwards, agent_forward) = match extras {
        Some(extras) => (extras.remote_forwards, extras.agent_forward),
        None => (Arc::new(RemoteForwards::default()), None),
    };
    let handler = ClientHandler::new(
        host.clone(),
        hop.port,
        known_hosts,
        hop.host_key_prompt.clone(),
        hop.timeouts.host_key_decision,
        progress.prompt_active.clone(),
        recorded.clone(),
        BannerSink::new(progress.banner_tx.clone()),
        remote_forwards,
        agent_forward,
    );
    let config = ClientConfig {
        // `SshId::Standard` writes the value verbatim and appends `\r\n`;
        // it MUST include the `SSH-2.0-` prefix or OpenSSH desyncs the
        // banner exchange (caught by the sshd integration suite).
        client_id: russh::SshId::Standard(std::borrow::Cow::Owned(format!(
            "SSH-2.0-Termru_{}",
            env!("CARGO_PKG_VERSION")
        ))),
        // russh's own keepalive is the only one: no second timer in the
        // actor (two timers double the traffic and race each other).
        keepalive_interval: Some(keepalive),
        keepalive_max: DEFAULT_KEEPALIVE_MAX,
        inactivity_timeout: None,
        // `connect_stream` does not tune the socket, `dial` already did;
        // this keeps russh's own view consistent.
        nodelay: true,
        preferred: russh::Preferred {
            key: std::borrow::Cow::Owned(preferred_host_key_algorithms(&known_types)),
            ..russh::Preferred::default()
        },
        ..ClientConfig::default()
    };

    let mut handle = match via {
        None => {
            let stream = dial(&host, hop.port, &mut budget, progress.cancel, keepalive).await?;
            handshake(
                stream,
                config,
                handler,
                hop,
                &recorded,
                &mut budget,
                progress,
            )
            .await?
        }
        Some(previous) => {
            let channel = budget
                .run(
                    async {
                        previous
                            .channel_open_direct_tcpip(
                                host.clone(),
                                u32::from(hop.port),
                                "127.0.0.1",
                                0,
                            )
                            .await
                            .map_err(|error| map_channel_error(&error, &host, hop.port))
                    },
                    None,
                    progress.cancel,
                    connect_timeout,
                )
                .await?;
            handshake(
                channel.into_stream(),
                config,
                handler,
                hop,
                &recorded,
                &mut budget,
                progress,
            )
            .await?
        }
    };

    authenticate_hop(&mut handle, hop, progress).await?;
    Ok(handle)
}

/// Key exchange (and the host-key decision) over `stream`.
async fn handshake<R>(
    stream: R,
    config: ClientConfig,
    handler: ClientHandler,
    hop: &ConnectParams,
    recorded: &Arc<Mutex<Option<SshError>>>,
    budget: &mut Budget,
    progress: &mut Progress<'_>,
) -> Result<Handle<ClientHandler>, SshError>
where
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let host = strip_brackets(&hop.host).to_string();
    let seconds = hop.timeouts.connect.as_secs();
    let work = async {
        client::connect_stream(Arc::new(config), stream, handler)
            .await
            .map_err(|error| {
                take_recorded(recorded)
                    .unwrap_or_else(|| map_connect_error(error, &host, hop.port, seconds))
            })
    };
    budget
        .run(work, Some(progress.prompt_active), progress.cancel, || {
            take_recorded(recorded).unwrap_or_else(|| SshError::ConnectTimeout {
                host: host.clone(),
                port: hop.port,
                seconds,
            })
        })
        .await
}

async fn authenticate_hop(
    handle: &mut Handle<ClientHandler>,
    hop: &ConnectParams,
    progress: &mut Progress<'_>,
) -> Result<(), SshError> {
    let _ = progress
        .events
        .send(SessionEvent::Status(SessionStatus::Authenticating))
        .await;
    let context = AuthContext {
        username: &hop.username,
        methods: &hop.auth,
        prompt: hop.auth_prompt.as_ref(),
        prompt_active: progress.prompt_active,
        prompt_timeout: hop.timeouts.auth_prompt,
        notices: progress.banner_tx,
    };
    // The deadline stands still while a human answers a question.
    let mut auth_budget = Budget::new(hop.timeouts.auth);
    let outcome = {
        let work = auth_budget.run(
            authenticate(handle, &context),
            Some(progress.prompt_active),
            progress.cancel,
            || SshError::AuthTimeout,
        );
        tokio::pin!(work);
        loop {
            tokio::select! {
                result = &mut work => break result,
                Some(text) = progress.banner_rx.recv() => {
                    forward_banner(progress.events, text).await;
                }
            }
        }
    };
    // What the server said last still reaches the user, also when the
    // authentication failed (that is when the text matters most).
    while let Ok(text) = progress.banner_rx.try_recv() {
        forward_banner(progress.events, text).await;
    }
    outcome
}

/// A refused or failed `direct-tcpip` open, as the error of the hop that
/// should have been reached through it.
fn map_channel_error(error: &russh::Error, host: &str, port: u16) -> SshError {
    match error {
        russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::AdministrativelyProhibited) => {
            // Retrying cannot change the administrator's mind.
            SshError::Protocol(format!(
                "the previous host does not allow forwarding to {host}:{port} \
                 (AllowTcpForwarding / PermitOpen)"
            ))
        }
        russh::Error::ChannelOpenFailure(reason) => SshError::ConnectFailed {
            host: host.to_string(),
            port,
            detail: format!("the previous host could not connect ({reason:?})"),
        },
        russh::Error::Disconnect | russh::Error::SendError | russh::Error::HUP => {
            SshError::ConnectionClosed
        }
        other => SshError::Protocol(format!(
            "could not open a channel to {host}:{port}: {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forwarding_ban_is_final_and_a_failed_connect_is_retriable() {
        let banned = map_channel_error(
            &russh::Error::ChannelOpenFailure(
                russh::ChannelOpenFailure::AdministrativelyProhibited,
            ),
            "inner",
            22,
        );
        assert!(!banned.is_retriable(), "{banned}");
        assert!(
            banned.to_string().contains("AllowTcpForwarding"),
            "{banned}"
        );

        let unreachable = map_channel_error(
            &russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed),
            "inner",
            22,
        );
        assert!(unreachable.is_retriable(), "{unreachable}");
        assert!(
            unreachable.to_string().contains("inner:22"),
            "{unreachable}"
        );

        assert!(matches!(
            map_channel_error(&russh::Error::Disconnect, "inner", 22),
            SshError::ConnectionClosed
        ));
    }
}
