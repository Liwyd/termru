//! The plumbing shared by the questions the engine asks the user mid-
//! connection: host-key trust ([`super::host_keys`]) and authentication
//! ([`super::auth_prompts`]).
//!
//! A question belongs to **one session** and goes to the **windows that show
//! it** (a tab can live in a window of its own; asking every window would
//! make several dialogs fight over one answer). The connect task registers a
//! pending entry, the event is emitted, and the connect task waits for
//! `…_resolve` to fill the entry.
//!
//! An entry never outlives its question:
//!
//! * answered: [`PromptHub::resolve`] removes it;
//! * timed out, or the connect task was dropped (the session was closed or
//!   cancelled while the question was open): the entry's guard removes it;
//! * the session ended: [`PromptHub::cancel_session`] removes **that
//!   session's** entries and wakes their waiters (they see "no answer").
//!
//! Whenever an entry goes away without an answer the windows are told
//! (`…-closed` event), so a dialog does not keep asking a question nobody
//! listens to. The registry is bounded; when it is full the *new* question is
//! refused — an older one a person may be reading right now is never evicted.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::oneshot;

/// Most questions waiting for a person at once, over all sessions.
pub(crate) const MAX_PENDING: usize = 64;

/// What the hub needs to know about sessions (implemented by the session
/// manager).
pub(crate) trait SessionDirectory: Send + Sync {
    /// The distinct labels of the windows that show the session.
    fn view_windows(&self, session_id: &str) -> Vec<String>;
    /// The host a session connects to, as the frontend knows it.
    fn host_id(&self, session_id: &str) -> Option<String>;
}

/// Delivers an event to one window (Tauri in the app, a recorder in tests).
pub(crate) trait PromptEmitter: Send + Sync {
    /// `true` when the event was handed to the window.
    fn emit(&self, window: &str, event: &str, payload: &Value) -> bool;
}

struct Entry<A> {
    session_id: String,
    /// The windows the question was sent to (they are told when it ends).
    windows: Vec<String>,
    reply: oneshot::Sender<A>,
}

/// What a finished entry leaves behind.
struct Gone {
    id: String,
    windows: Vec<String>,
}

/// The registry of questions waiting for an answer of type `A`.
pub(crate) struct PendingMap<A> {
    entries: Mutex<HashMap<String, Entry<A>>>,
    next: AtomicU64,
    prefix: &'static str,
    capacity: usize,
}

