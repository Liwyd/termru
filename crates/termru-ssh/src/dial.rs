//! Connection establishment: one overall deadline, name resolution, and
//! staggered ("Happy Eyeballs") TCP connects.
//!
//! The connect phases (DNS, TCP, key exchange, shell setup) share a single
//! [`Budget`] instead of each getting a fresh timeout, so the worst case is the
//! configured budget and not a multiple of it. The budget is *paused* while a
//! human decides about a host key, and it does not tick during authentication
//! (which has its own timeout): time the user spends is never charged to the
//! network.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::error::SshError;
use crate::reconnect::cancelled;

/// Delay before the next address is tried while an earlier one is still
/// connecting (RFC 8305 recommends 250 ms).
pub(crate) const STAGGER: Duration = Duration::from_millis(250);

/// Granularity at which a paused budget notices that the pause ended.
const TICK: Duration = Duration::from_millis(50);

/// Interval between TCP-level keepalive probes once the connection is idle.
const TCP_KEEPALIVE_PROBE_INTERVAL: Duration = Duration::from_secs(10);

/// What is left of the overall connect deadline.
#[derive(Debug)]
pub(crate) struct Budget {
    total: Duration,
    remaining: Duration,
}

impl Budget {
    pub(crate) fn new(total: Duration) -> Self {
        Self {
            total,
            remaining: total,
        }
    }

    pub(crate) fn total(&self) -> Duration {
        self.total
    }

    /// Runs `work` against the remaining budget.
    ///
    /// * `paused` (when given and `true`) stops the clock — used while a
    ///   host-key prompt waits for the user;
    /// * `cancel` aborts the work at once with [`SshError::SessionClosing`];
    /// * on expiry the work is dropped and `on_timeout()` is returned.
    pub(crate) async fn run<T>(
        &mut self,
        work: impl Future<Output = Result<T, SshError>>,
        paused: Option<&AtomicBool>,
        cancel: &mut watch::Receiver<bool>,
        on_timeout: impl FnOnce() -> SshError,
    ) -> Result<T, SshError> {
        let mut work = std::pin::pin!(work);
        let mut last = Instant::now();
        loop {
            if self.remaining.is_zero() {
                return Err(on_timeout());
            }
            let slice = self.remaining.min(TICK);
            tokio::select! {
                result = &mut work => {
                    self.charge(last.elapsed(), paused);
                    return result;
                }
                () = cancelled(cancel) => return Err(SshError::SessionClosing),
                () = tokio::time::sleep(slice) => {
                    let now = Instant::now();
                    self.charge(now - last, paused);
                    last = now;
                }
            }
        }
    }

    fn charge(&mut self, elapsed: Duration, paused: Option<&AtomicBool>) {
        if paused.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            return;
        }
        self.remaining = self.remaining.saturating_sub(elapsed);
    }
}

/// Resolves `host`, races its addresses and returns the first stream that
/// connected, tuned for interactive use (`TCP_NODELAY`, TCP keepalive).
///
/// Everything here runs under `budget` and `cancel`: a black-holed DNS server
/// or an unreachable first address cannot hang the connect past the deadline.
pub(crate) async fn dial(
    host: &str,
    port: u16,
    budget: &mut Budget,
    cancel: &mut watch::Receiver<bool>,
    tcp_keepalive: Duration,
) -> Result<TcpStream, SshError> {
    let total = budget.total();
    let stream = budget
        .run(resolve_and_connect(host, port), None, cancel, || {
            SshError::ConnectTimeout {
                host: host.to_string(),
                port,
                seconds: total.as_secs(),
            }
        })
        .await?;
    tune_socket(&stream, tcp_keepalive);
    Ok(stream)
}

async fn resolve_and_connect(host: &str, port: u16) -> Result<TcpStream, SshError> {
    let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| SshError::DnsFailure {
            host: host.to_string(),
            detail: e.to_string(),
        })?
        .collect();
    let addrs = interleave_families(resolved);
    if addrs.is_empty() {
        return Err(SshError::ConnectFailed {
            host: host.to_string(),
            port,
            detail: "hostname resolved to no addresses".to_string(),
        });
    }
    match race(&addrs, STAGGER, |addr| async move {
        TcpStream::connect(addr).await
    })
    .await
    {
        Ok(stream) => Ok(stream),
        Err(errors) => Err(SshError::ConnectFailed {
            host: host.to_string(),
            port,
            detail: errors.last().map_or_else(
                || "no address accepted the connection".to_string(),
                ToString::to_string,
            ),
        }),
    }
}

