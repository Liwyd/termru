//! Authentication questions routed to the UI: keyboard-interactive rounds and
//! the passphrase of an encrypted key.
//!
//! `termru_ssh::AuthPrompt` is implemented per session by
//! [`SessionAuthPrompt`] (made by [`AuthPrompts::for_session`]), exactly like
//! the host-key questions (see [`super::host_keys`] and [`super::prompts`] for
//! the lifecycle): the question carries the session id, goes to the windows
//! that show the session, and waits for `auth_prompt_resolve`, which brings
//! the answers or a cancel. A timeout, a closed session, no window to ask and
//! a full registry all end the question without an answer.
//!
//! The answers are secrets: they travel as [`SecretValue`]s from the moment
//! the command receives them, are never logged, and are not part of any
//! `Debug` output.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use termru_ssh::{AuthPrompt, AuthPromptRequest, AuthPromptResponse, SecretValue};

use super::prompts::{Asked, Envelope, PromptEmitter, PromptHub, SessionDirectory};

pub(crate) const EVENT_NAME: &str = "auth-prompt";
pub(crate) const CLOSED_EVENT_NAME: &str = "auth-prompt-closed";

/// How long the registry keeps a question. A little longer than the engine's
/// own bound (`ConnectTimeouts::auth_prompt`, see [`ENGINE_PROMPT_TIMEOUT`]),
/// so the engine normally ends the question; this is the backstop.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

/// The bound the engine puts on one question: longer than [`ANSWER_TIMEOUT`],
/// so that the registry (which tells the window to close the dialog) ends it
/// first.
pub(crate) const ENGINE_PROMPT_TIMEOUT: Duration = Duration::from_secs(130);

/// Largest answer set the command accepts (a runaway frontend must not push
/// megabytes into the connect task).
const MAX_ANSWERS: usize = 32;
const MAX_ANSWER_CHARS: usize = 4096;

/// Payload of the `auth-prompt` frontend event.
#[derive(Debug, Clone, Serialize)]
pub struct AuthPromptMessage {
    pub id: String,
    pub session_id: String,
    /// The host the session connects to (the frontend stores a remembered
    /// passphrase under it, see `secret_set`).
    pub host_id: Option<String>,
    pub request: AuthPromptRequest,
}

/// What the user answered, as the command received it.
pub(crate) enum UserAnswer {
    Answers(Vec<SecretValue>),
    Cancelled,
}

/// The authentication questions of the whole app.
pub struct AuthPrompts {
    hub: Arc<PromptHub<UserAnswer>>,
    timeout: Duration,
}

impl AuthPrompts {
    pub(crate) fn new(
        directory: Arc<dyn SessionDirectory>,
        emitter: Arc<dyn PromptEmitter>,
    ) -> Self {
        Self::with_timeout(directory, emitter, ANSWER_TIMEOUT)
    }

    pub(crate) fn with_timeout(
        directory: Arc<dyn SessionDirectory>,
        emitter: Arc<dyn PromptEmitter>,
        timeout: Duration,
    ) -> Self {
        Self {
            hub: PromptHub::new("ap", EVENT_NAME, CLOSED_EVENT_NAME, directory, emitter),
            timeout,
        }
    }

    /// The prompt for one session's connection.
    pub fn for_session(&self, session_id: &str) -> Arc<dyn AuthPrompt> {
        Arc::new(SessionAuthPrompt {
            hub: self.hub.clone(),
            session_id: session_id.to_string(),
            timeout: self.timeout,
        })
    }

    /// Delivers the user's answers (`None` = the user cancelled). `true` only
    /// when a live waiter received them.
    pub fn resolve(&self, id: &str, answers: Option<Vec<String>>) -> Result<bool, String> {
        let answer = match answers {
            None => UserAnswer::Cancelled,
            Some(answers) => {
                if answers.len() > MAX_ANSWERS
                    || answers
                        .iter()
                        .any(|answer| answer.chars().count() > MAX_ANSWER_CHARS)
                {
                    return Err("the answers are too large".to_string());
                }
                UserAnswer::Answers(answers.into_iter().map(SecretValue::new).collect())
            }
        };
        Ok(self.hub.resolve(id, answer))
    }

    /// The session ended: its questions are dropped.
    pub fn cancel_session(&self, session_id: &str) -> usize {
        self.hub.cancel_session(session_id)
    }

    #[cfg(test)]
    pub(crate) fn pending(&self) -> usize {
        self.hub.pending_len()
    }
}

struct SessionAuthPrompt {
    hub: Arc<PromptHub<UserAnswer>>,
    session_id: String,
    timeout: Duration,
}

