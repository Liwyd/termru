//! Ownership and lifecycle of live SSH sessions.
//!
//! Invariants:
//! * the manager is the **single owner** of every session (one map, shared by
//!   all windows); a session id exists iff the map has an entry;
//! * each session runs its connect/serve/forward loop in one background task;
//!   the task exits when the session ends, the user closes it, or the
//!   frontend channel disappears (orphan prevention);
//! * `Closed` is the terminal status of any connection; a network-flavoured
//!   death after a successful `Ready` gets a **bounded** number of automatic
//!   reconnect cycles, so a pathological server cannot cause a hot loop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use termru_core::ids::HostId;
use termru_ssh::{
    ConnectParams, ErrorCategory, ReconnectPolicy, Session, SessionEvent, SessionOptions,
    SessionStatus,
};
use termru_store::Store;
use tokio::sync::{mpsc, watch};

use crate::app_error::CommandError;

/// Per-session event buffer between the SSH engine and the forwarder.
const EVENT_BUFFER: usize = 256;
/// Consecutive auto-reconnect cycles allowed without a stable `Ready`.
const MAX_RECONNECT_CYCLES: u32 = 5;
/// A `Ready` lasting this long earns a fresh reconnect-cycle budget.
const STABLE_READY: Duration = Duration::from_secs(30);
/// Grace period for a view to (re-)attach before a session counts as
/// orphaned — a tab moved to another window briefly has no view.
const VIEW_GRACE: Duration = Duration::from_secs(10);

/// What the frontend receives on the per-session channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionUpdate {
    Status { status: SessionStatusDto },
    Data { base64: String },
    Exit { code: Option<i32> },
}

/// Serde-friendly session status (mirrors `termru_ssh::SessionStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatusDto {
    Connecting,
    Authenticating,
    Ready,
    Reconnecting {
        attempt: u32,
        delay_ms: u64,
        last_error: String,
    },
    Closed {
        reason: String,
    },
    Failed {
        category: ErrorCategoryDto,
        message: String,
    },
}

impl SessionStatusDto {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Closed { .. } | Self::Failed { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategoryDto {
    Auth,
    HostKey,
    Network,
    Timeout,
    Protocol,
    Local,
}

impl From<ErrorCategory> for ErrorCategoryDto {
    fn from(value: ErrorCategory) -> Self {
        match value {
            ErrorCategory::Auth => Self::Auth,
            ErrorCategory::HostKey => Self::HostKey,
            ErrorCategory::Network => Self::Network,
            ErrorCategory::Timeout => Self::Timeout,
            ErrorCategory::Protocol => Self::Protocol,
            ErrorCategory::Local => Self::Local,
        }
    }
}

impl From<&SessionStatus> for SessionStatusDto {
    fn from(value: &SessionStatus) -> Self {
        match value {
            SessionStatus::Connecting => Self::Connecting,
            SessionStatus::Authenticating => Self::Authenticating,
            SessionStatus::Ready => Self::Ready,
            SessionStatus::Reconnecting {
                attempt,
                delay,
                last_error,
            } => Self::Reconnecting {
                attempt: *attempt,
                delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                last_error: last_error.clone(),
            },
            SessionStatus::Closed { reason } => Self::Closed {
                reason: reason.clone(),
            },
            SessionStatus::Failed { category, message } => Self::Failed {
                category: (*category).into(),
                message: message.clone(),
            },
        }
    }
}

/// Snapshot for `session_list`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub host_id: HostId,
    pub status: SessionStatusDto,
}

struct Managed {
    host_id: HostId,
    status: SessionStatusDto,
    cancel: watch::Sender<bool>,
    session: Option<Session>,
    params: ConnectParams,
    /// `true` while the background connection task for this entry runs; a
    /// retry may never start a second task on the same entry.
    task_alive: bool,
    /// All views currently rendering this session. The first one is the
    /// owner that opened it; additional ones are attached later (a tab
    /// moved to another window). The session itself lives in the manager —
    /// views come and go without dropping the SSH connection.
    ///
    /// Each view carries an id so that a view attached *while* an event is
    /// being forwarded is never dropped by the cleanup of dead views.
    views: Vec<(u64, Channel<SessionUpdate>)>,
}

pub struct SessionManager {
    entries: Mutex<HashMap<String, Managed>>,
    next_id: AtomicU64,
    next_view_id: AtomicU64,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            next_view_id: AtomicU64::new(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Managed>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers a connecting session; returns its id and the cancellation
    /// receiver for the connection task.
    pub fn register(
        &self,
        host_id: HostId,
        params: ConnectParams,
        view: Channel<SessionUpdate>,
    ) -> (String, watch::Receiver<bool>) {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = format!("s{seq}");
        let (cancel, cancel_rx) = watch::channel(false);
        self.lock().insert(
            id.clone(),
            Managed {
                host_id,
                status: SessionStatusDto::Connecting,
                cancel,
                session: None,
                params,
                task_alive: true,
                views: vec![(self.next_view_id.fetch_add(1, Ordering::Relaxed), view)],
            },
        );
        (id, cancel_rx)
    }

    /// Called by the connection task when it exits for any reason.
    pub fn mark_task_dead(&self, id: &str) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.task_alive = false;
        }
    }