/// Interactive-latency and dead-peer settings of the TCP socket.
///
/// Failures are logged, not fatal: the connection works without them, only
/// less well.
fn tune_socket(stream: &TcpStream, keepalive: Duration) {
    // Without this, Nagle's algorithm holds back small writes (keystrokes)
    // while an earlier segment is unacknowledged, which feels like lag.
    if let Err(error) = stream.set_nodelay(true) {
        tracing::warn!("could not set TCP_NODELAY: {error}");
    }
    // OS-level probes complement the SSH keepalive: they keep NAT/firewall
    // state alive and let the kernel report a dead peer on its own.
    let probe = socket2::TcpKeepalive::new()
        .with_time(keepalive)
        .with_interval(TCP_KEEPALIVE_PROBE_INTERVAL.min(keepalive));
    if let Err(error) = socket2::SockRef::from(stream).set_tcp_keepalive(&probe) {
        tracing::warn!("could not enable TCP keepalive: {error}");
    }
}

/// Orders addresses for connecting: the family of the first resolved address
/// first (the resolver already ranked it), then alternating families so one
/// broken family cannot hold up the other; duplicates are dropped.
pub(crate) fn interleave_families(resolved: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let mut unique: Vec<SocketAddr> = Vec::with_capacity(resolved.len());
    for addr in resolved {
        if !unique.contains(&addr) {
            unique.push(addr);
        }
    }
    let first_is_v6 = unique.first().is_some_and(SocketAddr::is_ipv6);
    let (primary, secondary): (Vec<SocketAddr>, Vec<SocketAddr>) = unique
        .into_iter()
        .partition(|addr| addr.is_ipv6() == first_is_v6);
    let mut primary = primary.into_iter();
    let mut secondary = secondary.into_iter();
    let mut ordered = Vec::new();
    loop {
        match (primary.next(), secondary.next()) {
            (None, None) => break,
            (a, b) => ordered.extend(a.into_iter().chain(b)),
        }
    }
    ordered
}