#[async_trait::async_trait]
impl AuthPrompt for SessionAuthPrompt {
    async fn ask(&self, request: AuthPromptRequest) -> AuthPromptResponse {
        let asked = self
            .hub
            .ask(&self.session_id, self.timeout, |envelope: &Envelope| {
                let message = AuthPromptMessage {
                    id: envelope.id.clone(),
                    session_id: envelope.session_id.clone(),
                    host_id: envelope.host_id.clone(),
                    request,
                };
                serde_json::to_value(&message).ok()
            })
            .await;
        match asked {
            Asked::Answer(UserAnswer::Answers(answers)) => AuthPromptResponse::Answers(answers),
            Asked::TimedOut => AuthPromptResponse::TimedOut,
            Asked::Answer(UserAnswer::Cancelled) | Asked::Unanswerable => {
                AuthPromptResponse::Cancelled
            }
        }
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::prompts::testing::{FakeDirectory, RecordingEmitter};
    use super::*;
    use termru_ssh::{AuthPromptField, AuthPromptKind, KeyIdentity};

    fn question() -> AuthPromptRequest {
        AuthPromptRequest {
            kind: AuthPromptKind::KeyboardInteractive,
            name: "Two-factor".into(),
            instructions: "Enter the code".into(),
            prompts: vec![
                AuthPromptField {
                    text: "Code: ".into(),
                    echo: true,
                },
                AuthPromptField {
                    text: "PIN: ".into(),
                    echo: false,
                },
            ],
        }
    }

    fn setup(timeout: Duration) -> (AuthPrompts, Arc<FakeDirectory>, Arc<RecordingEmitter>) {
        let directory = Arc::new(FakeDirectory::default());
        let emitter = Arc::new(RecordingEmitter::default());
        let prompts = AuthPrompts::with_timeout(directory.clone(), emitter.clone(), timeout);
        (prompts, directory, emitter)
    }

    async fn emitted(emitter: &RecordingEmitter, count: usize) -> Vec<serde_json::Value> {
        loop {
            let events = emitter.of(EVENT_NAME);
            if events.len() >= count {
                return events.into_iter().map(|(_, payload)| payload).collect();
            }
            tokio::task::yield_now().await;
        }
    }

    fn answers_of(response: AuthPromptResponse) -> Vec<String> {
        match response {
            AuthPromptResponse::Answers(answers) => answers
                .iter()
                .map(|answer| answer.expose().to_string())
                .collect(),
            other => panic!("expected answers, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_question_reaches_the_window_and_the_answers_come_back_in_order() {
        let (prompts, directory, emitter) = setup(Duration::from_secs(120));
        directory.show("s3", &["win"]);
        let prompt = prompts.for_session("s3");
        let waiter = tokio::spawn(async move { prompt.ask(question()).await });
        let payload = emitted(&emitter, 1).await.remove(0);

        assert_eq!(payload["session_id"], "s3");
        assert_eq!(payload["host_id"], "host-of-s3");
        assert_eq!(payload["request"]["kind"]["type"], "keyboard_interactive");
        assert_eq!(payload["request"]["name"], "Two-factor");
        assert_eq!(payload["request"]["prompts"][0]["text"], "Code: ");
        assert_eq!(payload["request"]["prompts"][0]["echo"], true);
        assert_eq!(payload["request"]["prompts"][1]["echo"], false);

        let id = payload["id"].as_str().unwrap();
        assert_eq!(
            prompts.resolve(id, Some(vec!["123456".into(), "9876".into()])),
            Ok(true)
        );
        assert_eq!(answers_of(waiter.await.unwrap()), ["123456", "9876"]);
        assert_eq!(prompts.pending(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_passphrase_question_names_the_identity() {
        let (prompts, directory, emitter) = setup(Duration::from_secs(120));
        directory.show("s1", &["win"]);
        let prompt = prompts.for_session("s1");
        let request = AuthPromptRequest {
            kind: AuthPromptKind::Passphrase {
                identity: Some(KeyIdentity {
                    id: "identity-1".into(),
                    name: "work key".into(),
                }),
                retry: true,
            },
            name: String::new(),
            instructions: String::new(),
            prompts: vec![AuthPromptField {
                text: "Passphrase for work key".into(),
                echo: false,
            }],
        };
        let waiter = tokio::spawn(async move { prompt.ask(request).await });
        let payload = emitted(&emitter, 1).await.remove(0);
        assert_eq!(payload["request"]["kind"]["type"], "passphrase");
        assert_eq!(payload["request"]["kind"]["identity"]["name"], "work key");
        assert_eq!(payload["request"]["kind"]["retry"], true);
        let id = payload["id"].as_str().unwrap();
        assert_eq!(prompts.resolve(id, None), Ok(true));
        assert!(matches!(
            waiter.await.unwrap(),
            AuthPromptResponse::Cancelled
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn a_cancel_is_a_cancel() {
        let (prompts, directory, emitter) = setup(Duration::from_secs(120));
        directory.show("s1", &["win"]);
        let prompt = prompts.for_session("s1");
        let waiter = tokio::spawn(async move { prompt.ask(question()).await });
        let payload = emitted(&emitter, 1).await.remove(0);
        assert_eq!(
            prompts.resolve(payload["id"].as_str().unwrap(), None),
            Ok(true)
        );
        assert!(matches!(
            waiter.await.unwrap(),
            AuthPromptResponse::Cancelled
        ));
        assert_eq!(prompts.pending(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_timeout_cleans_up_and_is_reported_as_a_timeout() {
        let (prompts, directory, emitter) = setup(Duration::from_secs(120));
        directory.show("s1", &["win"]);
        let response = prompts.for_session("s1").ask(question()).await;
        assert!(
            matches!(response, AuthPromptResponse::TimedOut),
            "{response:?}"
        );
        assert_eq!(
            prompts.pending(),
            0,
            "the entry must not outlive the question"
        );
        assert_eq!(emitter.of(CLOSED_EVENT_NAME).len(), 1, "the dialog is told");
    }

    #[tokio::test(start_paused = true)]
    async fn closing_the_session_cleans_up_its_questions_only() {
        let (prompts, directory, emitter) = setup(Duration::from_secs(600));
        directory.show("s1", &["win"]);
        directory.show("s2", &["win"]);
        let first = prompts.for_session("s1");
        let second = prompts.for_session("s2");
        let a = tokio::spawn(async move { first.ask(question()).await });
        let b = tokio::spawn(async move { second.ask(question()).await });
        let events = emitted(&emitter, 2).await;
        assert_eq!(prompts.pending(), 2);

        assert_eq!(prompts.cancel_session("s1"), 1);
        assert!(matches!(a.await.unwrap(), AuthPromptResponse::Cancelled));
        assert_eq!(prompts.pending(), 1);

        let second_id = events
            .iter()
            .find(|p| p["session_id"] == "s2")
            .and_then(|p| p["id"].as_str())
            .unwrap();
        assert_eq!(
            prompts.resolve(second_id, Some(vec!["a".into(), "b".into()])),
            Ok(true)
        );
        assert_eq!(answers_of(b.await.unwrap()), ["a", "b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_questions_are_answered_independently() {
        let (prompts, directory, emitter) = setup(Duration::from_secs(600));
        let mut waiters = Vec::new();
        for n in 0..4 {
            let session = format!("s{n}");
            directory.show(&session, &["win"]);
            let prompt = prompts.for_session(&session);
            waiters.push(tokio::spawn(async move { prompt.ask(question()).await }));
        }
        let events = emitted(&emitter, 4).await;
        for event in events.iter().rev() {
            let session = event["session_id"].as_str().unwrap();
            let id = event["id"].as_str().unwrap();
            let marker = format!("from-{session}");
            assert_eq!(
                prompts.resolve(id, Some(vec![marker.clone(), marker])),
                Ok(true)
            );
        }
        for (n, waiter) in waiters.into_iter().enumerate() {
            assert_eq!(
                answers_of(waiter.await.unwrap()),
                [format!("from-s{n}"), format!("from-s{n}")]
            );
        }
        assert_eq!(prompts.pending(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn nobody_to_ask_means_no_answer() {
        let (prompts, _directory, emitter) = setup(Duration::from_secs(120));
        let response = prompts.for_session("ghost").ask(question()).await;
        assert!(matches!(response, AuthPromptResponse::Cancelled));
        assert!(emitter.of(EVENT_NAME).is_empty());
    }

    #[test]
    fn stale_ids_and_oversized_answers_are_refused() {
        let (prompts, _, _) = setup(Duration::from_secs(120));
        assert_eq!(
            prompts.resolve("ap-nope", Some(vec!["x".into()])),
            Ok(false)
        );
        assert!(
            prompts
                .resolve("ap-nope", Some(vec!["x".repeat(MAX_ANSWER_CHARS + 1)]))
                .is_err()
        );
        assert!(
            prompts
                .resolve("ap-nope", Some(vec![String::new(); MAX_ANSWERS + 1]))
                .is_err()
        );
    }

    /// The answers are secrets: nothing the app formats may show them.
    #[test]
    fn answers_never_show_in_debug_output() {
        let response = AuthPromptResponse::Answers(vec![SecretValue::new("hunter2".into())]);
        let debug = format!("{response:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
    }
}
