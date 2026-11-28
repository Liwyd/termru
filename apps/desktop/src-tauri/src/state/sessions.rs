//! Ownership and lifecycle of live SSH sessions.
//!
//! Invariants:
//! * the manager is the **single owner** of every session (one map, shared by
//!   all windows); a session id exists iff the map has an entry;
//! * each session runs its connect/serve/forward loop in one background task;
//!   the task exits when the session ends, the user closes it, or no view has
//!   been attached for [`VIEW_GRACE`] (orphan prevention). A view that
//!   disappears never ends the session by itself: output keeps flowing into
//!   the replay ring, and a view that attaches within the grace period gets
//!   the scrollback. This holds for every way a view can go: a tab detached
//!   because it moved to another window (which attaches only after its
//!   webview has loaded), a destroyed window, a view whose channel died.
//!   Nothing closes a still-running session just because its last view is
//!   gone; removing views wakes the session task, so the grace period also
//!   starts for a session that produces no output at all;
//! * `Closed` is the terminal status of any connection and is delivered to
//!   every live view; a network-flavoured death after a successful `Ready`
//!   gets a **bounded** number of automatic reconnect cycles (per streak and
//!   over the whole session lifetime), so a pathological server cannot cause a
//!   hot loop;
//! * the replay ring, the status and the view list of a session live behind
//!   **one lock** ([`Stream`]): output delivery and view registration are
//!   mutually atomic, so an attaching view gets exactly the ring and then
//!   every later byte — no gap, no duplicate.
//!
//! ## Write ordering
//!
//! `session_write` is a synchronous command that enqueues into the session's
//! ordered input queue before it returns. Synchronous commands run in the
//! order the IPC messages arrive, so arrival order equals enqueue order. The
//! webview, however, may deliver *concurrent* `invoke` calls out of order:
//! the frontend must serialize its writes (await the previous one, or batch
//! keystrokes into one chunk).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use termru_core::ForwardKind;
use termru_core::ids::HostId;
use termru_ssh::{
    ConnectParams, ErrorCategory, ForwardState, ForwardStatus, ReconnectPolicy, ReplayRing,
    Session, SessionEvent, SessionOptions, SessionStatus,
};
use termru_store::Store;
use tokio::sync::{Notify, mpsc, watch};

use super::prompts::SessionDirectory;
use crate::app_error::CommandError;

/// Per-session event buffer between the SSH engine and the forwarder.
const EVENT_BUFFER: usize = 256;
/// Consecutive auto-reconnect cycles allowed without a stable `Ready`.
const MAX_RECONNECT_CYCLES: u32 = 5;
/// Auto-reconnect cycles allowed over the whole life of a session, however
/// stable the connections in between were: a server that accepts and then
/// drops us every 31 seconds must not be retried forever.
const MAX_TOTAL_RECONNECTS: u32 = 20;
/// A `Ready` lasting this long earns a fresh reconnect-cycle budget.
const STABLE_READY: Duration = Duration::from_secs(30);
/// Grace period for a view to (re-)attach before a session counts as
/// orphaned — a tab moved to another window briefly has no view.
const VIEW_GRACE: Duration = Duration::from_secs(10);
/// Scrollback kept per session for views that attach later.
const REPLAY_LIMIT: usize = 512 * 1024;
/// Output that arrives within this window is forwarded as one update.
const COALESCE_WINDOW: Duration = Duration::from_millis(8);
/// A forwarded output update never exceeds this many bytes (before base64).
const COALESCE_LIMIT: usize = 64 * 1024;
/// A first chunk smaller than this is an interactive echo: it is forwarded
/// at once instead of waiting for company.
const COALESCE_MIN_WAIT: usize = 1024;

/// What the frontend receives on the per-session channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionUpdate {
    Status {
        status: SessionStatusDto,
    },
    Data {
        base64: String,
    },
    Exit {
        code: Option<i32>,
    },
    /// What became of one of the host's port forwards. They start after the
    /// session is `ready` (these follow that status), once per forward, and
    /// again after a reconnect; every status other than `ready` clears them.
    Forward {
        forward: ForwardUpdate,
    },
}

/// The state of one port forward of a session.
///
/// `id` is the forward's index in the host's `forwards` list (a disabled
/// forward keeps its slot and is simply never reported).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ForwardUpdate {
    /// The listener is up. `address`/`port` are where it is bound: on this
    /// machine for `local` and `dynamic`, on the server for `remote`.
    Listening {
        id: u32,
        kind: ForwardKind,
        address: String,
        port: u16,
    },
    /// The forward could not be set up, or died; `reason` says why.
    Failed {
        id: u32,
        kind: ForwardKind,
        reason: String,
    },
}

impl ForwardUpdate {
    pub fn id(&self) -> u32 {
        match self {
            Self::Listening { id, .. } | Self::Failed { id, .. } => *id,
        }
    }
}

impl From<&ForwardStatus> for ForwardUpdate {
    fn from(status: &ForwardStatus) -> Self {
        match &status.state {
            ForwardState::Listening { address, port } => Self::Listening {
                id: status.id,
                kind: status.kind,
                address: address.clone(),
                port: *port,
            },
            ForwardState::Failed { reason } => Self::Failed {
                id: status.id,
                kind: status.kind,
                reason: reason.clone(),
            },
        }
    }
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
    /// The forwards that are up (or failed) right now, by `id`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forwards: Vec<ForwardUpdate>,
}

/// One view (a terminal pane in some window) rendering a session. It carries
/// the label of the window it lives in, so a closed window can release its
/// views. (Dead views are removed under the [`Stream`] lock that also guards
/// registration, so a view attached meanwhile can never be dropped by mistake.)
struct View {
    window: String,
    channel: Channel<SessionUpdate>,
}

/// What must change atomically with the set of views: the status shown to a
/// new view, the scrollback, and the views themselves. Output delivery
/// (ring push + send to every view) and view registration (ring snapshot +
/// send + push) both hold this lock for their whole duration.
struct Stream {
    status: SessionStatusDto,
    /// What the session's forwards last reported, by id: a view that attaches
    /// later is told. Emptied by every status but `Ready` (the forwards live
    /// and die with the connection).
    forwards: BTreeMap<u32, ForwardUpdate>,
    ring: ReplayRing,
    views: Vec<View>,
    /// Wakes the session's pump when views were removed from outside it. The
    /// pump only looks at the views when an event wakes it, and an idle
    /// session (no output) has no events: without this signal it would never
    /// notice that its last view is gone and the orphan grace period
    /// ([`VIEW_GRACE`]) would never start.
    views_changed: Arc<Notify>,
}

impl Stream {
    /// Registers a view and wakes the pump, so that a grace period that is
    /// running is cancelled right now and cannot expire on a view that was
    /// attached and released again while the pump slept.
    fn add_view(&mut self, view: View) {
        self.views.push(view);
        self.views_changed.notify_one();
    }

    /// Releases the views `window` holds and wakes the pump so that it
    /// re-evaluates whether the session is still shown. (The pump removes dead
    /// views itself while delivering output, and re-checks right after, so
    /// that path needs no wake-up.)
    fn remove_window(&mut self, window: &str) {
        let before = self.views.len();
        self.views.retain(|view| view.window.as_str() != window);
        if self.views.len() != before {
            // `notify_one` keeps a permit when the pump is not waiting right
            // now, so the wake-up cannot be lost.
            self.views_changed.notify_one();
        }
    }
}

fn session_info(id: &str, entry: &Managed) -> SessionInfo {
    let stream = lock_stream(&entry.stream);
    SessionInfo {
        id: id.to_string(),
        host_id: entry.host_id,
        status: stream.status.clone(),
        forwards: stream.forwards.values().cloned().collect(),
    }
}

