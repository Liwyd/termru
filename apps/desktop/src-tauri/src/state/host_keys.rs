//! Interactive host-key trust decisions routed to the UI.
//!
//! `termru_ssh::HostKeyPrompt` is implemented by [`HostKeyPrompts`]: the
//! connect task registers a pending decision ([`PendingHostKeys`]), the
//! frontend receives a `host-key-prompt` event, and answers via
//! `host_key_prompt_resolve`. Dropped/timeout decisions default to **reject**
//! (never trust by default).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use termru_ssh::{HostKeyPrompt, HostKeyPromptRequest};

const EVENT_NAME: &str = "host-key-prompt";
/// Bound on pending prompts; the oldest are evicted (deny) beyond this.
const MAX_PENDING: usize = 32;

/// Payload of the `host-key-prompt` frontend event.
#[derive(Debug, Clone, Serialize)]
pub struct HostKeyPromptMessage {
    pub id: String,
    pub request: HostKeyPromptRequest,
}

type Pending = HashMap<String, (HostKeyPromptRequest, tokio::sync::oneshot::Sender<bool>)>;

/// The pending-prompt registry (pure logic; unit-testable without a window).
pub struct PendingHostKeys {
    pending: Mutex<Pending>,
    next: AtomicU64,
}

impl PendingHostKeys {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers a decision; returns the id, the event payload to emit, and
    /// the receiver that resolves to the user's verdict.
    pub fn register(
        &self,
        request: HostKeyPromptRequest,
    ) -> (
        String,
        HostKeyPromptMessage,
        tokio::sync::oneshot::Receiver<bool>,
    ) {
        let seq = self.next.fetch_add(1, Ordering::Relaxed);
        let id = format!("hk{seq}");
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut guard = self.lock();
            if guard.len() >= MAX_PENDING {
                // Evicted senders drop → their `decide` awaits resolve to
                // `false`; no unbounded growth from vanished connect tasks.
                guard.clear();
            }
            guard.insert(id.clone(), (request.clone(), tx));
        }
        let message = HostKeyPromptMessage {
            id: id.clone(),
            request,
        };
        (id, message, rx)
    }

    /// Answers (or cancels) a pending prompt. Returns `true` only when a
    /// live waiter received this answer. Unknown/duplicate ids return
    /// `false` — a stale answer can never grant trust.
    pub fn resolve(&self, id: &str, trust: bool) -> bool {
        match self.lock().remove(id) {
            // `send` fails only when the waiter is gone (timeout/closed).
            Some((_, sender)) => sender.send(trust).is_ok(),
            None => false,
        }
    }
}

impl Default for PendingHostKeys {
    fn default() -> Self {
        Self::new()
    }
}

/// `HostKeyPrompt` implementation backed by the UI: emits the prompt event
/// and awaits the frontend's answer (default-deny on any failure).
pub struct HostKeyPrompts {
    app: AppHandle,
    pending: PendingHostKeys,
}

impl HostKeyPrompts {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            pending: PendingHostKeys::new(),
        }
    }

    pub fn resolve(&self, id: &str, trust: bool) -> bool {
        self.pending.resolve(id, trust)
    }
}

#[async_trait::async_trait]
impl HostKeyPrompt for HostKeyPrompts {
    async fn decide(&self, request: HostKeyPromptRequest) -> bool {
        let (id, message, rx) = self.pending.register(request);
        if self.app.emit(EVENT_NAME, message).is_err() {
            self.pending.resolve(&id, false);
            return false;
        }
        // Default-deny: a dropped receiver (timeout, app shutdown) resolves
        // to `false`, never trust.
        rx.await.unwrap_or(false)
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request(marker: &str) -> HostKeyPromptRequest {
        HostKeyPromptRequest::Unknown {
            host: marker.to_string(),
            port: 22,
            key: termru_ssh::HostKeyEntry {
                key_type: "ssh-ed25519".into(),
                fingerprint: "SHA256:abc".into(),
            },
        }
    }

    #[test]
    fn register_then_resolve_delivers_verdict() {
        let pending = PendingHostKeys::new();
        let (id, message, rx) = pending.register(sample_request("a"));
        assert_eq!(message.id, id);
        assert!(pending.resolve(&id, true));
        // Receiver side of the oneshot (resolve consumed the sender).
        let accepted = futures_block(rx).unwrap_or(false);
        assert!(accepted);
    }

    #[test]
    fn unknown_id_is_rejected() {
        let pending = PendingHostKeys::new();
        assert!(!pending.resolve("hk-nope", true), "stale id must not trust");
    }

    #[test]
    fn duplicate_resolve_only_affects_first_call() {
        let pending = PendingHostKeys::new();
        let (id, _, rx) = pending.register(sample_request("b"));
        assert!(pending.resolve(&id, true));
        assert!(!pending.resolve(&id, false), "second answer has no waiter");
        assert!(futures_block(rx).unwrap_or(false));
    }

    #[test]
    fn eviction_bounds_the_pending_map_and_denies_evicted() {
        let pending = PendingHostKeys::new();
        // Keep receivers alive so a resolve on an evicted id can only return
        // false because the entry is gone, not because the waiter vanished.
        let mut keep: Vec<(String, tokio::sync::oneshot::Receiver<bool>)> = Vec::new();
        for index in 0..(MAX_PENDING + 1) {
            let (id, _, rx) = pending.register(sample_request(&format!("h{index}")));
            keep.push((id, rx));
        }
        assert!(
            !pending.resolve(&keep[0].0, true),
            "evicted prompt must deny even with a live waiter"
        );
        let (latest, _, rx) = pending.register(sample_request("latest"));
        assert!(pending.resolve(&latest, false));
        assert!(!futures_block(rx).unwrap_or(true), "reject delivered");
    }

    #[test]
    fn message_serializes_prompt_state_for_frontend() {
        let pending = PendingHostKeys::new();
        let (_, message, _rx) = pending.register(sample_request("example.test"));
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("\"state\":\"unknown\""));
        assert!(json.contains("example.test"));
        assert!(json.contains("SHA256:abc"));
    }

    fn futures_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }
}
