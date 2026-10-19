//! Bounded automatic reconnection.
//!
//! Design (see `docs/architecture/0002-ssh-transport.md`):
//!
//! * Only **retriable** failures are retried: [`SshError::is_retriable`],
//!   i.e. network/timeout failures. Authentication, host-key, protocol and
//!   local failures are never retried automatically.
//! * Retries are **bounded**: a fixed maximum number of total attempts and a
//!   capped exponential backoff. Exhaustion returns the last error to the
//!   caller, which is the "explicit user action required" state.
//! * Every backoff wait is **cancellable** (user closed the tab, app
//!   quitting); cancellation also aborts in-flight connection attempts
//!   (dial, handshake, host-key prompt, authentication).
//! * The policy is pure ([`ReconnectPolicy::decide`]) so scheduling is unit
//!   testable without time; the loop is driven by [`connect_with_retry`].
//!
//! The desktop app uses the same policy for *mid-session* drops: when a
//! session reports `Failed { category: Network | Timeout }` it re-enters
//! [`crate::session::Session::open_with_reconnect`].

use std::future::Future;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::error::SshError;
use crate::session::{ErrorCategory, SessionEvent, SessionStatus};

/// Backoff floor. Even a misconfigured policy cannot produce a hot loop.
const MIN_DELAY: Duration = Duration::from_millis(100);

/// How automatic reconnection behaves. All fields are public so the app can
/// expose them as preferences; construction goes through the constructors to
/// keep invariants (non-zero attempts, sane multiplier).
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    /// Total connection attempts, including the first one. `< 1` is treated
    /// as `1` (no retries).
    pub max_attempts: u32,
    /// Delay after the first failed attempt; doubled per subsequent failure.
    pub initial_delay: Duration,
    /// Upper bound for a single backoff delay.
    pub max_delay: Duration,
    /// Growth factor between attempts. `>= 1.0`.
    pub multiplier: f64,
    /// Randomization of each delay in `[0.0, 1.0]` (± ratio), so many
    /// sessions recovering from the same outage do not retry in lockstep.
    pub jitter_ratio: f64,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 8,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            jitter_ratio: 0.2,
        }
    }
}