/// Staggered connect race: starts the first address at once and the next one
/// every `stagger` (or immediately after an attempt failed); the first
/// success wins and the losers are dropped (cancelled). When every address
/// failed, all errors are returned in completion order.
pub(crate) async fn race<T, E, F, Fut>(
    addrs: &[SocketAddr],
    stagger: Duration,
    mut connect: F,
) -> Result<T, Vec<E>>
where
    F: FnMut(SocketAddr) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    if addrs.is_empty() {
        return Err(Vec::new());
    }
    let mut inflight = FuturesUnordered::new();
    let mut errors: Vec<E> = Vec::new();
    let mut next = 0usize;
    let timer = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(timer);
    loop {
        tokio::select! {
            () = &mut timer, if next < addrs.len() => {
                inflight.push(connect(addrs[next]));
                next += 1;
                timer.as_mut().reset(Instant::now() + stagger);
            }
            outcome = inflight.next(), if !inflight.is_empty() => {
                match outcome {
                    Some(Ok(value)) => return Ok(value),
                    Some(Err(error)) => {
                        errors.push(error);
                        if next < addrs.len() {
                            // A refused address must not delay the next one.
                            timer.as_mut().reset(Instant::now());
                        } else if inflight.is_empty() {
                            return Err(errors);
                        }
                    }
                    None => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn v4(last: u8) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::new(192, 0, 2, last), 22))
    }

    fn v6(last: u16) -> SocketAddr {
        SocketAddr::from((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, last), 22))
    }

    #[test]
    fn families_alternate_starting_with_the_resolvers_choice() {
        let ordered = interleave_families(vec![v6(1), v6(2), v4(1), v4(2), v4(3), v4(1)]);
        assert_eq!(ordered, vec![v6(1), v4(1), v6(2), v4(2), v4(3)]);
        let ordered = interleave_families(vec![v4(1), v6(1)]);
        assert_eq!(ordered, vec![v4(1), v6(1)]);
        assert!(interleave_families(Vec::new()).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_first_address_does_not_block_the_second() {
        let started = Instant::now();
        let winner = race(&[v4(1), v4(2)], STAGGER, |addr| async move {
            if addr == v4(1) {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            Ok::<_, std::io::Error>(addr)
        })
        .await
        .expect("the second address connects");
        assert_eq!(winner, v4(2));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the second address starts after the stagger, not after the first fails"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_address_starts_the_next_one_immediately() {
        let started = Instant::now();
        let winner = race(
            &[v4(1), v4(2)],
            Duration::from_secs(30),
            |addr| async move {
                if addr == v4(1) {
                    Err(std::io::Error::from(std::io::ErrorKind::ConnectionRefused))
                } else {
                    Ok(addr)
                }
            },
        )
        .await
        .expect("the second address connects");
        assert_eq!(winner, v4(2));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn every_failure_is_reported_when_nothing_connects() {
        let errors = race(&[v4(1), v4(2), v4(3)], STAGGER, |_| async {
            Err::<(), _>(std::io::Error::from(std::io::ErrorKind::ConnectionRefused))
        })
        .await
        .expect_err("all addresses fail");
        assert_eq!(errors.len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn budget_expires_and_reports_the_timeout() {
        let (_tx, mut cancel) = watch::channel(false);
        let mut budget = Budget::new(Duration::from_secs(2));
        let error = budget
            .run(
                std::future::pending::<Result<(), SshError>>(),
                None,
                &mut cancel,
                || SshError::AuthTimeout,
            )
            .await
            .expect_err("pending work must time out");
        assert!(matches!(error, SshError::AuthTimeout), "got {error:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn budget_does_not_tick_while_paused() {
        let (_tx, mut cancel) = watch::channel(false);
        let paused = AtomicBool::new(true);
        let mut budget = Budget::new(Duration::from_secs(2));
        // A "user decision" of 10 s, far beyond the 2 s budget, while paused.
        let result = budget
            .run(
                async {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    paused.store(false, Ordering::SeqCst);
                    Ok::<_, SshError>(7)
                },
                Some(&paused),
                &mut cancel,
                || SshError::AuthTimeout,
            )
            .await;
        assert_eq!(result.expect("paused time is free"), 7);
        // The whole budget is still available afterwards.
        assert!(budget.remaining >= Duration::from_millis(1900));
    }

    #[tokio::test(start_paused = true)]
    async fn budget_honours_cancellation() {
        let (tx, mut cancel) = watch::channel(false);
        let mut budget = Budget::new(Duration::from_secs(60));
        tx.send(true).expect("receiver alive");
        let error = budget
            .run(
                std::future::pending::<Result<(), SshError>>(),
                None,
                &mut cancel,
                || SshError::AuthTimeout,
            )
            .await
            .expect_err("cancelled");
        assert!(matches!(error, SshError::SessionClosing), "got {error:?}");
    }

    /// Regression test: the dialed socket must have `TCP_NODELAY` (Nagle
    /// otherwise delays keystrokes) and TCP keepalive enabled.
    #[tokio::test]
    async fn dialed_socket_has_nodelay_and_keepalive() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (_tx, mut cancel) = watch::channel(false);
        let mut budget = Budget::new(Duration::from_secs(5));
        let stream = dial(
            "127.0.0.1",
            port,
            &mut budget,
            &mut cancel,
            Duration::from_secs(30),
        )
        .await
        .expect("dial");
        assert!(stream.nodelay().expect("nodelay readable"), "TCP_NODELAY");
        assert!(
            socket2::SockRef::from(&stream)
                .keepalive()
                .expect("keepalive readable"),
            "SO_KEEPALIVE"
        );
    }

    #[tokio::test]
    async fn dial_reports_dns_failure_for_invalid_name() {
        let (_tx, mut cancel) = watch::channel(false);
        let mut budget = Budget::new(Duration::from_secs(3));
        let error = dial(
            "definitely.invalid.invalid",
            22,
            &mut budget,
            &mut cancel,
            Duration::from_secs(30),
        )
        .await
        .expect_err("must fail");
        assert!(
            matches!(error, SshError::DnsFailure { .. }),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn dial_aborts_immediately_when_cancelled() {
        let (_cancel_tx, mut cancel) = watch::channel(true);
        let mut budget = Budget::new(Duration::from_secs(60));
        let started = Instant::now();
        // Unroutable test address: black-holes SYNs in most environments, so
        // only cancellation can end this quickly.
        let result = dial(
            "10.255.255.1",
            22,
            &mut budget,
            &mut cancel,
            Duration::from_secs(30),
        )
        .await;
        assert!(matches!(result, Err(SshError::SessionClosing)));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "cancel must beat the 60s budget"
        );
    }
}