fn lock_stream(stream: &Mutex<Stream>) -> MutexGuard<'_, Stream> {
    stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Managed {
    host_id: HostId,
    cancel: watch::Sender<bool>,
    session: Option<Session>,
    params: ConnectParams,
    /// `true` while the background connection task for this entry runs; a
    /// retry may never start a second task on the same entry.
    task_alive: bool,
    /// The newest terminal size the UI asked for. It is applied when the
    /// session becomes ready and used for the PTY request of every
    /// (re)connect, so a resize that happens while connecting is not lost.
    geometry: (u16, u16),
    /// Status, scrollback and views. The views come and go without dropping
    /// the SSH connection; the first one is the owner that opened the
    /// session, additional ones are attached later (a tab moved to another
    /// window).
    stream: Arc<Mutex<Stream>>,
}

/// Called (outside every lock) when a session is gone for good, so that what
/// hangs on it — the questions it asked the user — can go too.
type EndHook = Arc<dyn Fn(&str) + Send + Sync>;

pub struct SessionManager {
    entries: Mutex<HashMap<String, Managed>>,
    next_id: AtomicU64,
    end_hook: Mutex<Option<EndHook>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            end_hook: Mutex::new(None),
        }
    }

    /// Installs the hook that learns about ended sessions.
    pub fn set_end_hook(&self, hook: EndHook) {
        *self
            .end_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    }

    /// Tells the hook that the session's connection is over. Never call this
    /// while holding the entry map or a stream lock: the hook may emit events
    /// and must be free to look at the manager.
    fn ended(&self, id: &str) {
        let hook = self
            .end_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(id);
        }
    }

    /// Reserves the id of a session that is about to be registered, so the
    /// connection's questions can be bound to it before it exists.
    pub fn reserve_id(&self) -> String {
        format!("s{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Managed>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Lock order: the entry map first, then at most one [`Stream`]. Every
    /// method that needs both takes the map lock, clones the stream `Arc`,
    /// releases the map and only then locks the stream.
    fn stream(&self, id: &str) -> Option<Arc<Mutex<Stream>>> {
        self.lock().get(id).map(|entry| entry.stream.clone())
    }

    fn new_view(window: &str, channel: Channel<SessionUpdate>) -> View {
        View {
            window: window.to_string(),
            channel,
        }
    }

    /// Registers a connecting session; returns its id and the cancellation
    /// receiver for the connection task.
    pub fn register(
        &self,
        host_id: HostId,
        params: ConnectParams,
        window: &str,
        view: Channel<SessionUpdate>,
    ) -> (String, watch::Receiver<bool>) {
        let id = self.reserve_id();
        let cancel_rx = self.register_with_id(&id, host_id, params, window, view);
        (id, cancel_rx)
    }

    /// [`SessionManager::register`] for an id from
    /// [`SessionManager::reserve_id`].
    pub fn register_with_id(
        &self,
        id: &str,
        host_id: HostId,
        params: ConnectParams,
        window: &str,
        view: Channel<SessionUpdate>,
    ) -> watch::Receiver<bool> {
        let id = id.to_string();
        let (cancel, cancel_rx) = watch::channel(false);
        let stream = Stream {
            status: SessionStatusDto::Connecting,
            forwards: BTreeMap::new(),
            ring: ReplayRing::new(REPLAY_LIMIT),
            views: vec![Self::new_view(window, view)],
            views_changed: Arc::new(Notify::new()),
        };
        self.lock().insert(
            id,
            Managed {
                host_id,
                cancel,
                session: None,
                params,
                task_alive: true,
                geometry: (80, 24),
                stream: Arc::new(Mutex::new(stream)),
            },
        );
        cancel_rx
    }

    /// Called by the connection task when it exits for any reason.
    ///
    /// An entry that nobody shows any more is released with its
    /// `ConnectParams` (and the secrets inside). While a view still shows the
    /// ended session the entry stays: the UI offers "Reconnect", which needs
    /// the parameters; closing the tab or the window releases it then.
    pub fn mark_task_dead(&self, id: &str) {
        {
            let mut guard = self.lock();
            let Some(entry) = guard.get_mut(id) else {
                return;
            };
            entry.task_alive = false;
            if lock_stream(&entry.stream).views.is_empty() {
                guard.remove(id);
            }
        }
        // The connection task is over: no question can still be waiting.
        self.ended(id);
    }

    /// Parameters + cancellation for re-running a stopped session's
    /// connection task under the same id (frontend `session_retry`).
    pub fn retry_handle(
        &self,
        id: &str,
    ) -> Result<(ConnectParams, watch::Receiver<bool>), CommandError> {
        let mut guard = self.lock();
        let entry = guard
            .get_mut(id)
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
        // The caller starts the new connection task right away: claiming the
        // entry here (under the lock) keeps a second retry out, and keeps the
        // entry from being treated as finished while the task runs.
        entry.task_alive = true;
        Ok((entry.params.clone(), entry.cancel.subscribe()))
    }

    /// Adds a view to a session without touching its transport (used when a
    /// view (re)connects to an existing session, e.g. on retry).
    pub fn add_view(&self, id: &str, window: &str, view: Channel<SessionUpdate>) {
        if let Some(stream) = self.stream(id) {
            let view = Self::new_view(window, view);
            lock_stream(&stream).add_view(view);
        }
    }

    /// Attaches an additional view to a session. The session keeps running:
    /// the new view receives the scrollback, the current status and then the
    /// live stream. Used when a tab is moved to another window.
    ///
    /// Snapshot and registration happen under the stream lock that output
    /// delivery also holds, so output produced meanwhile is either in the
    /// snapshot or delivered after the registration — never lost, never
    /// duplicated.
    pub async fn attach_view(
        &self,
        id: &str,
        window: &str,
        view: Channel<SessionUpdate>,
    ) -> Result<(), CommandError> {
        let stream = self
            .stream(id)
            .ok_or_else(|| CommandError::not_found("session", id))?;
        let view = Self::new_view(window, view);
        let mut guard = lock_stream(&stream);
        let snapshot = guard.ring.snapshot();
        if !snapshot.is_empty() {
            let _ = view.channel.send(SessionUpdate::Data {
                base64: encode(&snapshot),
            });
        }
        let _ = view.channel.send(SessionUpdate::Status {
            status: guard.status.clone(),
        });
        for forward in guard.forwards.values() {
            let _ = view.channel.send(SessionUpdate::Forward {
                forward: forward.clone(),
            });
        }
        guard.add_view(view);
        Ok(())
    }

    /// Releases the views of a window that was closed.
    ///
    /// A session that nobody shows any more is **not** closed here: its tab
    /// may be on its way to another window that has not attached yet (the
    /// window was closed right after "move to new window"). The session's
    /// pump closes it when no view attached within [`VIEW_GRACE`]; this call
    /// only wakes the pump so that the grace period starts at once, also for
    /// a session that produces no output. Sessions that already ended and are
    /// shown nowhere are released right here.
    pub fn drop_window_views(&self, window: &str) {
        let mut finished = Vec::new();
        {
            let mut guard = self.lock();
            for (id, entry) in guard.iter() {
                let mut stream = lock_stream(&entry.stream);
                stream.remove_window(window);
                if stream.views.is_empty() && !entry.task_alive {
                    finished.push(id.clone());
                }
            }
            for id in &finished {
                guard.remove(id);
            }
        }
        for id in &finished {
            self.ended(id);
        }
    }

    /// Releases the views that `window` holds on one session, without closing
    /// it (a tab that moved to another window). Same rule as for a destroyed
    /// window ([`SessionManager::drop_window_views`]): a session left without
    /// any view keeps running for [`VIEW_GRACE`], so a view that attaches in
    /// time keeps it alive, and is closed by its pump after that. A session
    /// that already ended and is shown nowhere is released here.
    ///
    /// The frontend detaches right after the new window was *created*, which
    /// is before that window's webview has loaded and attached; closing the
    /// session at this point would kill the tab that is being moved.
    pub fn detach_window_views(&self, id: &str, window: &str) -> Result<(), CommandError> {
        let finished = {
            let mut guard = self.lock();
            let entry = guard
                .get(id)
                .ok_or_else(|| CommandError::not_found("session", id))?;
            let finished = {
                let mut stream = lock_stream(&entry.stream);
                stream.remove_window(window);
                stream.views.is_empty() && !entry.task_alive
            };
            if finished {
                guard.remove(id);
            }
            finished
        };
        if finished {
            self.ended(id);
        }
        Ok(())
    }

    /// The labels of the windows that show a session (each once, in the
    /// order the views were added).
    pub fn view_windows(&self, id: &str) -> Vec<String> {
        let Some(stream) = self.stream(id) else {
            return Vec::new();
        };
        let guard = lock_stream(&stream);
        let mut windows: Vec<String> = Vec::new();
        for view in &guard.views {
            if !windows.contains(&view.window) {
                windows.push(view.window.clone());
            }
        }
        windows
    }

    /// Number of views currently rendering a session.
    pub fn view_count(&self, id: &str) -> usize {
        self.stream(id)
            .map(|stream| lock_stream(&stream).views.len())
            .unwrap_or(0)
    }

    pub fn set_session(&self, id: &str, session: Option<Session>) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.session = session;
        }
    }

    pub fn set_status(&self, id: &str, status: SessionStatusDto) {
        if let Some(stream) = self.stream(id) {
            lock_stream(&stream).status = status;
        }
    }

    pub fn status(&self, id: &str) -> Option<SessionStatusDto> {
        self.stream(id)
            .map(|stream| lock_stream(&stream).status.clone())
    }

    pub fn info(&self, id: &str) -> Option<SessionInfo> {
        let guard = self.lock();
        guard.get(id).map(|entry| session_info(id, entry))
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let guard = self.lock();
        let mut out: Vec<SessionInfo> = guard
            .iter()
            .map(|(id, entry)| session_info(id, entry))
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

    /// The terminal size the session should have (see [`Managed::geometry`]).
    pub fn geometry(&self, id: &str) -> Option<(u16, u16)> {
        self.lock().get(id).map(|entry| entry.geometry)
    }

    pub fn set_geometry(&self, id: &str, cols: u16, rows: u16) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.geometry = (cols, rows);
        }
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
        self.ended(id);
        if let Some(session) = entry.session {
            let _ = session.close().await;
        }
        Ok(())
    }

    /// Internal cleanup (no view left): tolerates a missing entry so races
    /// with `close` never error.
    async fn close_now(&self, id: &str) {
        // The lock guard must be released before awaiting.
        let entry = { self.lock().remove(id) };
        if let Some(entry) = entry {
            let _ = entry.cancel.send(true);
            self.ended(id);
            if let Some(session) = entry.session {
                let _ = session.close().await;
            }
        }
    }

    /// Queues `data` for the session's shell **now**, in call order, without
    /// blocking: this is what the synchronous `session_write` command uses.
    /// A full queue (the remote stopped reading input) is reported as an
    /// error instead of waiting.
    pub fn try_write(&self, id: &str, data: Vec<u8>) -> Result<(), CommandError> {
        self.live_session(id)?.try_write(data)?;
        Ok(())
    }

    /// Like [`SessionManager::try_write`], but waits while the input queue is
    /// full.
    pub async fn write(&self, id: &str, data: Vec<u8>) -> Result<(), CommandError> {
        let session = self.live_session(id)?;
        session.write(data).await?;
        Ok(())
    }

    /// Records the terminal size and applies it when the session is up.
    ///
    /// While the session is still connecting (or reconnecting) the size is
    /// only stored — and returns `Ok`, because nothing went wrong: it is
    /// applied the moment the session becomes ready and is used for the PTY
    /// request of every later reconnect.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), CommandError> {
        let session = {
            let mut guard = self.lock();
            let entry = guard
                .get_mut(id)
                .ok_or_else(|| CommandError::not_found("session", id))?;
            entry.geometry = (cols, rows);
            entry.session.clone()
        };
        if let Some(session) = session {
            // A session that is just going away needs no resize; the size is
            // stored for the reconnect.
            let _ = session.try_resize(cols, rows);
        }
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
        let drained: Vec<(String, Managed)> = {
            let mut guard = self.lock();
            guard.drain().collect()
        };
        for (id, entry) in drained {
            let _ = entry.cancel.send(true);
            self.ended(&id);
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

impl SessionDirectory for SessionManager {
    fn view_windows(&self, session_id: &str) -> Vec<String> {
        SessionManager::view_windows(self, session_id)
    }

    fn host_id(&self, session_id: &str) -> Option<String> {
        SessionManager::host_id(self, session_id).map(|id| id.to_string())
    }
}

// ---------- delivery to the views ----------

fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Sends `update` to every view and drops the views that are gone. Returns
/// how many views are left. The common single-view case sends by move.
fn broadcast(views: &mut Vec<View>, update: SessionUpdate) -> usize {
    if let [only] = views.as_slice() {
        if only.channel.send(update).is_err() {
            views.clear();
        }
        return views.len();
    }
    views.retain(|view| view.channel.send(update.clone()).is_ok());
    views.len()
}

/// Appends output to the scrollback **and** sends it to every view, under
/// one lock (see [`Stream`]). Returns the number of views left.
fn deliver_data(stream: &Mutex<Stream>, bytes: &[u8]) -> usize {
    let mut guard = lock_stream(stream);
    guard.ring.push(bytes);
    broadcast(
        &mut guard.views,
        SessionUpdate::Data {
            base64: encode(bytes),
        },
    )
}

/// Records the status (for views that attach later) and sends it to every
/// view.
fn deliver_status(stream: &Mutex<Stream>, status: SessionStatusDto) -> usize {
    let mut guard = lock_stream(stream);
    if status != SessionStatusDto::Ready {
        // The forwards belong to the connection that just changed or ended.
        guard.forwards.clear();
    }
    guard.status = status.clone();
    broadcast(&mut guard.views, SessionUpdate::Status { status })
}

/// Records what a forward reported (for views that attach later) and sends
/// it to every view.
fn deliver_forward(stream: &Mutex<Stream>, forward: ForwardUpdate) -> usize {
    let mut guard = lock_stream(stream);
    guard.forwards.insert(forward.id(), forward.clone());
    broadcast(&mut guard.views, SessionUpdate::Forward { forward })
}

fn deliver_exit(stream: &Mutex<Stream>, code: Option<i32>) -> usize {
    let mut guard = lock_stream(stream);
    broadcast(&mut guard.views, SessionUpdate::Exit { code })
}

// ---------- the connection task ----------

/// Outcome of one connect+serve cycle.
enum CycleEnd {
    /// Death qualifies for automatic reconnection (budget permitting).
    AutoReconnect,
    /// Done: user close, non-retriable failure, budget exhausted, cancel.
    Stop,
    /// No view showed the session for the whole grace period.
    ChannelGone,
}

/// Auto-reconnect accounting of one session.
#[derive(Default)]
struct ReconnectBudget {
    /// Cycles since the last stable `Ready`.
    streak: u32,
    /// Cycles over the whole session lifetime (never reset).
    total: u32,
}

/// Pure reconnect verdict — unit-testable without time or sockets.
fn should_auto_reconnect(
    ever_ready: bool,
    failed: Option<ErrorCategory>,
    budget: &ReconnectBudget,
    cancelled: bool,
) -> bool {
    if cancelled
        || !ever_ready
        || budget.streak >= MAX_RECONNECT_CYCLES
        || budget.total >= MAX_TOTAL_RECONNECTS
    {
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
    // The requested size is the session's geometry from here on; resizes
    // update it, and every cycle reads it back.
    manager.set_geometry(&id, options.cols, options.rows);
    tauri::async_runtime::spawn(async move {
        let mut ever_ready = false;
        let mut budget = ReconnectBudget::default();
        let mut ready_since: Option<Instant> = None;

        while !*cancel.borrow() {
            let Some(stream) = manager.stream(&id) else {
                break;
            };
            let (cols, rows) = manager
                .geometry(&id)
                .unwrap_or((options.cols, options.rows));
            let cycle_options = SessionOptions {
                cols,
                rows,
                // The manager keeps its own ring next to the view list (see
                // `Stream`); a second copy inside the engine would only cost
                // memory.
                replay_limit: 0,
            };
            let end = run_cycle(
                &manager,
                &store,
                &id,
                stream,
                params.clone(),
                cycle_options,
                cancel.clone(),
                (&mut ever_ready, &mut ready_since, &mut budget),
            )
            .await;
            manager.set_session(&id, None);

            match end {
                CycleEnd::Stop => break,
                CycleEnd::ChannelGone => {
                    manager.close_now(&id).await;
                    break;
                }
                CycleEnd::AutoReconnect => {
                    budget.streak = budget.streak.saturating_add(1);
                    budget.total = budget.total.saturating_add(1);
                }
            }
        }
        manager.mark_task_dead(&id);
    });
}

/// One connect + serve cycle: opens the session and forwards its events to
/// the views until it ends.
///
/// Opening and forwarding run in one loop, so the progress statuses
/// (`Connecting`, `Reconnecting`, ...) reach the UI while the connect is
/// still in flight instead of in one burst afterwards.
#[allow(clippy::too_many_arguments)]
async fn run_cycle(
    manager: &SessionManager,
    store: &Arc<Store>,
    id: &str,
    stream: Arc<Mutex<Stream>>,
    params: ConnectParams,
    options: SessionOptions,
    cancel: watch::Receiver<bool>,
    (ever_ready, ready_since, budget): (&mut bool, &mut Option<Instant>, &mut ReconnectBudget),
) -> CycleEnd {
    let (ev_tx, mut ev_rx) = mpsc::channel::<SessionEvent>(EVENT_BUFFER);
    let opened_size = (options.cols, options.rows);
    let mut open = Box::pin(Session::open_with_reconnect(
        params,
        options,
        ev_tx,
        ReconnectPolicy::default(),
        cancel,
    ));
    let mut opening = true;
    // `Ready` is held back until the session is registered with the manager,
    // so a UI that reacts to it by writing never hits "not ready yet". The
    // forwards' reports follow `Ready` and are held with it, in order.
    let mut held: VecDeque<SessionEvent> = VecDeque::new();
    let mut pump = Pump {
        id,
        manager,
        store,
        views: ViewWatch::new(stream.clone()),
        stream,
        session: None,
        opened_size,
        ever_ready,
        ready_since,
        budget,
        failed: None,
    };
    // Events to handle before reading new ones (ahead of the channel).
    let mut deferred: VecDeque<SessionEvent> = VecDeque::new();

    loop {
        let event = if let Some(event) = deferred.pop_front() {
            event
        } else {
            tokio::select! {
                // Polled first so that an expired grace period wins over a
                // steady flow of events. It also reacts to views that were
                // removed while no event arrives (see `ViewWatch`).
                biased;
                () = pump.views.orphaned() => return CycleEnd::ChannelGone,
                opened = &mut open, if opening => {
                    opening = false;
                    if let Ok(session) = opened {
                        manager.set_session(id, Some(session.clone()));
                        pump.session = Some(session);
                        deferred.extend(held.drain(..));
                    }
                    // On failure the final statuses are already queued in
                    // `ev_rx`; they are forwarded below.
                    continue;
                }
                received = ev_rx.recv() => match received {
                    Some(event) => event,
                    None => return pump.ended_without_close(),
                },
            }
        };

        match event {
            SessionEvent::Data(bytes) => {
                let (merged, next) = coalesce(bytes, &mut ev_rx).await;
                deliver_data(&pump.stream, &merged);
                if let Some(next) = next {
                    deferred.push_front(next);
                }
            }
            SessionEvent::Banner(text) => {
                // The server's pre-login text is plain output as far as the
                // terminal is concerned: it also lands in the scrollback.
                deliver_data(&pump.stream, &banner_to_terminal(&text));
            }
            SessionEvent::Exit { code } => {
                deliver_exit(&pump.stream, code);
            }
            SessionEvent::Status(SessionStatus::Ready) if opening => {
                held.push_back(SessionEvent::Status(SessionStatus::Ready));
            }
            SessionEvent::Forward(status) if opening => {
                held.push_back(SessionEvent::Forward(status));
            }
            SessionEvent::Forward(status) => {
                deliver_forward(&pump.stream, ForwardUpdate::from(&status));
            }
            SessionEvent::Status(status) => {
                if let Some(end) = pump.on_status(&status) {
                    return end;
                }
            }
        }
    }
}

/// The server's banner as terminal output: one line per line of text, with
/// the carriage returns a terminal needs (the text is already stripped of
/// everything a terminal would interpret, see `termru-ssh`).
fn banner_to_terminal(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 4);
    for line in text.trim_end_matches('\n').split('\n') {
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Merges consecutive output chunks into one update of at most
/// [`COALESCE_LIMIT`] bytes.
///
/// Chunks that are already queued are always merged. When the first chunk is
/// big enough to suggest streaming output, the producer gets up to
/// [`COALESCE_WINDOW`] to add more; a small chunk (a keystroke echo) is
/// forwarded immediately. Returns the merged bytes and the first event that
/// is not output (it must be handled next, in order).
async fn coalesce(
    first: Vec<u8>,
    ev_rx: &mut mpsc::Receiver<SessionEvent>,
) -> (Vec<u8>, Option<SessionEvent>) {
    let mut merged = first;
    let deadline = tokio::time::Instant::now() + COALESCE_WINDOW;
    while merged.len() < COALESCE_LIMIT {
        let next = match ev_rx.try_recv() {
            Ok(event) => event,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
            Err(mpsc::error::TryRecvError::Empty) => {
                if merged.len() < COALESCE_MIN_WAIT {
                    break;
                }
                match tokio::time::timeout_at(deadline, ev_rx.recv()).await {
                    Ok(Some(event)) => event,
                    Ok(None) | Err(_) => break,
                }
            }
        };
        match next {
            SessionEvent::Data(more) if merged.len() + more.len() <= COALESCE_LIMIT => {
                merged.extend_from_slice(&more);
            }
            // Over the limit, or not output at all: handled next, in order.
            other => return (merged, Some(other)),
        }
    }
    (merged, None)
}

/// Tracks how long a session has been shown nowhere.
///
/// All of its state lives in the struct, so the future of
/// [`ViewWatch::orphaned`] can be dropped and recreated at will (the pump
/// does that on every loop iteration).
struct ViewWatch {
    stream: Arc<Mutex<Stream>>,
    views_changed: Arc<Notify>,
    /// Since when no view shows the session. Tokio's clock, so that the
    /// grace period follows the paused clock of the tests.
    orphan_since: Option<tokio::time::Instant>,
}

impl ViewWatch {
    fn new(stream: Arc<Mutex<Stream>>) -> Self {
        let views_changed = lock_stream(&stream).views_changed.clone();
        Self {
            stream,
            views_changed,
            orphan_since: None,
        }
    }

    /// Starts the grace period when the last view is gone, cancels it when a
    /// view is there again.
    fn refresh(&mut self) {
        let shown = !lock_stream(&self.stream).views.is_empty();
        if shown {
            self.orphan_since = None;
        } else if self.orphan_since.is_none() {
            self.orphan_since = Some(tokio::time::Instant::now());
        }
    }

    /// Resolves when no view has shown the session for [`VIEW_GRACE`].
    ///
    /// A view that attaches within the grace period cancels it. The future
    /// is woken whenever views come or go (`Stream::add_view`,
    /// `Stream::remove_window`), so a session that receives no events still
    /// starts the grace period when it loses its last view.
    async fn orphaned(&mut self) {
        loop {
            self.refresh();
            let Some(since) = self.orphan_since else {
                self.views_changed.notified().await;
                continue;
            };
            tokio::select! {
                () = self.views_changed.notified() => {}
                () = tokio::time::sleep_until(since + VIEW_GRACE) => {
                    // A view may have attached while the timer fired.
                    self.refresh();
                    if self.orphan_since.is_some() {
                        return;
                    }
                }
            }
        }
    }
}

/// The forwarding state of one cycle.
struct Pump<'a> {
    id: &'a str,
    manager: &'a SessionManager,
    store: &'a Arc<Store>,
    stream: Arc<Mutex<Stream>>,
    views: ViewWatch,
    /// The live session, once the open completed.
    session: Option<Session>,
    /// Size the session was opened with.
    opened_size: (u16, u16),
    ever_ready: &'a mut bool,
    ready_since: &'a mut Option<Instant>,
    budget: &'a mut ReconnectBudget,
    failed: Option<ErrorCategory>,
}

impl Pump<'_> {
    /// Handles a status event; returns the end of the cycle when it was the
    /// terminal `Closed`.
    fn on_status(&mut self, status: &SessionStatus) -> Option<CycleEnd> {
        match status {
            SessionStatus::Failed { category, .. } => self.failed = Some(*category),
            SessionStatus::Ready => {
                self.failed = None;
                *self.ever_ready = true;
                *self.ready_since = Some(Instant::now());
                self.touch_recent();
                self.apply_latest_geometry();
            }
            _ => {}
        }
        deliver_status(&self.stream, SessionStatusDto::from(status));
        if matches!(status, SessionStatus::Closed { .. }) {
            return Some(self.verdict());
        }
        None
    }

    /// The engine has no `Closed` for us (its task died or was dropped): tell
    /// the views anyway, so no tab is left showing a connection that is gone.
    fn ended_without_close(&mut self) -> CycleEnd {
        deliver_status(
            &self.stream,
            SessionStatusDto::Closed {
                reason: "the connection ended unexpectedly".to_string(),
            },
        );
        self.verdict()
    }

    /// Records the host as recently used. SQLite must not run on an async
    /// worker, and the pump must not wait for it.
    fn touch_recent(&self) {
        if let Some(host_id) = self.manager.host_id(self.id) {
            let store = self.store.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(error) = store.repos().hosts().touch_recent(host_id) {
                    tracing::warn!(%error, "could not record the recent host");
                }
            });
        }
    }

    /// A resize that arrived while connecting is applied now.
    fn apply_latest_geometry(&self) {
        let (Some(session), Some((cols, rows))) = (&self.session, self.manager.geometry(self.id))
        else {
            return;
        };
        if (cols, rows) != self.opened_size {
            let _ = session.try_resize(cols, rows);
        }
    }

    fn verdict(&mut self) -> CycleEnd {
        if self
            .ready_since
            .take()
            .is_some_and(|at| at.elapsed() >= STABLE_READY)
        {
            self.budget.streak = 0;
        }
        let cancelled = self.manager.is_cancelled(self.id);
        if should_auto_reconnect(*self.ever_ready, self.failed, self.budget, cancelled) {
            CycleEnd::AutoReconnect
        } else {
            CycleEnd::Stop
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
            auth: vec![termru_ssh::AuthChoice::Agent {
                socket: None,
                prefer_fingerprint: None,
            }],
            term: "xterm-256color".into(),
            keepalive_seconds: 0,
            timeouts: termru_ssh::ConnectTimeouts::default(),
            known_hosts_path: std::path::PathBuf::from("/dev/null"),
            extra_known_hosts: Vec::new(),
            host_key_prompt: std::sync::Arc::new(termru_ssh::config::RejectUnknownHostKeys),
            auth_prompt: std::sync::Arc::new(termru_ssh::NoAuthPrompts),
            setup: Default::default(),
        }
    }

    /// Records the sessions the manager reported as ended.
    fn record_ends(manager: &SessionManager) -> Arc<Mutex<Vec<String>>> {
        let ended = Arc::new(Mutex::new(Vec::new()));
        let log = ended.clone();
        manager.set_end_hook(Arc::new(move |id| log.lock().unwrap().push(id.to_string())));
        ended
    }

    #[test]
    fn the_windows_of_a_session_are_listed_once_each() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        manager.add_view(&id, "second", test_channel());
        manager.add_view(&id, "main", test_channel());
        assert_eq!(manager.view_windows(&id), ["main", "second"]);
        assert!(manager.view_windows("nope").is_empty());
        manager.drop_window_views("main");
        assert_eq!(manager.view_windows(&id), ["second"]);
    }

    /// A reserved id is the one the session gets, so the connection's
    /// questions can carry it before the session exists.
    #[test]
    fn a_reserved_id_is_the_sessions_id() {
        let manager = SessionManager::new();
        let id = manager.reserve_id();
        let other = manager.reserve_id();
        assert_ne!(id, other);
        let _cancel =
            manager.register_with_id(&id, HostId::new(), params(), "main", test_channel());
        assert!(manager.info(&id).is_some());
        assert!(manager.info(&other).is_none());
    }

    /// The questions a session asked go away with it, however it ends.
    #[test]
    fn every_way_a_session_ends_tells_the_end_hook() {
        let manager = SessionManager::new();
        let ended = record_ends(&manager);
        let host = HostId::new();

        // The user closes the tab.
        let (closed, _a) = manager.register(host, params(), "main", test_channel());
        futures_block(manager.close(&closed)).expect("close");

        // The connection task ends with nobody looking at the session.
        let (dead, _b) = manager.register(host, params(), "other", test_channel());
        manager.drop_window_views("other");
        manager.mark_task_dead(&dead);

        // The task ends but the tab still shows the failure: the session
        // stays for "Reconnect", and its questions are gone all the same.
        let (shown, _c) = manager.register(host, params(), "main", test_channel());
        manager.mark_task_dead(&shown);
        assert!(manager.info(&shown).is_some());

        // The app shuts down.
        let (left, _d) = manager.register(host, params(), "main", test_channel());
        futures_block(manager.shutdown());

        // The hook may hear of a session more than once (idempotent).
        let mut ended = ended.lock().unwrap().clone();
        ended.sort();
        ended.dedup();
        let mut expected = vec![closed, dead, shown, left];
        expected.sort();
        assert_eq!(ended, expected);
    }

    #[test]
    fn the_banner_is_printed_line_by_line_with_carriage_returns() {
        assert_eq!(
            banner_to_terminal("Welcome\nSecond"),
            b"Welcome\r\nSecond\r\n"
        );
        assert_eq!(banner_to_terminal("One line\n\n"), b"One line\r\n");
        assert_eq!(banner_to_terminal("A\n\nB"), b"A\r\n\r\nB\r\n");
    }

    #[test]
    fn register_assigns_unique_ids_and_lists_in_order() {
        let manager = SessionManager::new();
        let host = HostId::new();
        let (a, _ra) = manager.register(host, params(), "main", test_channel());
        let (b, _rb) = manager.register(host, params(), "main", test_channel());
        assert_ne!(a, b);
        let list = manager.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, a);
        assert_eq!(list[1].id, b);
        assert!(list.iter().all(|info| !info.status.is_terminal()));
    }

    #[test]
    fn closing_a_window_releases_only_its_views() {
        let manager = SessionManager::new();
        let host = HostId::new();
        let (mine, _a) = manager.register(host, params(), "main", test_channel());
        let (shared, _b) = manager.register(host, params(), "other", test_channel());
        // A second window shows the same session.
        manager.add_view(&shared, "main", test_channel());
        assert_eq!(manager.view_count(&shared), 2);

        // Closing the first window releases its views.
        manager.drop_window_views("main");
        // The session the other window still shows keeps running.
        assert_eq!(manager.view_count(&shared), 1);
        assert!(manager.info(&shared).is_some());
        // Regression test (a tab moved to a window that has not attached yet
        // died with the window it left): a session nobody shows any more is
        // not closed here, its pump closes it after the grace period.
        assert_eq!(manager.view_count(&mine), 0);
        assert!(manager.info(&mine).is_some(), "left to the grace period");
    }

    /// `session_detach`: only the calling window's views go.
    ///
    /// Regression test (moving a tab to a new window killed the session): the
    /// frontend detaches as soon as the new window is *created*, before that
    /// window's webview attached. A session left without a view must survive
    /// the detach and accept a view that attaches afterwards.
    #[test]
    fn detaching_a_window_keeps_sessions_other_windows_show() {
        let manager = SessionManager::new();
        let host = HostId::new();
        let (id, _cancel) = manager.register(host, params(), "main", test_channel());
        // The tab moved: the new window attached first.
        manager.add_view(&id, "other", test_channel());
        manager.add_view(&id, "other", test_channel());
        assert_eq!(manager.view_count(&id), 3);

        // The old window lets go: the session stays, with the new window's views.
        manager.detach_window_views(&id, "main").unwrap();
        assert_eq!(manager.view_count(&id), 2);
        // Detaching a window that holds no view is a harmless no-op.
        manager.detach_window_views(&id, "main").unwrap();
        assert_eq!(manager.view_count(&id), 2);
        assert!(manager.info(&id).is_some(), "still running");

        // The last window lets go: the session is *not* closed (its view may
        // be on the way), it is left to the grace period of its pump.
        manager.detach_window_views(&id, "other").unwrap();
        assert_eq!(manager.view_count(&id), 0);
        assert!(manager.info(&id).is_some(), "not closed by the detach");
        // A view that attaches afterwards (the new window's webview loaded).
        futures_block(manager.attach_view(&id, "new", test_channel())).unwrap();
        assert_eq!(manager.view_count(&id), 1);
        assert!(manager.info(&id).is_some());
        futures_block(manager.close(&id)).unwrap();
        assert!(manager.info(&id).is_none());
    }

    #[test]
    fn detaching_from_an_unknown_or_ended_session() {
        let manager = SessionManager::new();
        let error = manager.detach_window_views("s999", "main").unwrap_err();
        assert!(error.message.contains("no session"));

        // An ended session that nobody shows after the detach is released.
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        manager.mark_task_dead(&id);
        assert!(manager.info(&id).is_some(), "shown, so it stays");
        manager.detach_window_views(&id, "main").unwrap();
        assert!(manager.info(&id).is_none(), "released with its last view");
    }

    /// The interleavings of a user who opens, closes, moves and quits fast.
    ///
    /// The invariants: a session that was closed is gone (and closing it
    /// again reports "no session"), a session that is still shown keeps its
    /// views, and a session nobody shows any more is kept for the grace
    /// period instead of being closed on the spot.
    #[test]
    fn rapid_open_close_and_move_keep_the_invariants() {
        let manager = SessionManager::new();
        let host = HostId::new();
        let mut ids = Vec::new();
        for _ in 0..20 {
            let (id, _cancel) = manager.register(host, params(), "main", test_channel());
            // Every session is also shown in a second window (a moved tab).
            manager.add_view(&id, "other", test_channel());
            ids.push(id);
        }
        assert_eq!(manager.list().len(), 20);

        // Close every third session, then close one of them again.
        for (index, id) in ids.iter().enumerate() {
            if index % 3 == 0 {
                futures_block(manager.close(id)).expect("the first close");
                let error = futures_block(manager.close(id));
                assert!(error.unwrap_err().message.contains("no session"));
            }
        }
        let alive = manager.list().len();
        assert_eq!(alive, 20 - 7);

        // The first window goes away: every session is still shown in the
        // second window.
        manager.drop_window_views("main");
        for id in &ids {
            if manager.info(id).is_none() {
                continue; // closed above
            }
            assert!(manager.info(id).is_some());
            assert_eq!(manager.view_count(id), 1);
        }

        // The second window goes away too: nobody shows them, but they are
        // not closed — each pump closes its session after the grace period.
        manager.drop_window_views("other");
        let left: Vec<&String> = ids
            .iter()
            .filter(|id| manager.info(id.as_str()).is_some())
            .collect();
        assert_eq!(left.len(), alive);
        assert!(left.iter().all(|id| manager.view_count(id) == 0));

        // Closing them releases everything.
        for id in left {
            futures_block(manager.close(id)).expect("the close");
        }
        assert!(manager.list().is_empty());
    }

    /// The manager is used from several tasks: the lock must hold.
    #[test]
    fn concurrent_open_and_close_never_panic() {
        let manager = std::sync::Arc::new(SessionManager::new());
        let host = HostId::new();
        let mut handles = Vec::new();
        for _ in 0..4 {
            let manager = manager.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..25 {
                    let (id, _cancel) = manager.register(host, params(), "main", test_channel());
                    manager.add_view(&id, "other", test_channel());
                    let _ = futures_block(manager.close(&id));
                }
            }));
        }
        for handle in handles {
            handle.join().expect("the thread did not panic");
        }
        // Every session was closed: nothing is left.
        assert!(manager.list().is_empty());
    }

    #[test]
    fn unknown_session_operations_report_not_found() {
        let manager = SessionManager::new();
        let error = futures_block(manager.close("s999"));
        assert!(error.unwrap_err().message.contains("no session"));
        let error = futures_block(async { manager.write("s999", vec![1]).await });
        assert!(error.unwrap_err().message.contains("no session"));
        let error = manager.resize("s999", 80, 24);
        assert!(error.unwrap_err().message.contains("no session"));
        let error = manager.try_write("s999", vec![1]);
        assert!(error.unwrap_err().message.contains("no session"));
    }

    #[test]
    fn write_before_session_is_attached_reports_not_ready() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let error = futures_block(async { manager.write(&id, b"x".to_vec()).await });
        assert!(error.unwrap_err().message.contains("not ready"));
        let error = manager.try_write(&id, b"x".to_vec());
        assert!(error.unwrap_err().message.contains("not ready"));
    }

    #[test]
    fn close_removes_entry_and_second_close_is_not_found() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
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
        let fresh = ReconnectBudget::default();
        let streak = |streak| ReconnectBudget {
            streak,
            total: streak,
        };
        // ready before, network death, budget left, not cancelled → retry
        assert!(should_auto_reconnect(true, network, &fresh, false));
        assert!(should_auto_reconnect(true, timeout, &streak(4), false));
        // never ready (initial connect) → the connect policy already retried
        assert!(!should_auto_reconnect(false, network, &fresh, false));
        // non-retriable categories are never auto-retried
        assert!(!should_auto_reconnect(true, auth, &fresh, false));
        assert!(!should_auto_reconnect(true, None, &fresh, false));
        // streak budget exhausted
        assert!(!should_auto_reconnect(
            true,
            network,
            &streak(MAX_RECONNECT_CYCLES),
            false
        ));
        // user closed
        assert!(!should_auto_reconnect(true, network, &fresh, true));
    }

    /// Regression test: the streak counter resets after every stable
    /// connection, so a server that drops us every 31 seconds used to be
    /// retried forever. The lifetime total is never reset.
    #[test]
    fn total_reconnects_are_capped_over_the_session_lifetime() {
        let worn_out = ReconnectBudget {
            streak: 0,
            total: MAX_TOTAL_RECONNECTS,
        };
        assert!(!should_auto_reconnect(
            true,
            Some(ErrorCategory::Network),
            &worn_out,
            false
        ));
        let almost = ReconnectBudget {
            streak: 0,
            total: MAX_TOTAL_RECONNECTS - 1,
        };
        assert!(should_auto_reconnect(
            true,
            Some(ErrorCategory::Network),
            &almost,
            false
        ));
    }

    #[test]
    fn resize_while_connecting_is_stored_and_not_an_error() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        // No session yet: this used to fail with "not ready" and drop the size.
        manager.resize(&id, 132, 43).expect("stored, not an error");
        assert_eq!(manager.geometry(&id), Some((132, 43)));
        manager.resize(&id, 100, 30).expect("newest wins");
        assert_eq!(manager.geometry(&id), Some((100, 30)));
    }

    /// Regression test (attach while streaming): a view that registers while
    /// output is being delivered used to lose the bytes between its snapshot
    /// and its registration. Every view must see exactly the stream.
    #[test]
    fn a_view_attached_while_output_streams_sees_every_byte_exactly_once() {
        let manager = Arc::new(SessionManager::new());
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let stream = manager.stream(&id).expect("stream");
        let total: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();

        let producer = {
            let stream = stream.clone();
            let total = total.clone();
            std::thread::spawn(move || {
                for chunk in total.chunks(997) {
                    deliver_data(&stream, chunk);
                    std::thread::yield_now();
                }
            })
        };
        let mut views = Vec::new();
        for _ in 0..25 {
            let (channel, log) = capturing_channel();
            futures_block(manager.attach_view(&id, "other", channel)).expect("attach");
            views.push(log);
            std::thread::sleep(Duration::from_micros(300));
        }
        producer.join().expect("producer");

        for (index, log) in views.iter().enumerate() {
            let seen = captured_bytes(log);
            assert_eq!(
                seen.len(),
                total.len(),
                "view {index}: gap or duplicate ({} of {} bytes)",
                seen.len(),
                total.len()
            );
            assert!(seen == total, "view {index}: bytes differ from the stream");
        }
    }

    /// Same property once the ring wrapped: what a late view sees is a
    /// contiguous suffix of the stream.
    #[test]
    fn a_late_view_sees_a_contiguous_suffix_after_the_ring_wrapped() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let stream = manager.stream(&id).expect("stream");
        let total: Vec<u8> = (0..(REPLAY_LIMIT as u32 * 2 + 12_345))
            .map(|i| (i % 253) as u8)
            .collect();
        let mut late = None;
        for (index, chunk) in total.chunks(4093).enumerate() {
            if index == 200 {
                let (channel, log) = capturing_channel();
                futures_block(manager.attach_view(&id, "other", channel)).expect("attach");
                late = Some(log);
            }
            deliver_data(&stream, chunk);
        }
        let seen = captured_bytes(&late.expect("attached"));
        assert!(!seen.is_empty());
        assert!(total.ends_with(&seen), "not a suffix of the stream");
    }

    #[test]
    fn a_dead_view_is_dropped_but_delivery_continues_for_the_others() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", dead_channel());
        let (channel, log) = capturing_channel();
        manager.add_view(&id, "other", channel);
        let stream = manager.stream(&id).expect("stream");
        assert_eq!(deliver_data(&stream, b"abc"), 1);
        assert_eq!(manager.view_count(&id), 1);
        assert_eq!(captured_bytes(&log), b"abc");
    }

    /// Regression test (hygiene): ended sessions that nobody shows used to
    /// stay in the map forever, with their credentials in `ConnectParams`.
    #[test]
    fn an_ended_session_is_released_once_nobody_shows_it() {
        let manager = SessionManager::new();
        let host = HostId::new();

        // Still shown: kept, so the UI can offer "Reconnect".
        let (shown, _c1) = manager.register(host, params(), "main", test_channel());
        manager.mark_task_dead(&shown);
        assert!(manager.info(&shown).is_some());
        // The window goes away: the ended session is released with it.
        manager.drop_window_views("main");
        assert!(manager.info(&shown).is_none());

        // Orphaned while running, then the task ends: released right away.
        let (orphan, _c2) = manager.register(host, params(), "gone", test_channel());
        manager.drop_window_views("gone");
        assert!(
            manager.info(&orphan).is_some(),
            "still running: not released"
        );
        manager.mark_task_dead(&orphan);
        assert!(manager.info(&orphan).is_none());
    }

    /// Starts the watcher a session's pump runs, on a session that is shown
    /// by one view of window `main`.
    fn spawn_watch(manager: &SessionManager, id: &str) -> tokio::task::JoinHandle<()> {
        let mut watch = ViewWatch::new(manager.stream(id).expect("stream"));
        tokio::spawn(async move { watch.orphaned().await })
    }

    /// Regression test (an idle session whose last view went away was never
    /// closed): the pump only looked at the views when an event woke it, so a
    /// session without output never started its grace period. Removing the
    /// last view must wake the watcher, and the session is orphaned exactly
    /// [`VIEW_GRACE`] later — not earlier.
    #[tokio::test(start_paused = true)]
    async fn an_idle_session_that_loses_its_last_view_is_orphaned_after_the_grace() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let watcher = spawn_watch(&manager, &id);
        // The watcher runs and parks: a view shows the session, nothing else
        // will ever wake it (no output).
        tokio::task::yield_now().await;
        assert!(!watcher.is_finished());
        tokio::time::sleep(Duration::from_secs(3600)).await;
        assert!(!watcher.is_finished(), "a shown session is never orphaned");

        manager.detach_window_views(&id, "main").unwrap();
        let detached = tokio::time::Instant::now();
        tokio::time::sleep(VIEW_GRACE - Duration::from_millis(1)).await;
        assert!(!watcher.is_finished(), "inside the grace period");
        tokio::time::timeout(Duration::from_secs(1), watcher)
            .await
            .expect("orphaned once the grace period is over")
            .expect("the watcher did not panic");
        assert_eq!(detached.elapsed(), VIEW_GRACE);
    }

    /// The same for a destroyed window, which releases its views through
    /// another path.
    #[tokio::test(start_paused = true)]
    async fn an_idle_session_of_a_destroyed_window_is_orphaned_after_the_grace() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let watcher = spawn_watch(&manager, &id);
        tokio::task::yield_now().await;

        manager.drop_window_views("main");
        let dropped = tokio::time::Instant::now();
        tokio::time::timeout(VIEW_GRACE * 2, watcher)
            .await
            .expect("orphaned once the grace period is over")
            .expect("the watcher did not panic");
        assert_eq!(dropped.elapsed(), VIEW_GRACE);
        // The watcher only reports; the pump closes the session.
        assert!(manager.info(&id).is_some());
    }

    /// A view that attaches within the grace period keeps the session alive,
    /// and the grace period starts anew when it goes away again.
    #[tokio::test(start_paused = true)]
    async fn a_view_that_attaches_within_the_grace_keeps_the_session() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let watcher = spawn_watch(&manager, &id);
        tokio::task::yield_now().await;

        // The tab moves: the old window detaches first, the new one attaches
        // a few seconds later, when its webview has loaded.
        manager.detach_window_views(&id, "main").unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        manager
            .attach_view(&id, "new", test_channel())
            .await
            .unwrap();
        // Far beyond the first grace period: the session is shown, so alive.
        tokio::time::sleep(VIEW_GRACE * 6).await;
        assert!(!watcher.is_finished(), "attached in time: not orphaned");
        assert!(manager.info(&id).is_some());

        // The new window goes away for good: the grace period counts from now,
        // not from the first detach.
        manager.detach_window_views(&id, "new").unwrap();
        let detached = tokio::time::Instant::now();
        tokio::time::timeout(VIEW_GRACE * 2, watcher)
            .await
            .expect("orphaned after the second grace period")
            .expect("the watcher did not panic");
        assert_eq!(detached.elapsed(), VIEW_GRACE);
    }

    /// A view that was attached and released again while the watcher was
    /// waiting for the old deadline restarts the grace period; the session is
    /// not closed on the strength of the first detach.
    #[tokio::test(start_paused = true)]
    async fn the_grace_period_restarts_when_a_view_came_and_went() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let watcher = spawn_watch(&manager, &id);
        tokio::task::yield_now().await;

        manager.detach_window_views(&id, "main").unwrap();
        tokio::time::sleep(Duration::from_secs(6)).await;
        manager
            .attach_view(&id, "new", test_channel())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        manager.detach_window_views(&id, "new").unwrap();
        let detached = tokio::time::Instant::now();

        // The first deadline (10 s after the first detach) passes: too early
        // to close, the last view left only 2 s ago.
        tokio::time::sleep(Duration::from_secs(4) + Duration::from_millis(500)).await;
        assert!(!watcher.is_finished(), "counted from the last detach");
        tokio::time::timeout(VIEW_GRACE, watcher)
            .await
            .expect("orphaned after the full grace period")
            .expect("the watcher did not panic");
        assert_eq!(detached.elapsed(), VIEW_GRACE);
    }

    #[tokio::test]
    async fn output_chunks_merge_up_to_the_first_other_event() {
        let (tx, mut rx) = mpsc::channel(16);
        tx.send(SessionEvent::Data(b"bc".to_vec())).await.unwrap();
        tx.send(SessionEvent::Data(b"d".to_vec())).await.unwrap();
        tx.send(SessionEvent::Status(SessionStatus::Ready))
            .await
            .unwrap();
        tx.send(SessionEvent::Data(b"later".to_vec()))
            .await
            .unwrap();
        let (merged, next) = coalesce(b"a".to_vec(), &mut rx).await;
        assert_eq!(merged, b"abcd");
        assert!(matches!(
            next,
            Some(SessionEvent::Status(SessionStatus::Ready))
        ));
    }

    #[tokio::test]
    async fn merged_output_never_exceeds_the_update_limit() {
        let (tx, mut rx) = mpsc::channel(16);
        tx.send(SessionEvent::Data(vec![1; 40 * 1024]))
            .await
            .unwrap();
        let (merged, next) = coalesce(vec![0; 40 * 1024], &mut rx).await;
        assert_eq!(merged.len(), 40 * 1024);
        assert!(matches!(next, Some(SessionEvent::Data(rest)) if rest.len() == 40 * 1024));
    }

    #[tokio::test(start_paused = true)]
    async fn a_keystroke_echo_is_forwarded_without_waiting() {
        let (_tx, mut rx) = mpsc::channel::<SessionEvent>(16);
        let started = tokio::time::Instant::now();
        let (merged, next) = coalesce(b"x".to_vec(), &mut rx).await;
        assert_eq!(merged, b"x");
        assert!(next.is_none());
        assert_eq!(started.elapsed(), Duration::ZERO, "no coalescing delay");
    }

    #[tokio::test(start_paused = true)]
    async fn streaming_output_waits_at_most_the_coalesce_window() {
        let (_tx, mut rx) = mpsc::channel::<SessionEvent>(16);
        let started = tokio::time::Instant::now();
        let (merged, _) = coalesce(vec![7; COALESCE_MIN_WAIT], &mut rx).await;
        assert_eq!(merged.len(), COALESCE_MIN_WAIT);
        assert_eq!(started.elapsed(), COALESCE_WINDOW);
    }

    // ---- port forward reports ----

    fn listening(id: u32, port: u16) -> ForwardUpdate {
        ForwardUpdate::Listening {
            id,
            kind: ForwardKind::Local,
            address: "127.0.0.1".into(),
            port,
        }
    }

    /// The exact shape the frontend reads (`types.ts`).
    #[test]
    fn forward_updates_serialize_for_the_frontend_contract() {
        let json = serde_json::to_value(SessionUpdate::Forward {
            forward: listening(2, 8080),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "forward",
                "forward": {
                    "state": "listening", "id": 2, "kind": "local",
                    "address": "127.0.0.1", "port": 8080
                }
            })
        );
        let json = serde_json::to_value(SessionUpdate::Forward {
            forward: ForwardUpdate::Failed {
                id: 0,
                kind: ForwardKind::Dynamic,
                reason: "the port is already in use".into(),
            },
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "forward",
                "forward": {
                    "state": "failed", "id": 0, "kind": "dynamic",
                    "reason": "the port is already in use"
                }
            })
        );
    }

    #[test]
    fn the_engines_forward_status_converts_without_loss() {
        let status = ForwardStatus {
            id: 4,
            kind: ForwardKind::Remote,
            state: ForwardState::Listening {
                address: "localhost".into(),
                port: 9000,
            },
        };
        assert_eq!(
            ForwardUpdate::from(&status),
            ForwardUpdate::Listening {
                id: 4,
                kind: ForwardKind::Remote,
                address: "localhost".into(),
                port: 9000
            }
        );
        let failed = ForwardStatus {
            id: 1,
            kind: ForwardKind::Local,
            state: ForwardState::Failed {
                reason: "nope".into(),
            },
        };
        assert!(matches!(
            ForwardUpdate::from(&failed),
            ForwardUpdate::Failed { id: 1, .. }
        ));
    }

    #[test]
    fn a_view_that_attaches_later_is_told_what_the_forwards_are() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let stream = manager.stream(&id).expect("stream");
        deliver_status(&stream, SessionStatusDto::Ready);
        deliver_forward(&stream, listening(1, 8080));
        deliver_forward(&stream, listening(0, 1080));
        // A newer report for the same forward replaces the older one.
        deliver_forward(
            &stream,
            ForwardUpdate::Failed {
                id: 1,
                kind: ForwardKind::Local,
                reason: "died".into(),
            },
        );

        let (channel, log) = capturing_channel();
        futures_block(manager.attach_view(&id, "other", channel)).expect("attach");
        let seen: Vec<ForwardUpdate> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|update| match update {
                SessionUpdate::Forward { forward } => Some(forward.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0].id(), 0);
        assert!(matches!(seen[1], ForwardUpdate::Failed { id: 1, .. }));
        // The status comes before the forwards it qualifies.
        let first = log.lock().unwrap().first().cloned();
        assert!(
            matches!(first, Some(SessionUpdate::Status { .. })),
            "{first:?}"
        );

        let info = manager.info(&id).expect("info");
        assert_eq!(info.forwards.len(), 2);
    }

    /// The forwards live and die with the connection: any status but `ready`
    /// (reconnecting, closed, failed, connecting again) forgets them, so a
    /// view that attaches after a drop is not told of listeners that are gone.
    #[test]
    fn only_a_ready_session_keeps_its_forwards() {
        let manager = SessionManager::new();
        let (id, _cancel) = manager.register(HostId::new(), params(), "main", test_channel());
        let stream = manager.stream(&id).expect("stream");
        for status in [
            SessionStatusDto::Reconnecting {
                attempt: 1,
                delay_ms: 10,
                last_error: "x".into(),
            },
            SessionStatusDto::Connecting,
            SessionStatusDto::Closed {
                reason: "bye".into(),
            },
        ] {
            deliver_status(&stream, SessionStatusDto::Ready);
            deliver_forward(&stream, listening(0, 8080));
            assert_eq!(manager.info(&id).expect("info").forwards.len(), 1);
            deliver_status(&stream, status.clone());
            assert!(
                manager.info(&id).expect("info").forwards.is_empty(),
                "{status:?} forgets the forwards"
            );
        }
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

    /// A channel that records every update it receives.
    fn capturing_channel() -> (Channel<SessionUpdate>, Arc<Mutex<Vec<SessionUpdate>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let channel = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                if let Ok(update) = serde_json::from_str::<SessionUpdate>(&json) {
                    sink.lock().unwrap().push(update);
                }
            }
            Ok(())
        });
        (channel, log)
    }

    /// The output bytes a channel received, in order.
    fn captured_bytes(log: &Mutex<Vec<SessionUpdate>>) -> Vec<u8> {
        let mut out = Vec::new();
        for update in log.lock().unwrap().iter() {
            if let SessionUpdate::Data { base64 } = update {
                out.extend(
                    base64::engine::general_purpose::STANDARD
                        .decode(base64)
                        .unwrap(),
                );
            }
        }
        out
    }

    /// A channel whose window is gone: every send fails.
    fn dead_channel() -> Channel<SessionUpdate> {
        Channel::new(|_body| Err(tauri::Error::WebviewNotFound))
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