    /// Parameters + cancellation for re-running a stopped session's
    /// connection task under the same id (frontend `session_retry`).
    pub fn retry_handle(
        &self,
        id: &str,
    ) -> Result<(ConnectParams, watch::Receiver<bool>), CommandError> {
        let guard = self.lock();
        let entry = guard
            .get(id)
            .ok_or_else(|| CommandError::not_found("session", id))?;
        if entry.task_alive {
            return Err(CommandError::new(
                "connection attempt still in progress; try again shortly",
            ));
        }
        if entry.session.is_some() {
            return Err(CommandError::new("session is already connected"));
        }
        if *entry.cancel.borrow() {
            return Err(CommandError::new(
                "session was cancelled; open a new connection instead",
            ));
        }
        Ok((entry.params.clone(), entry.cancel.subscribe()))
    }

    /// Adds a view to a session without touching its transport (used when a
    /// view (re)connects to an existing session).
    pub fn add_view(&self, id: &str, view: Channel<SessionUpdate>) {
        let view_id = self.next_view_id.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.lock();
        if let Some(entry) = guard.get_mut(id) {
            entry.views.push((view_id, view));
        }
    }

    /// Attaches an additional view to a live session. The session keeps
    /// running: the new view receives the replay buffer and then the live
    /// stream. Used when a tab is moved to another window.
    pub async fn attach_view(
        &self,
        id: &str,
        view: Channel<SessionUpdate>,
    ) -> Result<(), CommandError> {
        let (session, status) = {
            let guard = self.lock();
            let entry = guard
                .get(id)
                .ok_or_else(|| CommandError::not_found("session", id))?;
            let session = entry
                .session
                .clone()
                .ok_or_else(|| CommandError::new("session is not connected yet"))?;
            (session, entry.status.clone())
        };

        // Replay first, so the new view shows the same scrollback, then live
        // events follow.
        let replay = session.replay_snapshot().await?;
        if !replay.is_empty() {
            let _ = view.send(SessionUpdate::Data {
                base64: base64::engine::general_purpose::STANDARD.encode(replay),
            });
        }
        let _ = view.send(SessionUpdate::Status { status });

        let view_id = self.next_view_id.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.lock();
        if let Some(entry) = guard.get_mut(id) {
            entry.views.push((view_id, view));
        }
        Ok(())
    }

    /// Number of views currently rendering a session.
    pub fn view_count(&self, id: &str) -> usize {
        self.lock()
            .get(id)
            .map(|entry| entry.views.len())
            .unwrap_or(0)
    }

