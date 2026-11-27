//! Interactive host-key trust decisions routed to the UI.
//!
//! `termru_ssh::HostKeyPrompt` is implemented per session by
//! [`SessionHostKeyPrompt`] (made by [`HostKeyPrompts::for_session`]): the
//! question carries the session id, goes to the windows that show the session
//! and waits for `host_key_prompt_resolve`. Everything that is not a clear
//! "trust" — a timeout, a closed session, no window to ask, a full registry —
//! is a **reject** (never trust by default). See [`super::prompts`] for the
//! lifecycle of a pending question.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use termru_ssh::{HostKeyPrompt, HostKeyPromptRequest};

use super::prompts::{Asked, Envelope, PromptEmitter, PromptHub, SessionDirectory};

pub(crate) const EVENT_NAME: &str = "host-key-prompt";
pub(crate) const CLOSED_EVENT_NAME: &str = "host-key-prompt-closed";

/// How long the registry keeps a question. A little longer than the engine's
/// own decision budget (`ConnectTimeouts::host_key_decision`, 120 s), so the
/// engine normally ends the question and reports the precise error; this is
/// the backstop.
const DECISION_TIMEOUT: Duration = Duration::from_secs(125);

/// Payload of the `host-key-prompt` frontend event.
#[derive(Debug, Clone, Serialize)]
pub struct HostKeyPromptMessage {
    pub id: String,
    pub session_id: String,
    pub request: HostKeyPromptRequest,
}

/// The host-key questions of the whole app.
pub struct HostKeyPrompts {
    hub: Arc<PromptHub<bool>>,
}

impl HostKeyPrompts {
    pub(crate) fn new(
        directory: Arc<dyn SessionDirectory>,
        emitter: Arc<dyn PromptEmitter>,
    ) -> Self {
        Self {
            hub: PromptHub::new("hk", EVENT_NAME, CLOSED_EVENT_NAME, directory, emitter),
        }
    }

    /// The prompt for one session's connection.
    pub fn for_session(&self, session_id: &str) -> Arc<dyn HostKeyPrompt> {
        Arc::new(SessionHostKeyPrompt {
            hub: self.hub.clone(),
            session_id: session_id.to_string(),
        })
    }

    /// Answers (or rejects) a pending question. `true` only when a live
    /// waiter received this answer; unknown or repeated ids return `false` —
    /// a stale answer can never grant trust.
    pub fn resolve(&self, id: &str, trust: bool) -> bool {
        self.hub.resolve(id, trust)
    }

    /// The session ended: its questions are rejected and removed.
    pub fn cancel_session(&self, session_id: &str) -> usize {
        self.hub.cancel_session(session_id)
    }
}

struct SessionHostKeyPrompt {
    hub: Arc<PromptHub<bool>>,
    session_id: String,
}