impl<A> PendingMap<A> {
    pub(crate) fn new(prefix: &'static str, capacity: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            prefix,
            capacity,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry<A>>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers a question; `None` when the registry is full.
    pub(crate) fn register(
        &self,
        session_id: &str,
        windows: Vec<String>,
    ) -> Option<(String, oneshot::Receiver<A>)> {
        let mut entries = self.lock();
        if entries.len() >= self.capacity {
            return None;
        }
        let seq = self.next.fetch_add(1, Ordering::Relaxed);
        let id = format!("{}{seq}", self.prefix);
        let (reply, receiver) = oneshot::channel();
        entries.insert(
            id.clone(),
            Entry {
                session_id: session_id.to_string(),
                windows,
                reply,
            },
        );
        Some((id, receiver))
    }

    /// Answers a pending question. `Some` only when a live waiter received
    /// the answer: an unknown, repeated or abandoned id answers nothing, so
    /// a stale reply can never grant anything.
    fn resolve(&self, id: &str, answer: A) -> Option<Vec<String>> {
        let entry = self.lock().remove(id)?;
        entry.reply.send(answer).is_ok().then_some(entry.windows)
    }

    /// Removes a question without answering it.
    fn forget(&self, id: &str) -> Option<Gone> {
        self.lock().remove(id).map(|entry| Gone {
            id: id.to_string(),
            windows: entry.windows,
        })
    }

    /// Removes every question of one session (dropping the senders wakes the
    /// waiters). Other sessions' questions are untouched.
    fn cancel_session(&self, session_id: &str) -> Vec<Gone> {
        let mut entries = self.lock();
        let ids: Vec<String> = entries
            .iter()
            .filter(|(_, entry)| entry.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                entries.remove(&id).map(|entry| Gone {
                    id,
                    windows: entry.windows,
                })
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }
}

/// The identifying part of a question's event payload.
pub(crate) struct Envelope {
    pub id: String,
    pub session_id: String,
    pub host_id: Option<String>,
}

/// How a question ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Asked<A> {
    Answer(A),
    /// Nobody answered in time.
    TimedOut,
    /// Nobody could answer: no window shows the session, the registry was
    /// full, or the session ended while the question was open.
    Unanswerable,
}

/// One kind of question (host key, authentication) and its event names.
pub(crate) struct PromptHub<A> {
    pending: PendingMap<A>,
    directory: Arc<dyn SessionDirectory>,
    emitter: Arc<dyn PromptEmitter>,
    event: &'static str,
    closed_event: &'static str,
}

impl<A: Send + 'static> PromptHub<A> {
    pub(crate) fn new(
        prefix: &'static str,
        event: &'static str,
        closed_event: &'static str,
        directory: Arc<dyn SessionDirectory>,
        emitter: Arc<dyn PromptEmitter>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pending: PendingMap::new(prefix, MAX_PENDING),
            directory,
            emitter,
            event,
            closed_event,
        })
    }

    /// Asks the windows of `session_id` and waits for the answer.
    ///
    /// `build` makes the event payload from the question's envelope; `None`
    /// (it could not be serialized) counts as unanswerable.
    pub(crate) async fn ask(
        self: &Arc<Self>,
        session_id: &str,
        timeout: Duration,
        build: impl FnOnce(&Envelope) -> Option<Value>,
    ) -> Asked<A> {
        let windows = self.directory.view_windows(session_id);
        if windows.is_empty() {
            tracing::debug!(
                session_id,
                "no window shows the session; the question is dropped"
            );
            return Asked::Unanswerable;
        }
        let Some((id, receiver)) = self.pending.register(session_id, windows.clone()) else {
            tracing::warn!("too many questions are waiting for an answer; refusing a new one");
            return Asked::Unanswerable;
        };
        // From here on the entry is removed when this future ends, however
        // it ends (answer, timeout, or being dropped with the connect task).
        let _guard = Guard {
            hub: self.clone(),
            id: id.clone(),
        };

        let envelope = Envelope {
            id,
            session_id: session_id.to_string(),
            host_id: self.directory.host_id(session_id),
        };
        let Some(payload) = build(&envelope) else {
            return Asked::Unanswerable;
        };
        let delivered = windows
            .iter()
            .filter(|window| self.emitter.emit(window, self.event, &payload))
            .count();
        if delivered == 0 {
            return Asked::Unanswerable;
        }

        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(answer)) => Asked::Answer(answer),
            // The sender went away: the session ended.
            Ok(Err(_)) => Asked::Unanswerable,
            Err(_) => Asked::TimedOut,
        }
    }

    /// Delivers the user's answer. `true` only when the waiter got it.
    pub(crate) fn resolve(&self, id: &str, answer: A) -> bool {
        match self.pending.resolve(id, answer) {
            Some(windows) => {
                // The answering window already dropped it; the others must.
                self.announce_closed(id, &windows);
                true
            }
            None => false,
        }
    }

    /// The session ended (closed, cancelled, orphaned): its questions go.
    pub(crate) fn cancel_session(&self, session_id: &str) -> usize {
        let gone = self.pending.cancel_session(session_id);
        for entry in &gone {
            self.announce_closed(&entry.id, &entry.windows);
        }
        gone.len()
    }

    #[cfg(test)]
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    pub(crate) fn is_pending(&self, id: &str) -> bool {
        self.pending.contains(id)
    }

    fn announce_closed(&self, id: &str, windows: &[String]) {
        let payload = serde_json::json!({ "id": id });
        for window in windows {
            self.emitter.emit(window, self.closed_event, &payload);
        }
    }
}

/// Removes the question's entry when the waiting future ends.
struct Guard<A: Send + 'static> {
    hub: Arc<PromptHub<A>>,
    id: String,
}

impl<A: Send + 'static> Drop for Guard<A> {
    fn drop(&mut self) {
        // Still there: nobody answered (timeout, cancelled connect). An
        // answered question was removed by `resolve`.
        if let Some(gone) = self.hub.pending.forget(&self.id) {
            self.hub.announce_closed(&gone.id, &gone.windows);
        }
    }
}

// ---------- Tauri ----------

/// Emits to one window of the app.
pub(crate) struct TauriEmitter {
    pub app: tauri::AppHandle,
}