    /// Snapshot of the views rendering a session.
    fn views(&self, id: &str) -> Vec<(u64, Channel<SessionUpdate>)> {
        self.lock()
            .get(id)
            .map(|entry| entry.views.clone())
            .unwrap_or_default()
    }

    /// Removes the views whose window disappeared. Views attached in the
    /// meantime are kept (they are not part of `dead`).
    fn remove_views(&self, id: &str, dead: &[u64]) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.views.retain(|(view_id, _)| !dead.contains(view_id));
        }
    }

    pub fn set_session(&self, id: &str, session: Option<Session>) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.session = session;
        }
    }

    pub fn set_status(&self, id: &str, status: SessionStatusDto) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.status = status;
        }
    }

    pub fn status(&self, id: &str) -> Option<SessionStatusDto> {
        self.lock().get(id).map(|entry| entry.status.clone())
    }

    pub fn info(&self, id: &str) -> Option<SessionInfo> {
        self.lock().get(id).map(|entry| SessionInfo {
            id: id.to_string(),
            host_id: entry.host_id,
            status: entry.status.clone(),
        })
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let guard = self.lock();
        let mut out: Vec<SessionInfo> = guard
            .iter()
            .map(|(id, entry)| SessionInfo {
                id: id.clone(),
                host_id: entry.host_id,
                status: entry.status.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub fn params(&self, id: &str) -> Option<ConnectParams> {
        self.lock().get(id).map(|entry| entry.params.clone())
    }

    pub fn host_id(&self, id: &str) -> Option<HostId> {
        self.lock().get(id).map(|entry| entry.host_id)
    }

    fn is_cancelled(&self, id: &str) -> bool {
        self.lock()
            .get(id)
            .is_some_and(|entry| *entry.cancel.borrow())
    }

    /// User-facing close: take the entry, signal cancellation, close the
    /// transport. A second close reports not-found (the UI has already
    /// removed the tab).
    pub async fn close(&self, id: &str) -> Result<(), CommandError> {
        let entry = self
            .lock()
            .remove(id)
            .ok_or_else(|| CommandError::not_found("session", id))?;
        let _ = entry.cancel.send(true);
        if let Some(session) = entry.session {
            let _ = session.close().await;
        }
        Ok(())
    }

    /// Internal cleanup (frontend channel gone): tolerates a missing entry
    /// so races with `close` never error.
    async fn close_now(&self, id: &str) {
        // The lock guard must be released before awaiting.
        let entry = { self.lock().remove(id) };
        if let Some(entry) = entry {
            let _ = entry.cancel.send(true);
            if let Some(session) = entry.session {
                let _ = session.close().await;
            }
        }
    }

    pub async fn write(&self, id: &str, data: Vec<u8>) -> Result<(), CommandError> {
        let session = self.live_session(id)?;
        session.write(data).await?;
        Ok(())
    }

    pub async fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), CommandError> {
        let session = self.live_session(id)?;
        session.resize(cols, rows).await?;
        Ok(())
    }

    fn live_session(&self, id: &str) -> Result<Session, CommandError> {
        let guard = self.lock();
        let entry = guard
            .get(id)
            .ok_or_else(|| CommandError::not_found("session", id))?;
        entry
            .session
            .clone()
            .ok_or_else(|| CommandError::new("session is not ready yet"))
    }

    /// Cancel + close every session; used on app exit so no session outlives
    /// the process's single owner.
    pub async fn shutdown(&self) {
        let drained: Vec<Managed> = {
            let mut guard = self.lock();
            guard.drain().map(|(_, entry)| entry).collect()
        };
        for entry in drained {
            let _ = entry.cancel.send(true);
            if let Some(session) = entry.session {
                let _ = tokio::time::timeout(Duration::from_secs(2), session.close()).await;
            }
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Waits (up to `grace`) for a view to attach to the session.
async fn wait_for_view(manager: &SessionManager, id: &str, grace: Duration) -> bool {
    let deadline = std::time::Instant::now() + grace;
    loop {
        if manager.view_count(id) > 0 {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Sends an update to every view of a session.
///
/// Returns `false` when no view accepted it (all views are gone) — the
/// caller then treats the session as orphaned.
fn forward(manager: &SessionManager, id: &str, update: SessionUpdate) -> bool {
    let views = manager.views(id);
    if views.is_empty() {
        return false;
    }
    let mut dead: Vec<u64> = Vec::new();
    let mut any_alive = false;
    for (view_id, view) in views {
        if view.send(update.clone()).is_ok() {
            any_alive = true;
        } else {
            dead.push(view_id);
        }
    }
    manager.remove_views(id, &dead);
    any_alive
}

/// Outcome of one connect+serve cycle.
enum CycleEnd {
    /// Death qualifies for automatic reconnection (budget permitting).
    AutoReconnect,
    /// Done: user close, non-retriable failure, budget exhausted, cancel.
    Stop,
    /// The frontend channel receiver is gone: tab/window disappeared.
    ChannelGone,
}

/// Pure reconnect verdict — unit-testable without time or sockets.
fn should_auto_reconnect(
    ever_ready: bool,
    failed: Option<ErrorCategory>,
    cycles: u32,
    cancelled: bool,
) -> bool {
    if cancelled || !ever_ready || cycles >= MAX_RECONNECT_CYCLES {
        return false;
    }
    matches!(
        failed,
        Some(ErrorCategory::Network) | Some(ErrorCategory::Timeout)
    )
}

/// Spawns the background connection task for a registered session id.
///
/// Public so the app-layer end-to-end test can drive a real connection
/// through the same code path the Tauri commands use.
pub fn spawn_connection(
    manager: Arc<SessionManager>,
    store: Arc<Store>,
    id: String,
    params: ConnectParams,
    options: SessionOptions,
    cancel: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        let mut ever_ready = false;
        let mut cycles: u32 = 0;
        let mut ready_since: Option<Instant> = None;

        loop {
            if *cancel.borrow() {
                break;
            }
            let (ev_tx, mut ev_rx) = mpsc::channel::<SessionEvent>(EVENT_BUFFER);
            let opened = Session::open_with_reconnect(
                params.clone(),
                options.clone(),
                ev_tx,
                ReconnectPolicy::default(),
                cancel.clone(),
            )
            .await;

            let end = match opened {
                Ok(session) => {
                    manager.set_session(&id, Some(session));
                    let end = pump(
                        &id,
                        &manager,
                        &store,
                        &mut ev_rx,
                        &mut ever_ready,
                        &mut ready_since,
                        &mut cycles,
                    )
                    .await;
                    manager.set_session(&id, None);
                    end
                }
                Err(_) => {
                    // Failure/cancellation statuses were already emitted on
                    // the events channel; forward whatever is buffered.
                    drain(&id, &manager, &mut ev_rx, &mut ever_ready).await;
                    manager.set_session(&id, None);
                    CycleEnd::Stop
                }
            };

            match end {
                CycleEnd::Stop => break,
                CycleEnd::ChannelGone => {
                    manager.close_now(&id).await;
                    break;
                }
                CycleEnd::AutoReconnect => cycles = cycles.saturating_add(1),
            }
        }
        manager.mark_task_dead(&id);
    });
}

#[allow(clippy::too_many_arguments)]
async fn pump(
    id: &str,
    manager: &SessionManager,
    store: &Store,
    ev_rx: &mut mpsc::Receiver<SessionEvent>,
    ever_ready: &mut bool,
    ready_since: &mut Option<Instant>,
    cycles: &mut u32,
) -> CycleEnd {
    let mut failed: Option<ErrorCategory> = None;
    loop {
        let Some(event) = ev_rx.recv().await else {
            // All senders gone without a `Closed`: abrupt end (actor panic or
            // drop). Same verdict logic with no observed failure category.
            return verdict(*ever_ready, failed, ready_since, cycles, false);
        };

        let (update, terminal_closed) = match &event {
            SessionEvent::Status(status) => {
                let dto = SessionStatusDto::from(status);
                if let SessionStatus::Failed { category, .. } = status {
                    failed = Some(*category);
                }
                if matches!(status, SessionStatus::Ready) {
                    failed = None;
                    *ever_ready = true;
                    *ready_since = Some(Instant::now());
                    if let Some(host_id) = manager.host_id(id) {
                        let _ = store.repos().hosts().touch_recent(host_id);
                    }
                }
                let closed = matches!(status, SessionStatus::Closed { .. });
                (
                    SessionUpdate::Status {
                        status: dto.clone(),
                    },
                    closed,
                )
            }
            SessionEvent::Data(bytes) => (
                SessionUpdate::Data {
                    base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                },
                false,
            ),
            SessionEvent::Exit { code } => (SessionUpdate::Exit { code: *code }, false),
        };

        if let SessionUpdate::Status { status } = &update {
            manager.set_status(id, status.clone());
        }
        if !forward(manager, id, update) {
            // No view accepted the event. The frontend window/tab may be
            // gone (orphaned) or just moving to another window (a new view
            // attaches within the grace period).
            if !wait_for_view(manager, id, VIEW_GRACE).await {
                return CycleEnd::ChannelGone;
            }
            // The new view gets the replay buffer on attach.
            continue;
        }
        if terminal_closed {
            let cancelled = manager.is_cancelled(id);
            return verdict(*ever_ready, failed, ready_since, cycles, cancelled);
        }
    }
}

fn verdict(
    ever_ready: bool,
    failed: Option<ErrorCategory>,
    ready_since: &mut Option<Instant>,
    cycles: &mut u32,
    cancelled: bool,
) -> CycleEnd {
    if ready_since
        .take()
        .is_some_and(|at| at.elapsed() >= STABLE_READY)
    {
        *cycles = 0;
    }
    if should_auto_reconnect(ever_ready, failed, *cycles, cancelled) {
        CycleEnd::AutoReconnect
    } else {
        CycleEnd::Stop
    }
}

/// Drain events buffered during a failed connect attempt, forwarding each.
async fn drain(
    id: &str,
    manager: &SessionManager,
    ev_rx: &mut mpsc::Receiver<SessionEvent>,
    ever_ready: &mut bool,
) {
    loop {
        match ev_rx.try_recv() {
            Ok(SessionEvent::Status(status)) => {
                let dto = SessionStatusDto::from(&status);
                if matches!(status, SessionStatus::Ready) {
                    *ever_ready = true;
                }
                manager.set_status(id, dto.clone());
                if !forward(manager, id, SessionUpdate::Status { status: dto }) {
                    return;
                }
            }
            Ok(SessionEvent::Data(bytes)) => {
                let update = SessionUpdate::Data {
                    base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                };
                if !forward(manager, id, update) {
                    return;
                }
            }
            Ok(SessionEvent::Exit { code }) => {
                if !forward(manager, id, SessionUpdate::Exit { code }) {
                    return;
                }
            }
            Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ConnectParams {
        ConnectParams {
            host: "example.test".into(),
            port: 22,
            username: "u".into(),
            auth: termru_ssh::AuthChoice::Agent { socket: None },
            term: "xterm-256color".into(),
            keepalive_seconds: 0,
            timeouts: termru_ssh::ConnectTimeouts::default(),
            known_hosts_path: std::path::PathBuf::from("/dev/null"),
            host_key_prompt: std::sync::Arc::new(termru_ssh::config::RejectUnknownHostKeys),
        }
    }

    #[test]
    fn register_assigns_unique_ids_and_lists_in_order() {
        let manager = SessionManager::new();
        let host = HostId::new();
        let (a, _ra) = manager.register(host, params(), test_channel());
        let (b, _rb) = manager.register(host, params(), test_channel());
        assert_ne!(a, b);
        let list = manager.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, a);
        assert_eq!(list[1].id, b);
        assert!(list.iter().all(|info| !info.status.is_terminal()));
    }

    #[test]
    fn unknown_session_operations_report_not_found() {
        let manager = SessionManager::new();
        let error = futures_block(manager.close("s999"));
        assert!(error.unwrap_err().message.contains("no session"));
        let error = futures_block(async { manager.write("s999", vec![1]).await });
        assert!(error.unwrap_err().message.contains("no session"));
        let error = futures_block(async { manager.resize("s999", 80, 24).await });
        assert!(error.unwrap_err().message.contains("no session"));
    }

    #[test]
    fn write_before_session_is_attached_reports_not_ready() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), test_channel());
        let error = futures_block(async { manager.write(&id, b"x".to_vec()).await });
        assert!(error.unwrap_err().message.contains("not ready"));
    }

    #[test]
    fn close_removes_entry_and_second_close_is_not_found() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), test_channel());
        futures_block(manager.close(&id)).expect("first close");
        assert!(manager.info(&id).is_none());
        let error = futures_block(manager.close(&id));
        assert!(error.unwrap_err().message.contains("no session"));
    }

    #[test]
    fn reconnect_verdict_matches_policy() {
        let network = Some(ErrorCategory::Network);
        let timeout = Some(ErrorCategory::Timeout);
        let auth = Some(ErrorCategory::Auth);
        // ready before, network death, budget left, not cancelled → retry
        assert!(should_auto_reconnect(true, network, 0, false));
        assert!(should_auto_reconnect(true, timeout, 4, false));
        // never ready (initial connect) → the connect policy already retried
        assert!(!should_auto_reconnect(false, network, 0, false));
        // non-retriable categories are never auto-retried
        assert!(!should_auto_reconnect(true, auth, 0, false));
        assert!(!should_auto_reconnect(true, None, 0, false));
        // budget exhausted
        assert!(!should_auto_reconnect(
            true,
            network,
            MAX_RECONNECT_CYCLES,
            false
        ));
        // user closed
        assert!(!should_auto_reconnect(true, network, 0, true));
    }

    #[test]
    fn status_dto_serializes_for_frontend_contract() {
        let json = serde_json::to_string(&SessionUpdate::Status {
            status: SessionStatusDto::Failed {
                category: ErrorCategoryDto::Network,
                message: "connection reset".into(),
            },
        })
        .unwrap();
        assert!(json.contains("\"kind\":\"status\""));
        assert!(json.contains("\"state\":\"failed\""));
        assert!(json.contains("\"category\":\"network\""));

        let json = serde_json::to_string(&SessionUpdate::Data {
            base64: "aGk=".into(),
        })
        .unwrap();
        assert!(json.contains("\"kind\":\"data\""));

        let reconnecting = SessionStatusDto::Reconnecting {
            attempt: 2,
            delay_ms: 1000,
            last_error: "boom".into(),
        };
        assert!(!reconnecting.is_terminal());
        assert!(
            SessionStatusDto::Closed {
                reason: "done".into()
            }
            .is_terminal()
        );
    }

    #[test]
    fn status_conversions_preserve_payload() {
        use std::time::Duration;
        let source = SessionStatus::Reconnecting {
            attempt: 3,
            delay: Duration::from_secs(2),
            last_error: "refused".into(),
        };
        let dto = SessionStatusDto::from(&source);
        assert_eq!(
            dto,
            SessionStatusDto::Reconnecting {
                attempt: 3,
                delay_ms: 2000,
                last_error: "refused".into(),
            }
        );
    }

    /// A channel that accepts (and drops) every update.
    fn test_channel() -> Channel<SessionUpdate> {
        Channel::new(|_body| Ok(()))
    }

    /// Tiny block-on for std-future test bodies (no runtime needed for the
    /// map-only paths under test).
    fn futures_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }
}