#[async_trait::async_trait]
impl HostKeyPrompt for SessionHostKeyPrompt {
    async fn decide(&self, request: HostKeyPromptRequest) -> bool {
        let asked = self
            .hub
            .ask(&self.session_id, DECISION_TIMEOUT, |envelope: &Envelope| {
                let message = HostKeyPromptMessage {
                    id: envelope.id.clone(),
                    session_id: envelope.session_id.clone(),
                    request,
                };
                serde_json::to_value(&message).ok()
            })
            .await;
        // Default-deny: only an explicit "trust" trusts.
        matches!(asked, Asked::Answer(true))
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::prompts::testing::{FakeDirectory, RecordingEmitter};
    use super::*;

    fn unknown(marker: &str, others: &[&str]) -> HostKeyPromptRequest {
        HostKeyPromptRequest::Unknown {
            host: marker.to_string(),
            port: 22,
            key: termru_ssh::HostKeyEntry {
                key_type: "ssh-ed25519".into(),
                fingerprint: "SHA256:abc".into(),
            },
            other_key_types: others.iter().map(|t| (*t).to_string()).collect(),
        }
    }

    fn setup() -> (HostKeyPrompts, Arc<FakeDirectory>, Arc<RecordingEmitter>) {
        let directory = Arc::new(FakeDirectory::default());
        let emitter = Arc::new(RecordingEmitter::default());
        let prompts = HostKeyPrompts::new(directory.clone(), emitter.clone());
        (prompts, directory, emitter)
    }

    async fn emitted_id(emitter: &RecordingEmitter) -> String {
        loop {
            if let Some((_, payload)) = emitter.of(EVENT_NAME).first() {
                return payload["id"].as_str().unwrap().to_string();
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_event_carries_the_session_the_request_and_goes_to_its_window() {
        let (prompts, directory, emitter) = setup();
        directory.show("s7", &["win-a"]);
        let prompt = prompts.for_session("s7");
        let decision =
            tokio::spawn(async move { prompt.decide(unknown("example.test", &[])).await });
        let id = emitted_id(&emitter).await;

        let events = emitter.of(EVENT_NAME);
        assert_eq!(events.len(), 1);
        let (window, payload) = &events[0];
        assert_eq!(window, "win-a");
        assert_eq!(payload["session_id"], "s7");
        assert_eq!(payload["request"]["state"], "unknown");
        assert_eq!(payload["request"]["host"], "example.test");
        assert_eq!(payload["request"]["key"]["fingerprint"], "SHA256:abc");
        assert!(
            payload["request"].get("other_key_types").is_none(),
            "no other types: the field is left out"
        );

        assert!(prompts.resolve(&id, true));
        assert!(decision.await.unwrap(), "an explicit trust trusts");
    }

    #[tokio::test(start_paused = true)]
    async fn other_known_key_types_are_part_of_the_question() {
        let (prompts, directory, emitter) = setup();
        directory.show("s1", &["main"]);
        let prompt = prompts.for_session("s1");
        let decision = tokio::spawn(async move {
            prompt
                .decide(unknown("example.test", &["ssh-rsa", "ecdsa-sha2-nistp256"]))
                .await
        });
        let id = emitted_id(&emitter).await;
        assert_eq!(
            emitter.of(EVENT_NAME)[0].1["request"]["other_key_types"],
            serde_json::json!(["ssh-rsa", "ecdsa-sha2-nistp256"])
        );
        assert!(prompts.resolve(&id, false));
        assert!(!decision.await.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn anything_but_trust_rejects() {
        let (prompts, directory, emitter) = setup();

        // No window shows the session.
        assert!(!prompts.for_session("ghost").decide(unknown("a", &[])).await);

        // The question times out.
        directory.show("s1", &["main"]);
        assert!(!prompts.for_session("s1").decide(unknown("b", &[])).await);
        assert_eq!(emitter.of(CLOSED_EVENT_NAME).len(), 1);

        // The session ends while the question is open.
        let prompt = prompts.for_session("s1");
        let decision = tokio::spawn(async move { prompt.decide(unknown("c", &[])).await });
        while emitter.of(EVENT_NAME).len() < 2 {
            tokio::task::yield_now().await;
        }
        assert_eq!(prompts.cancel_session("s1"), 1);
        assert!(!decision.await.unwrap());
    }

    #[test]
    fn unknown_ids_never_trust() {
        let (prompts, _, _) = setup();
        assert!(!prompts.resolve("hk-nope", true), "stale id must not trust");
    }

    #[test]
    fn an_unknown_question_serializes_for_the_frontend() {
        let message = HostKeyPromptMessage {
            id: "hk1".into(),
            session_id: "s1".into(),
            request: unknown("example.test", &["ssh-rsa"]),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("\"state\":\"unknown\""));
        assert!(json.contains("\"session_id\":\"s1\""));
        assert!(json.contains("example.test"));
        assert!(json.contains("SHA256:abc"));
        assert!(json.contains("\"other_key_types\":[\"ssh-rsa\"]"));
    }
}