impl PromptEmitter for TauriEmitter {
    fn emit(&self, window: &str, event: &str, payload: &Value) -> bool {
        use tauri::Emitter;
        // The frontend listens per window (see `ipc-backend.ts`), so a
        // targeted emit reaches exactly that window.
        self.app
            .emit_to(tauri::EventTarget::webview_window(window), event, payload)
            .is_ok()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Fakes for the broker tests.

    use super::*;

    /// A session directory with a fixed answer per session.
    #[derive(Default)]
    pub(crate) struct FakeDirectory {
        windows: Mutex<HashMap<String, Vec<String>>>,
    }

    impl FakeDirectory {
        pub(crate) fn show(&self, session: &str, windows: &[&str]) {
            self.windows.lock().unwrap().insert(
                session.to_string(),
                windows.iter().map(|w| (*w).to_string()).collect(),
            );
        }
    }

    impl SessionDirectory for FakeDirectory {
        fn view_windows(&self, session_id: &str) -> Vec<String> {
            self.windows
                .lock()
                .unwrap()
                .get(session_id)
                .cloned()
                .unwrap_or_default()
        }

        fn host_id(&self, session_id: &str) -> Option<String> {
            Some(format!("host-of-{session_id}"))
        }
    }

    /// Records every emitted event.
    #[derive(Default)]
    pub(crate) struct RecordingEmitter {
        pub events: Mutex<Vec<(String, String, Value)>>,
    }

    impl RecordingEmitter {
        pub(crate) fn of(&self, event: &str) -> Vec<(String, Value)> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, name, _)| name == event)
                .map(|(window, _, payload)| (window.clone(), payload.clone()))
                .collect()
        }
    }

    impl PromptEmitter for RecordingEmitter {
        fn emit(&self, window: &str, event: &str, payload: &Value) -> bool {
            self.events.lock().unwrap().push((
                window.to_string(),
                event.to_string(),
                payload.clone(),
            ));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{FakeDirectory, RecordingEmitter};
    use super::*;

    const EVENT: &str = "test-prompt";
    const CLOSED: &str = "test-prompt-closed";

    fn hub() -> (
        Arc<PromptHub<bool>>,
        Arc<FakeDirectory>,
        Arc<RecordingEmitter>,
    ) {
        let directory = Arc::new(FakeDirectory::default());
        let emitter = Arc::new(RecordingEmitter::default());
        let hub = PromptHub::new("t", EVENT, CLOSED, directory.clone(), emitter.clone());
        (hub, directory, emitter)
    }

    fn payload(envelope: &Envelope) -> Option<Value> {
        Some(
            serde_json::json!({ "id": envelope.id, "session_id": envelope.session_id,
            "host_id": envelope.host_id }),
        )
    }

    /// The id of the question the emitter was just given.
    async fn first_prompt_id(emitter: &RecordingEmitter) -> String {
        loop {
            if let Some((_, payload)) = emitter.of(EVENT).first() {
                return payload["id"].as_str().unwrap().to_string();
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_answer_reaches_the_waiter_and_closes_the_other_windows() {
        let (hub, directory, emitter) = hub();
        directory.show("s1", &["main", "second"]);
        let waiter = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.ask("s1", Duration::from_secs(60), payload).await })
        };
        let id = first_prompt_id(&emitter).await;
        // The question went to both windows that show the session.
        let windows: Vec<String> = emitter.of(EVENT).into_iter().map(|(w, _)| w).collect();
        assert_eq!(windows, ["main", "second"]);
        assert_eq!(emitter.of(EVENT)[0].1["host_id"], "host-of-s1");

        assert!(hub.resolve(&id, true));
        assert_eq!(waiter.await.unwrap(), Asked::Answer(true));
        assert_eq!(hub.pending_len(), 0);
        let closed: Vec<String> = emitter.of(CLOSED).into_iter().map(|(w, _)| w).collect();
        assert_eq!(closed, ["main", "second"], "every window drops the dialog");
    }

    #[tokio::test(start_paused = true)]
    async fn a_question_nobody_can_see_is_not_asked() {
        let (hub, _directory, emitter) = hub();
        let outcome = hub.ask("ghost", Duration::from_secs(60), payload).await;
        assert_eq!(outcome, Asked::Unanswerable);
        assert!(emitter.of(EVENT).is_empty());
        assert_eq!(hub.pending_len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_timeout_cleans_up_and_tells_the_windows() {
        let (hub, directory, emitter) = hub();
        directory.show("s1", &["main"]);
        let outcome = hub.ask("s1", Duration::from_secs(120), payload).await;
        assert_eq!(outcome, Asked::TimedOut);
        assert_eq!(
            hub.pending_len(),
            0,
            "the entry must not outlive the question"
        );
        assert_eq!(emitter.of(CLOSED).len(), 1, "the dialog must go away");
    }

    /// The connect task is dropped while the question is open (the session
    /// was closed or cancelled): the entry goes with it.
    #[tokio::test(start_paused = true)]
    async fn dropping_the_waiter_cleans_up() {
        let (hub, directory, emitter) = hub();
        directory.show("s1", &["main"]);
        let waiter = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.ask("s1", Duration::from_secs(600), payload).await })
        };
        let id = first_prompt_id(&emitter).await;
        assert!(hub.is_pending(&id));
        waiter.abort();
        let _ = waiter.await;
        assert!(!hub.is_pending(&id));
        assert_eq!(hub.pending_len(), 0);
        assert_eq!(emitter.of(CLOSED).len(), 1);
        assert!(!hub.resolve(&id, true), "a late answer finds nobody");
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_session_wakes_its_waiter_and_leaves_other_sessions_alone() {
        let (hub, directory, emitter) = hub();
        directory.show("s1", &["main"]);
        directory.show("s2", &["main"]);
        let first = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.ask("s1", Duration::from_secs(600), payload).await })
        };
        let second = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.ask("s2", Duration::from_secs(600), payload).await })
        };
        while emitter.of(EVENT).len() < 2 {
            tokio::task::yield_now().await;
        }
        assert_eq!(hub.pending_len(), 2);

        assert_eq!(hub.cancel_session("s1"), 1);
        assert_eq!(first.await.unwrap(), Asked::Unanswerable);
        assert_eq!(hub.pending_len(), 1, "the other session's question stays");

        let other_id = emitter
            .of(EVENT)
            .into_iter()
            .map(|(_, p)| p)
            .find(|p| p["session_id"] == "s2")
            .map(|p| p["id"].as_str().unwrap().to_string())
            .unwrap();
        assert!(hub.resolve(&other_id, false));
        assert_eq!(second.await.unwrap(), Asked::Answer(false));
        assert_eq!(hub.cancel_session("s1"), 0, "nothing left to cancel");
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_questions_get_distinct_ids_and_their_own_answers() {
        let (hub, directory, emitter) = hub();
        let mut waiters = Vec::new();
        for n in 0..5 {
            let session = format!("s{n}");
            directory.show(&session, &["main"]);
            let hub = hub.clone();
            waiters.push(tokio::spawn(async move {
                hub.ask(&session, Duration::from_secs(600), payload).await
            }));
        }
        while emitter.of(EVENT).len() < 5 {
            tokio::task::yield_now().await;
        }
        let mut ids: Vec<(String, String)> = emitter
            .of(EVENT)
            .into_iter()
            .map(|(_, p)| {
                (
                    p["session_id"].as_str().unwrap().to_string(),
                    p["id"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        ids.sort();
        let distinct: std::collections::HashSet<_> = ids.iter().map(|(_, id)| id).collect();
        assert_eq!(distinct.len(), 5, "{ids:?}");

        // Answer in reverse order: each waiter must get *its* answer.
        for (session, id) in ids.iter().rev() {
            let answer = session.as_str() == "s3";
            assert!(hub.resolve(id, answer));
        }
        for (n, waiter) in waiters.into_iter().enumerate() {
            assert_eq!(waiter.await.unwrap(), Asked::Answer(n == 3), "session s{n}");
        }
        assert_eq!(hub.pending_len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn stale_and_repeated_answers_find_nobody() {
        let (hub, directory, emitter) = hub();
        directory.show("s1", &["main"]);
        assert!(
            !hub.resolve("t-nope", true),
            "an unknown id answers nothing"
        );
        let waiter = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.ask("s1", Duration::from_secs(600), payload).await })
        };
        let id = first_prompt_id(&emitter).await;
        assert!(hub.resolve(&id, true));
        assert!(!hub.resolve(&id, false), "the second answer has no waiter");
        assert_eq!(waiter.await.unwrap(), Asked::Answer(true));
    }

    /// Regression: a full registry used to be **cleared** (every waiting
    /// question was silently denied). It now refuses the new question and
    /// leaves the older ones alone.
    #[tokio::test(start_paused = true)]
    async fn a_full_registry_refuses_the_new_question_and_keeps_the_old_ones() {
        let (hub, directory, emitter) = hub();
        let mut waiters = Vec::new();
        for n in 0..MAX_PENDING {
            let session = format!("s{n}");
            directory.show(&session, &["main"]);
            let hub = hub.clone();
            waiters.push(tokio::spawn(async move {
                hub.ask(&session, Duration::from_secs(600), payload).await
            }));
        }
        while emitter.of(EVENT).len() < MAX_PENDING {
            tokio::task::yield_now().await;
        }
        assert_eq!(hub.pending_len(), MAX_PENDING);

        directory.show("late", &["main"]);
        let late = hub.ask("late", Duration::from_secs(600), payload).await;
        assert_eq!(late, Asked::Unanswerable, "the newcomer is refused");
        assert_eq!(hub.pending_len(), MAX_PENDING, "nothing was evicted");

        // The oldest question still gets its answer.
        let oldest = emitter
            .of(EVENT)
            .into_iter()
            .map(|(_, p)| p)
            .find(|p| p["session_id"] == "s0")
            .map(|p| p["id"].as_str().unwrap().to_string())
            .unwrap();
        assert!(hub.resolve(&oldest, true));
        assert_eq!(waiters.remove(0).await.unwrap(), Asked::Answer(true));
        for waiter in waiters {
            waiter.abort();
        }
    }
}