impl ReconnectPolicy {
    /// Single attempt, no retries: used by [`crate::session::Session::open`].
    #[must_use]
    pub fn no_retries() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }

    /// Verdict for `attempt` (1-based, the attempt that just failed).
    #[must_use]
    pub fn decide(&self, attempt: u32, error: &SshError) -> ReconnectDecision {
        if !error.is_retriable() {
            return ReconnectDecision::GiveUp {
                reason: GiveUpReason::NotRetriable,
            };
        }
        if attempt >= self.max_attempts.max(1) {
            return ReconnectDecision::GiveUp {
                reason: GiveUpReason::AttemptsExhausted,
            };
        }
        ReconnectDecision::Retry {
            delay: self.base_delay(attempt),
        }
    }

    /// Deterministic (un-jittered) delay to wait after `attempt` failed.
    /// Result is clamped to `[MIN_DELAY, max_delay]`.
    #[must_use]
    pub fn base_delay(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(63) as i32;
        let multiplier = if self.multiplier.is_finite() && self.multiplier >= 1.0 {
            self.multiplier
        } else {
            1.0
        };
        let secs = self.initial_delay.as_secs_f64() * multiplier.powi(exponent);
        let cap = self.max_delay.as_secs_f64();
        let clamped = if secs.is_finite() {
            secs.clamp(MIN_DELAY.as_secs_f64(), cap.max(MIN_DELAY.as_secs_f64()))
        } else {
            cap.max(MIN_DELAY.as_secs_f64())
        };
        Duration::from_secs_f64(clamped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectDecision {
    /// Wait `delay`, then attempt again (bounded by `max_attempts`).
    Retry { delay: Duration },
    /// Stop; surface the error to the user.
    GiveUp { reason: GiveUpReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiveUpReason {
    /// Authentication, host-key, protocol, or local failure: retrying would
    /// at best repeat a user mistake, at worst hammer a server.
    NotRetriable,
    /// `max_attempts` reached; needs an explicit user action to retry.
    AttemptsExhausted,
}

/// Drives attempts through `attempt_fn` under `policy`, emitting status
/// events exactly as the UI contract requires:
///
/// * `Reconnecting { .. }` before each backoff wait;
/// * `Failed { .. }` + `Closed { .. }` only when giving up;
/// * `Closed { .. }` (without `Failed`) when cancelled.
///
/// `Ready` is emitted by the caller after it performs post-connect steps.
pub(crate) async fn connect_with_retry<T, F, Fut>(
    policy: ReconnectPolicy,
    mut cancel: watch::Receiver<bool>,
    events: &mpsc::Sender<SessionEvent>,
    mut attempt_fn: F,
) -> Result<T, SshError>
where
    F: FnMut(watch::Receiver<bool>) -> Fut,
    Fut: Future<Output = Result<T, SshError>>,
{
    let mut jitter = Jitter::from_entropy();
    let mut attempt: u32 = 0;
    loop {
        if *cancel.borrow() {
            emit_cancelled(events).await;
            return Err(SshError::SessionClosing);
        }
        attempt = attempt.saturating_add(1);

        match attempt_fn(cancel.clone()).await {
            Ok(value) => return Ok(value),
            Err(SshError::SessionClosing) => {
                // Cancelled mid-attempt (dial/handshake/auth); never retried.
                emit_cancelled(events).await;
                return Err(SshError::SessionClosing);
            }
            Err(error) => {
                let message = error.to_string();
                match policy.decide(attempt, &error) {
                    ReconnectDecision::GiveUp { .. } => {
                        let category = ErrorCategory::of(&error);
                        let _ = events
                            .send(SessionEvent::Status(SessionStatus::Failed {
                                category,
                                message: message.clone(),
                            }))
                            .await;
                        let reason = if attempt > 1 {
                            format!("{message} (gave up after {attempt} attempts)")
                        } else {
                            message
                        };
                        let _ = events
                            .send(SessionEvent::Status(SessionStatus::Closed { reason }))
                            .await;
                        return Err(error);
                    }
                    ReconnectDecision::Retry { delay } => {
                        let delay = jittered(delay, policy.jitter_ratio, &mut jitter);
                        let _ = events
                            .send(SessionEvent::Status(SessionStatus::Reconnecting {
                                attempt,
                                delay,
                                last_error: message,
                            }))
                            .await;
                        tokio::select! {
                            _ = tokio::time::sleep(delay) => {}
                            _ = cancelled(&mut cancel) => {
                                emit_cancelled(events).await;
                                return Err(SshError::SessionClosing);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Resolves when `cancel` becomes `true`. If the sender is dropped while the
/// flag is still `false`, never resolves (no further cancellation possible).
pub(crate) async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

async fn emit_cancelled(events: &mpsc::Sender<SessionEvent>) {
    let _ = events
        .send(SessionEvent::Status(SessionStatus::Closed {
            reason: "connection cancelled".to_string(),
        }))
        .await;
}

fn jittered(delay: Duration, ratio: f64, jitter: &mut Jitter) -> Duration {
    let ratio = ratio.clamp(0.0, 1.0);
    if ratio == 0.0 {
        return delay;
    }
    let factor = 1.0 - ratio + 2.0 * ratio * jitter.next_f64();
    let secs = delay.as_secs_f64() * factor;
    if secs.is_finite() && secs >= 0.0 {
        Duration::from_secs_f64(secs)
    } else {
        delay
    }
}

/// xorshift64* — jitter does not need cryptographic randomness, only decorrelation.
struct Jitter(u64);

impl Jitter {
    fn from_entropy() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xA5A5_5A5A_C3C3_3C3C);
        Self(if nanos == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            nanos
        })
    }

    fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let value = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // Top 53 bits → f64 in [0, 1).
        (value >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SshError;

    fn network_error() -> SshError {
        SshError::Interrupted
    }

    fn network_error_cat() -> SshError {
        SshError::ConnectFailed {
            host: "h".into(),
            port: 22,
            detail: "refused".into(),
        }
    }

    fn status(events: &mut mpsc::Receiver<SessionEvent>) -> Vec<SessionStatus> {
        let mut out = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let SessionEvent::Status(status) = event {
                out.push(status);
            }
        }
        out
    }

    #[test]
    fn backoff_schedule_is_bounded_and_capped() {
        let policy = ReconnectPolicy::default();
        let expected = [
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(16),
            Duration::from_secs(30), // capped from 32s
        ];
        for (index, want) in expected.iter().enumerate() {
            let attempt = (index + 1) as u32;
            assert_eq!(policy.base_delay(attempt), *want, "attempt {attempt}");
        }
        // Attempt 8 (== max_attempts) gives up instead of scheduling.
        assert_eq!(
            policy.decide(8, &network_error()),
            ReconnectDecision::GiveUp {
                reason: GiveUpReason::AttemptsExhausted
            }
        );
        assert_eq!(
            policy.decide(7, &network_error()),
            ReconnectDecision::Retry {
                delay: Duration::from_secs(30)
            }
        );
    }

    #[test]
    fn terminal_failures_are_never_retried() {
        let policy = ReconnectPolicy::default();
        let terminal = [
            SshError::AuthFailed {
                method: "password",
                detail: "no".into(),
            },
            SshError::HostKeyChanged {
                host: "h".into(),
                port: 22,
                old_fingerprint: "a".into(),
                new_fingerprint: "b".into(),
            },
            SshError::HostKeyPromptTimeout,
            SshError::PassphraseRequired,
            SshError::Protocol("bad".into()),
            SshError::ShellExited { code: 1 },
            SshError::SessionClosing,
        ];
        for error in terminal {
            assert_eq!(
                policy.decide(1, &error),
                ReconnectDecision::GiveUp {
                    reason: GiveUpReason::NotRetriable
                },
                "error: {error}"
            );
        }
    }

    #[test]
    fn retriable_network_and_timeout_errors_are_scheduled() {
        let policy = ReconnectPolicy::default();
        for error in [
            network_error(),
            network_error_cat(),
            SshError::ConnectionClosed,
            SshError::DnsFailure {
                host: "h".into(),
                detail: "tempfail".into(),
            },
            SshError::ConnectTimeout {
                host: "h".into(),
                port: 22,
                seconds: 15,
            },
        ] {
            assert!(error.is_retriable(), "expected retriable: {error}");
            assert!(matches!(
                policy.decide(1, &error),
                ReconnectDecision::Retry { .. }
            ));
        }
    }

    #[test]
    fn misconfigured_policy_cannot_spin_or_panic() {
        let mut policy = ReconnectPolicy {
            max_attempts: 4,
            initial_delay: Duration::ZERO,
            max_delay: Duration::from_secs(1),
            multiplier: 0.0,        // degenerate
            jitter_ratio: f64::NAN, // degenerate
        };
        let delay = policy.base_delay(1);
        assert!(delay >= MIN_DELAY);
        policy.multiplier = 1e300; // would overflow naive pow
        assert!(policy.base_delay(63) <= policy.max_delay.max(MIN_DELAY));
        policy.multiplier = f64::INFINITY;
        assert!(policy.base_delay(10) <= policy.max_delay.max(MIN_DELAY));
        // Jitter with NaN ratio must not panic or return a zero/NaN duration.
        let d = jittered(Duration::from_secs(2), f64::NAN, &mut Jitter(12345));
        assert!(d > Duration::ZERO);
    }

    #[test]
    fn jitter_stays_within_ratio() {
        let base = Duration::from_secs(2);
        let mut jitter = Jitter::from_entropy();
        for _ in 0..500 {
            let d = jittered(base, 0.2, &mut jitter);
            assert!(d >= Duration::from_millis(1600) && d <= Duration::from_millis(2400));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_max_attempts_with_terminal_statuses() {
        let (tx, mut rx) = mpsc::channel(64);
        let policy = ReconnectPolicy {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(1),
            multiplier: 2.0,
            jitter_ratio: 0.0,
        };
        let (_cancel_tx, cancel) = watch::channel(false);
        let mut calls = 0u32;

        let result: Result<(), SshError> = connect_with_retry(policy, cancel, &tx, |_c| {
            calls += 1;
            std::future::ready(Err::<(), _>(network_error_cat()))
        })
        .await;

        assert!(result.is_err());
        assert_eq!(calls, 3, "bounded attempts");
        let seen = status(&mut rx);
        assert!(
            seen.iter()
                .any(|s| matches!(s, SessionStatus::Reconnecting { attempt: 1, .. })),
            "expected Reconnecting after first failure: {seen:?}"
        );
        assert!(
            seen.iter()
                .any(|s| matches!(s, SessionStatus::Reconnecting { attempt: 2, .. })),
            "expected Reconnecting after second failure: {seen:?}"
        );
        let failed = seen
            .iter()
            .filter(|s| matches!(s, SessionStatus::Failed { .. }))
            .count();
        assert_eq!(failed, 1, "Failed only on final give-up: {seen:?}");
        let closed = seen
            .iter()
            .filter(|s| matches!(s, SessionStatus::Closed { .. }))
            .count();
        assert_eq!(closed, 1, "Closed only on final give-up: {seen:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn retries_until_success() {
        let (tx, mut rx) = mpsc::channel(64);
        let (_cancel_tx, cancel) = watch::channel(false);
        let mut calls = 0u32;

        let result: Result<u32, SshError> =
            connect_with_retry(ReconnectPolicy::default(), cancel, &tx, |_c| {
                calls += 1;
                let n = calls;
                std::future::ready(if n < 3 {
                    Err(SshError::Interrupted)
                } else {
                    Ok(42)
                })
            })
            .await;

        assert_eq!(result.ok(), Some(42));
        assert_eq!(calls, 3);
        let seen = status(&mut rx);
        assert!(
            !seen
                .iter()
                .any(|s| matches!(s, SessionStatus::Failed { .. })),
            "no Failed on eventual success: {seen:?}"
        );
        assert_eq!(
            seen.iter()
                .filter(|s| matches!(s, SessionStatus::Reconnecting { .. }))
                .count(),
            2
        );
    }

    #[tokio::test(start_paused = true)]
    async fn auth_failure_is_not_retried() {
        let (tx, mut rx) = mpsc::channel(64);
        let (_cancel_tx, cancel) = watch::channel(false);
        let mut calls = 0u32;

        let result: Result<(), SshError> =
            connect_with_retry(ReconnectPolicy::default(), cancel, &tx, |_c| {
                calls += 1;
                std::future::ready(Err::<(), _>(SshError::AuthFailed {
                    method: "password",
                    detail: "denied".into(),
                }))
            })
            .await;

        assert_eq!(calls, 1, "auth failure must not be retried");
        assert!(result.is_err());
        let seen = status(&mut rx);
        assert!(
            !seen
                .iter()
                .any(|s| matches!(s, SessionStatus::Reconnecting { .. })),
            "no Reconnecting for auth failure: {seen:?}"
        );
        assert_eq!(
            seen.iter()
                .filter(|s| matches!(s, SessionStatus::Failed { .. }))
                .count(),
            1
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_before_start_prevents_any_attempt() {
        let (tx, mut rx) = mpsc::channel(64);
        let (cancel_tx, cancel) = watch::channel(true);
        drop(cancel_tx);
        let mut calls = 0u32;

        let result: Result<(), SshError> =
            connect_with_retry(ReconnectPolicy::default(), cancel, &tx, |_c| {
                calls += 1;
                std::future::ready(Err::<(), _>(SshError::Interrupted))
            })
            .await;

        assert_eq!(calls, 0);
        assert!(matches!(result, Err(SshError::SessionClosing)));
        let seen = status(&mut rx);
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert!(
            matches!(&seen[0], SessionStatus::Closed { reason } if reason == "connection cancelled")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_during_backoff_stops_retries() {
        let (tx, mut rx) = mpsc::channel(64);
        let (cancel_tx, cancel) = watch::channel(false);
        let mut calls = 0u32;

        let result: Result<(), SshError> =
            connect_with_retry(ReconnectPolicy::default(), cancel, &tx, |_c| {
                calls += 1;
                if calls == 1 {
                    let _ = cancel_tx.send(true);
                }
                std::future::ready(Err::<(), _>(SshError::Interrupted))
            })
            .await;

        assert_eq!(calls, 1, "cancelled instead of running the second attempt");
        assert!(matches!(result, Err(SshError::SessionClosing)));
        let seen = status(&mut rx);
        assert_eq!(
            seen.iter()
                .filter(|s| matches!(s, SessionStatus::Reconnecting { .. }))
                .count(),
            1
        );
        assert_eq!(
            seen.iter()
                .filter(|s| matches!(s, SessionStatus::Failed { .. }))
                .count(),
            0,
            "cancellation is not a failure: {seen:?}"
        );
        assert_eq!(
            seen.iter()
                .filter(|s| matches!(s, SessionStatus::Closed { .. }))
                .count(),
            1
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_attempt_is_not_retried() {
        let (tx, mut rx) = mpsc::channel(64);
        let (_cancel_tx, cancel) = watch::channel(false);
        let mut calls = 0u32;

        let result: Result<(), SshError> =
            connect_with_retry(ReconnectPolicy::default(), cancel, &tx, |_c| {
                calls += 1;
                std::future::ready(Err::<(), _>(SshError::SessionClosing))
            })
            .await;

        assert_eq!(calls, 1);
        assert!(matches!(result, Err(SshError::SessionClosing)));
        let seen = status(&mut rx);
        assert_eq!(seen.len(), 1);
        assert!(matches!(&seen[0], SessionStatus::Closed { .. }));
    }

    #[tokio::test(start_paused = true)]
    async fn attempt_cancellation_signal_is_delivered() {
        let (cancel_tx, mut cancel) = watch::channel(false);
        cancel_tx.send(true).unwrap();
        let started = std::time::Instant::now();
        cancelled(&mut cancel).await;
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn dropped_cancel_sender_never_cancels() {
        let (tx, _rx) = mpsc::channel(8);
        let (cancel_tx, mut cancel) = watch::channel(false);
        drop(cancel_tx);
        let attempt =
            connect_with_retry(ReconnectPolicy::no_retries(), cancel.clone(), &tx, |_c| {
                std::future::ready(Err::<(), _>(SshError::Interrupted))
            });
        // If `cancelled()` resolved spuriously the loop would return
        // SessionClosing; with the sender dropped it must give up normally.
        let result = tokio::time::timeout(Duration::from_secs(10), attempt).await;
        assert!(
            matches!(
                result.expect("attempt finished"),
                Err(SshError::Interrupted)
            ),
            "give-up, not cancellation, when the sender is gone"
        );
        // Direct helper semantics: with a dropped sender and a false flag,
        // `cancelled()` must not resolve (the ready branch wins the select).
        let observed = tokio::select! {
            _ = cancelled(&mut cancel) => "cancelled",
            () = std::future::ready(()) => "pending",
        };
        assert_eq!(observed, "pending");
    }
}
