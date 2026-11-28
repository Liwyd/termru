//! The authentication chain against an **in-process `russh` server**.
//!
//! A real `sshd` cannot be driven into the interesting states here: its
//! keyboard-interactive needs PAM (not usable unprivileged, so the private
//! `sshd` of `sshd.rs` runs with `UsePAM no` and has no such method), it
//! cannot be told to accept `none`, and multi-factor sequences need root-owned
//! configuration. This server speaks the same protocol, is scripted per test
//! and records every method the client tried — which is what lets the tests
//! prove that a method was *skipped* and not merely unsuccessful.
//!
//! Covered: the `none` probe (accepted and as a source of the allowed
//! methods), skipping methods the server does not list, keyboard-interactive
//! (answered from the stored password, asked of the user, wrong stored
//! password is not repeated, one-time-code questions never get the password),
//! cancel and timeout of a question, the time a human needs not counting
//! against the authentication deadline, partial success (publickey then
//! keyboard-interactive, in either configured order), passphrase prompts for
//! an encrypted key, and the banner.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test failures must panic loudly"
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use russh::keys::ssh_key::{self, LineEnding, rand_core};
use russh::keys::{PrivateKey, PublicKey};
use russh::server::{self, Auth, Msg, Server as _, Session as ServerSession, VerifiedPublicKey};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use termru_ssh::config::AcceptAllHostKeys;
use termru_ssh::{
    AuthChoice, AuthPrompt, AuthPromptKind, AuthPromptRequest, AuthPromptResponse, ConnectParams,
    ConnectTimeouts, KeyIdentity, NoAuthPrompts, SecretValue, Session, SessionEvent,
    SessionOptions, SessionStatus, SshError,
};

// ----------------------------------------------------------------- test keys --

/// Deterministic xorshift RNG (the bound `PrivateKey::random` needs), so a
/// failing run can be reproduced from the seed.
struct TestRng(u64);

impl TestRng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

impl rand_core::TryCryptoRng for TestRng {}

impl rand_core::TryRng for TestRng {
    type Error = core::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.next() as u32)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.next())
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        for chunk in dst.chunks_mut(8) {
            let word = self.next().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        Ok(())
    }
}

fn key(seed: u64) -> PrivateKey {
    PrivateKey::random(&mut TestRng::new(seed), ssh_key::Algorithm::Ed25519).unwrap()
}

fn write_key(dir: &Path, name: &str, key: &PrivateKey) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, key.to_openssh(LineEnding::LF).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}

fn encrypted(dir: &Path, name: &str, key: &PrivateKey, passphrase: &str) -> PathBuf {
    let locked = key.encrypt(&mut TestRng::new(77), passphrase).unwrap();
    write_key(dir, name, &locked)
}

// ----------------------------------------------------------------- the server --

/// One scripted keyboard-interactive question set.
#[derive(Clone)]
struct Questions {
    prompts: Vec<(&'static str, bool)>,
    /// The answers that log the client in.
    expected: Vec<&'static str>,
}

#[derive(Clone, Default)]
struct Policy {
    accept_none: bool,
    /// Methods the server lists when it rejects (single-factor setups).
    offered: Vec<MethodKind>,
    password: Option<&'static str>,
    keys: Vec<PublicKey>,
    questions: Option<Questions>,
    /// Methods that must all succeed, in this order (multi-factor).
    require: Vec<MethodKind>,
    banner: Option<&'static str>,
}

#[derive(Default)]
struct Observed {
    /// `method:ok` / `method:bad`, in order.
    attempts: Vec<String>,
    /// Every keyboard-interactive answer received.
    answers: Vec<String>,
}

#[derive(Clone)]
struct Factory {
    policy: Policy,
    observed: Arc<Mutex<Observed>>,
}

struct Handler {
    policy: Policy,
    observed: Arc<Mutex<Observed>>,
    done: Vec<MethodKind>,
    kbd_rounds: usize,
}

impl server::Server for Factory {
    type Handler = Handler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Handler {
        Handler {
            policy: self.policy.clone(),
            observed: self.observed.clone(),
            done: Vec::new(),
            kbd_rounds: 0,
        }
    }
}

impl Handler {
    fn note(&self, entry: String) {
        self.observed.lock().unwrap().attempts.push(entry);
    }

    fn remaining(&self) -> MethodSet {
        let methods: Vec<MethodKind> = if self.policy.require.is_empty() {
            self.policy.offered.clone()
        } else {
            self.policy
                .require
                .iter()
                .copied()
                .find(|m| !self.done.contains(m))
                .into_iter()
                .collect()
        };
        MethodSet::from(methods.as_slice())
    }

    fn reject(&self) -> Auth {
        Auth::Reject {
            proceed_with_methods: Some(self.remaining()),
            partial_success: false,
        }
    }

    /// The outcome of a correct credential: log in, or (multi-factor) ask
    /// for the next required method.
    fn succeed(&mut self, method: MethodKind) -> Auth {
        self.done.push(method);
        if self.policy.require.iter().all(|m| self.done.contains(m)) {
            return Auth::Accept;
        }
        Auth::Reject {
            proceed_with_methods: Some(self.remaining()),
            partial_success: true,
        }
    }
}

impl server::Handler for Handler {
    type Error = russh::Error;

    async fn authentication_banner(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.policy.banner.map(str::to_string))
    }

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, Self::Error> {
        self.note("none".into());
        Ok(if self.policy.accept_none {
            Auth::Accept
        } else {
            self.reject()
        })
    }

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        let ok = self.policy.password == Some(password);
        self.note(format!("password:{}", if ok { "ok" } else { "bad" }));
        Ok(if ok {
            self.succeed(MethodKind::Password)
        } else {
            self.reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        let ok = self.policy.keys.iter().any(|k| k == key.public_key);
        self.note(format!("publickey:{}", if ok { "ok" } else { "bad" }));
        Ok(if ok {
            self.succeed(MethodKind::PublicKey)
        } else {
            self.reject()
        })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        let Some(questions) = self.policy.questions.clone() else {
            self.note("keyboard-interactive:bad".into());
            return Ok(self.reject());
        };
        let ask = |rounds: usize| Auth::Partial {
            name: format!("round {rounds}").into(),
            instructions: "answer the questions".into(),
            prompts: questions
                .prompts
                .iter()
                .map(|(text, echo)| ((*text).into(), *echo))
                .collect::<Vec<_>>()
                .into(),
        };
        let Some(response) = response else {
            self.note("keyboard-interactive:start".into());
            self.kbd_rounds = 1;
            return Ok(ask(1));
        };
        let answers: Vec<String> = response
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect();
        self.observed
            .lock()
            .unwrap()
            .answers
            .extend(answers.iter().cloned());
        if answers == questions.expected {
            self.note("keyboard-interactive:ok".into());
            return Ok(self.succeed(MethodKind::KeyboardInteractive));
        }
        // Like PAM: a wrong answer is asked again, a few times.
        self.note("keyboard-interactive:bad".into());
        self.kbd_rounds += 1;
        if self.kbd_rounds <= 3 {
            return Ok(ask(self.kbd_rounds));
        }
        Ok(self.reject())
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pix_w: u32,
        _pix_h: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(channel, b"welcome\r\n".to_vec())?;
        Ok(())
    }
}

struct TestServer {
    port: u16,
    observed: Arc<Mutex<Observed>>,
    dir: tempfile::TempDir,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start(policy: Policy) -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    let config = Arc::new(server::Config {
        keys: vec![key(1)],
        methods: MethodSet::from(
            &[
                MethodKind::Password,
                MethodKind::PublicKey,
                MethodKind::KeyboardInteractive,
            ][..],
        ),
        // Rejections are delayed on purpose by default (brute-force
        // protection); the tests are not about that.
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..server::Config::default()
    });
    let socket = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let observed = Arc::new(Mutex::new(Observed::default()));
    let mut factory = Factory {
        policy,
        observed: observed.clone(),
    };
    let task = tokio::spawn(async move {
        let _ = factory.run_on_socket(config, &socket).await;
    });
    TestServer {
        port,
        observed,
        dir,
        task,
    }
}

impl TestServer {
    fn attempts(&self) -> Vec<String> {
        self.observed.lock().unwrap().attempts.clone()
    }

    fn answers(&self) -> Vec<String> {
        self.observed.lock().unwrap().answers.clone()
    }

    fn params(&self, chain: Vec<AuthChoice>, prompt: Arc<dyn AuthPrompt>) -> ConnectParams {
        self.params_with(chain, prompt, ConnectTimeouts::default())
    }

    fn params_with(
        &self,
        chain: Vec<AuthChoice>,
        prompt: Arc<dyn AuthPrompt>,
        timeouts: ConnectTimeouts,
    ) -> ConnectParams {
        ConnectParams {
            host: "127.0.0.1".into(),
            port: self.port,
            username: "tester".into(),
            auth: chain,
            term: "xterm-256color".into(),
            keepalive_seconds: 0,
            timeouts,
            known_hosts_path: self.dir.path().join("known_hosts"),
            extra_known_hosts: Vec::new(),
            host_key_prompt: Arc::new(AcceptAllHostKeys),
            auth_prompt: prompt,
            setup: Default::default(),
        }
    }
}

// ----------------------------------------------------------------- the client --

/// A scripted user: answers the questions in order; `None` cancels.
struct ScriptedUser {
    replies: Mutex<VecDeque<Option<Vec<&'static str>>>>,
    seen: Mutex<Vec<AuthPromptRequest>>,
    /// How long the "human" thinks before answering.
    think: Duration,
}

impl ScriptedUser {
    fn new(replies: Vec<Option<Vec<&'static str>>>) -> Arc<Self> {
        Self::slow(replies, Duration::ZERO)
    }

    fn slow(replies: Vec<Option<Vec<&'static str>>>, think: Duration) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
            think,
        })
    }

    fn seen(&self) -> Vec<AuthPromptRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl AuthPrompt for ScriptedUser {
    async fn ask(&self, request: AuthPromptRequest) -> AuthPromptResponse {
        self.seen.lock().unwrap().push(request);
        tokio::time::sleep(self.think).await;
        match self.replies.lock().unwrap().pop_front() {
            Some(Some(answers)) => AuthPromptResponse::Answers(
                answers
                    .into_iter()
                    .map(|a| SecretValue::new(a.to_string()))
                    .collect(),
            ),
            _ => AuthPromptResponse::Cancelled,
        }
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

/// Never answers.
struct SilentUser;

#[async_trait]
impl AuthPrompt for SilentUser {
    async fn ask(&self, _request: AuthPromptRequest) -> AuthPromptResponse {
        std::future::pending().await
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

fn kbd(password: Option<&str>) -> AuthChoice {
    AuthChoice::KeyboardInteractive {
        password: password.map(|p| SecretValue::new(p.to_string())),
    }
}

fn password(value: &str) -> AuthChoice {
    AuthChoice::Password {
        password: SecretValue::new(value.to_string()),
    }
}

fn key_file(path: &Path, identity: Option<KeyIdentity>) -> AuthChoice {
    AuthChoice::PrivateKey {
        key_path: path.to_path_buf(),
        passphrase: None,
        identity,
    }
}

struct Opened {
    result: Result<Session, SshError>,
    banners: Vec<String>,
    statuses: Vec<SessionStatus>,
    /// Banner texts seen before the first `Ready` / `Failed`.
    banners_before_end: usize,
}

async fn open(params: ConnectParams) -> Opened {
    let (tx, mut rx) = mpsc::channel(64);
    let result = Session::open(params, SessionOptions::default(), tx).await;
    let mut banners = Vec::new();
    let mut statuses = Vec::new();
    let mut banners_before_end = 0;
    while let Ok(event) = rx.try_recv() {
        match event {
            SessionEvent::Banner(text) => banners.push(text),
            SessionEvent::Status(status) => {
                if matches!(status, SessionStatus::Ready | SessionStatus::Failed { .. })
                    && banners_before_end == 0
                {
                    banners_before_end = banners.len();
                }
                statuses.push(status);
            }
            _ => {}
        }
    }
    Opened {
        result,
        banners,
        statuses,
        banners_before_end,
    }
}

fn single(method: MethodKind) -> Vec<MethodKind> {
    vec![method]
}

// ---------------------------------------------------------------------- tests --

/// Some servers accept `none`: no method may be tried then (a password sent
/// to a server that did not ask for one is a credential given away).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_accepts_none_gets_no_credentials() {
    let server = start(Policy {
        accept_none: true,
        ..Policy::default()
    })
    .await;
    let opened = open(server.params(vec![password("never-sent")], Arc::new(NoAuthPrompts))).await;
    opened
        .result
        .expect("none must be enough")
        .close()
        .await
        .unwrap();
    assert_eq!(server.attempts(), ["none"]);
}

/// The `none` probe tells the client what the server allows; methods outside
/// that list are not attempted at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn methods_the_server_does_not_list_are_never_attempted() {
    let client_key = key(10);
    let dir = tempfile::tempdir().unwrap();
    let path = write_key(dir.path(), "id", &client_key);
    let server = start(Policy {
        offered: single(MethodKind::PublicKey),
        keys: vec![client_key.public_key().clone()],
        password: Some("irrelevant"),
        ..Policy::default()
    })
    .await;

    let chain = vec![password("pw"), kbd(Some("pw")), key_file(&path, None)];
    let opened = open(server.params(chain, Arc::new(NoAuthPrompts))).await;
    opened
        .result
        .expect("the key is the way in")
        .close()
        .await
        .unwrap();
    assert_eq!(server.attempts(), ["none", "publickey:ok"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keyboard_interactive_is_answered_with_the_stored_password() {
    let server = start(Policy {
        offered: single(MethodKind::KeyboardInteractive),
        questions: Some(Questions {
            prompts: vec![("Password: ", false)],
            expected: vec!["stored-pw"],
        }),
        ..Policy::default()
    })
    .await;
    let user = ScriptedUser::new(vec![]);
    // The way the desktop sets up a password host: password first, then
    // keyboard-interactive with the same password.
    let chain = vec![password("stored-pw"), kbd(Some("stored-pw"))];
    let opened = open(server.params(chain, user.clone())).await;
    opened.result.expect("logged in").close().await.unwrap();

    assert!(
        user.seen().is_empty(),
        "nobody was asked: {:?}",
        user.seen()
    );
    assert_eq!(
        server.attempts(),
        [
            "none",
            "keyboard-interactive:start",
            "keyboard-interactive:ok"
        ],
        "the password method is skipped, the question answered silently"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_questions_go_to_the_user() {
    let server = start(Policy {
        offered: single(MethodKind::KeyboardInteractive),
        questions: Some(Questions {
            prompts: vec![("Verification code: ", true), ("PIN: ", false)],
            expected: vec!["123456", "4321"],
        }),
        ..Policy::default()
    })
    .await;
    let user = ScriptedUser::new(vec![Some(vec!["123456", "4321"])]);
    let opened = open(server.params(vec![kbd(Some("stored-pw"))], user.clone())).await;
    opened.result.expect("logged in").close().await.unwrap();

    let seen = user.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].kind, AuthPromptKind::KeyboardInteractive);
    assert_eq!(seen[0].name, "round 1");
    assert_eq!(seen[0].instructions, "answer the questions");
    assert_eq!(seen[0].prompts.len(), 2);
    assert_eq!(seen[0].prompts[0].text, "Verification code: ");
    assert!(seen[0].prompts[0].echo);
    assert!(!seen[0].prompts[1].echo, "a PIN is hidden");
}

/// The stored password is for the account password only: a one-time-code
/// question that happens to contain the word "password" must reach the user.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_one_time_password_question_never_receives_the_stored_password() {
    let server = start(Policy {
        offered: single(MethodKind::KeyboardInteractive),
        questions: Some(Questions {
            prompts: vec![("One-time password: ", false)],
            expected: vec!["654321"],
        }),
        ..Policy::default()
    })
    .await;
    let user = ScriptedUser::new(vec![Some(vec!["654321"])]);
    let opened = open(server.params(vec![kbd(Some("stored-pw"))], user.clone())).await;
    opened.result.expect("logged in").close().await.unwrap();
    assert_eq!(user.seen().len(), 1);
    assert!(
        !server.answers().contains(&"stored-pw".to_string()),
        "the stored password leaked to the server: {:?}",
        server.answers()
    );
}

/// A wrong stored password is sent once. When the server asks again the user
/// is asked: repeating the stored one would only burn the server's lock-out
/// counter.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_stored_password_is_not_repeated() {
    let server = start(Policy {
        offered: single(MethodKind::KeyboardInteractive),
        questions: Some(Questions {
            prompts: vec![("Password: ", false)],
            expected: vec!["right-pw"],
        }),
        ..Policy::default()
    })
    .await;
    let user = ScriptedUser::new(vec![Some(vec!["right-pw"])]);
    let opened = open(server.params(vec![kbd(Some("stale-pw"))], user.clone())).await;
    opened.result.expect("logged in").close().await.unwrap();

    assert_eq!(user.seen().len(), 1, "the second round is the user's");
    assert_eq!(server.answers(), ["stale-pw", "right-pw"]);
}

/// The password method was turned down: the same password must not go to the
/// server a second time through keyboard-interactive (two failed logins for
/// one wrong password is how accounts get locked). The server's question is
/// the user's to answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_password_is_not_sent_again_through_keyboard_interactive() {
    let server = start(Policy {
        offered: vec![MethodKind::Password, MethodKind::KeyboardInteractive],
        password: Some("right-pw"),
        questions: Some(Questions {
            prompts: vec![("Password: ", false)],
            expected: vec!["right-pw"],
        }),
        ..Policy::default()
    })
    .await;
    let user = ScriptedUser::new(vec![Some(vec!["right-pw"])]);
    let chain = vec![password("wrong-pw"), kbd(Some("wrong-pw"))];
    let opened = open(server.params(chain, user.clone())).await;
    opened
        .result
        .expect("the user typed the right one")
        .close()
        .await
        .unwrap();

    assert_eq!(
        user.seen().len(),
        1,
        "the user is asked, the stored one is not reused"
    );
    assert_eq!(
        server.answers(),
        ["right-pw"],
        "the wrong password was sent once, as the password method"
    );
    assert_eq!(server.attempts()[1], "password:bad");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_question_ends_the_whole_authentication() {
    let server = start(Policy {
        offered: vec![MethodKind::KeyboardInteractive, MethodKind::Password],
        password: Some("pw"),
        questions: Some(Questions {
            prompts: vec![("Code: ", true)],
            expected: vec!["1"],
        }),
        ..Policy::default()
    })
    .await;
    let user = ScriptedUser::new(vec![None]);
    let chain = vec![kbd(None), password("pw")];
    let opened = open(server.params(chain, user)).await;
    let error = opened.result.expect_err("cancelled");
    assert!(matches!(error, SshError::AuthCancelled), "{error:?}");
    assert!(error.is_auth_failure() && !error.is_retriable());
    assert!(
        !server.attempts().iter().any(|a| a.starts_with("password")),
        "the user said no: nothing else may be tried: {:?}",
        server.attempts()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unanswered_question_times_out() {
    let server = start(Policy {
        offered: single(MethodKind::KeyboardInteractive),
        questions: Some(Questions {
            prompts: vec![("Code: ", true)],
            expected: vec!["1"],
        }),
        ..Policy::default()
    })
    .await;
    let timeouts = ConnectTimeouts {
        auth_prompt: Duration::from_millis(300),
        ..ConnectTimeouts::default()
    };
    let opened = open(server.params_with(vec![kbd(None)], Arc::new(SilentUser), timeouts)).await;
    let error = opened.result.expect_err("timed out");
    assert!(matches!(error, SshError::AuthPromptTimeout), "{error:?}");
    assert!(!error.is_retriable());
}

/// The time a person needs to type is not the network's time: the
/// authentication deadline (500 ms here) stands still while the question is
/// open (the user takes 1.5 s).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_time_a_human_needs_does_not_count_against_the_deadline() {
    let server = start(Policy {
        offered: single(MethodKind::KeyboardInteractive),
        questions: Some(Questions {
            prompts: vec![("Code: ", true)],
            expected: vec!["1"],
        }),
        ..Policy::default()
    })
    .await;
    let user = ScriptedUser::slow(vec![Some(vec!["1"])], Duration::from_millis(1500));
    let timeouts = ConnectTimeouts {
        auth: Duration::from_millis(500),
        ..ConnectTimeouts::default()
    };
    let opened = open(server.params_with(vec![kbd(None)], user, timeouts)).await;
    opened
        .result
        .expect("a slow human is fine")
        .close()
        .await
        .unwrap();
}

/// The deadline still guards the network part: a server that never answers
/// is cut off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_authentication_deadline_still_applies_without_a_question() {
    // A listener that accepts and says nothing: the handshake itself never
    // completes, so the connect budget is what ends it.
    let silent = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = silent.local_addr().unwrap().port();
    let _hold = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = silent.accept().await {
            held.push(stream);
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let params = ConnectParams {
        host: "127.0.0.1".into(),
        port,
        username: "tester".into(),
        auth: vec![password("x")],
        term: "xterm".into(),
        keepalive_seconds: 0,
        timeouts: ConnectTimeouts {
            connect: Duration::from_millis(400),
            ..ConnectTimeouts::default()
        },
        known_hosts_path: dir.path().join("known_hosts"),
        extra_known_hosts: Vec::new(),
        host_key_prompt: Arc::new(AcceptAllHostKeys),
        auth_prompt: Arc::new(NoAuthPrompts),
        setup: Default::default(),
    };
    let (tx, _rx) = mpsc::channel(8);
    let error = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect_err("must time out");
    assert!(
        matches!(error, SshError::ConnectTimeout { .. }),
        "{error:?}"
    );
}

/// Two factors: the key is accepted but the server wants the keyboard-
/// interactive step as well.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publickey_then_keyboard_interactive_completes_a_two_factor_login() {
    let client_key = key(20);
    let dir = tempfile::tempdir().unwrap();
    let path = write_key(dir.path(), "id", &client_key);
    let server = start(Policy {
        require: vec![MethodKind::PublicKey, MethodKind::KeyboardInteractive],
        keys: vec![client_key.public_key().clone()],
        questions: Some(Questions {
            prompts: vec![("Password: ", false)],
            expected: vec!["second-factor"],
        }),
        ..Policy::default()
    })
    .await;
    let chain = vec![key_file(&path, None), kbd(Some("second-factor"))];
    let opened = open(server.params(chain, Arc::new(NoAuthPrompts))).await;
    opened.result.expect("two factors").close().await.unwrap();
    assert_eq!(
        server.attempts(),
        [
            "none",
            "publickey:ok",
            "keyboard-interactive:start",
            "keyboard-interactive:ok"
        ]
    );
}

/// The order in the configuration does not have to match the server's: the
/// keyboard-interactive step is not allowed at first (so it is skipped), but
/// it becomes allowed after the key — and must be tried then.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_method_skipped_early_is_revisited_after_a_partial_success() {
    let client_key = key(21);
    let dir = tempfile::tempdir().unwrap();
    let path = write_key(dir.path(), "id", &client_key);
    let server = start(Policy {
        require: vec![MethodKind::PublicKey, MethodKind::KeyboardInteractive],
        keys: vec![client_key.public_key().clone()],
        questions: Some(Questions {
            prompts: vec![("Password: ", false)],
            expected: vec!["pw"],
        }),
        ..Policy::default()
    })
    .await;
    let chain = vec![kbd(Some("pw")), key_file(&path, None)];
    let opened = open(server.params(chain, Arc::new(NoAuthPrompts))).await;
    opened.result.expect("two factors").close().await.unwrap();
    assert_eq!(
        server.attempts(),
        [
            "none",
            "publickey:ok",
            "keyboard-interactive:start",
            "keyboard-interactive:ok"
        ]
    );
}

/// When the first factor is all the connection can provide, the error says
/// the server wanted more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_factor_the_connection_cannot_provide_is_explained() {
    let client_key = key(22);
    let dir = tempfile::tempdir().unwrap();
    let path = write_key(dir.path(), "id", &client_key);
    let server = start(Policy {
        require: vec![MethodKind::PublicKey, MethodKind::KeyboardInteractive],
        keys: vec![client_key.public_key().clone()],
        ..Policy::default()
    })
    .await;
    let opened = open(server.params(vec![key_file(&path, None)], Arc::new(NoAuthPrompts))).await;
    let message = opened.result.expect_err("incomplete").to_string();
    assert!(
        message.contains("requires additional authentication"),
        "{message}"
    );
    assert!(message.contains("keyboard-interactive"), "{message}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_banner_arrives_sanitized_and_before_ready() {
    let server = start(Policy {
        accept_none: true,
        banner: Some("Authorized users only\x1b[2J\x1b]0;x\x07\r\nSecond line"),
        ..Policy::default()
    })
    .await;
    let opened = open(server.params(vec![password("x")], Arc::new(NoAuthPrompts))).await;
    let banner = opened.banners.concat();
    opened.result.expect("none accepted").close().await.unwrap();
    assert!(banner.contains("Authorized users only"), "{banner:?}");
    assert!(banner.contains("Second line"), "{banner:?}");
    assert!(
        !banner.chars().any(|c| c.is_control() && c != '\n'),
        "{banner:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_banner_precedes_the_failure_when_the_login_is_refused() {
    let server = start(Policy {
        offered: single(MethodKind::Password),
        password: Some("right"),
        banner: Some("Access is logged."),
        ..Policy::default()
    })
    .await;
    let opened = open(server.params(vec![password("wrong")], Arc::new(NoAuthPrompts))).await;
    assert!(opened.result.is_err());
    assert_eq!(opened.banners, ["Access is logged."]);
    assert_eq!(opened.banners_before_end, 1, "{:?}", opened.statuses);
}

// -------------------------------------------------------------- passphrases --

fn identity() -> KeyIdentity {
    KeyIdentity {
        id: "identity-1".into(),
        name: "work key".into(),
    }
}

async fn encrypted_setup() -> (TestServer, tempfile::TempDir, PathBuf) {
    let client_key = key(30);
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted(dir.path(), "id_locked", &client_key, "s3cret");
    let server = start(Policy {
        offered: single(MethodKind::PublicKey),
        keys: vec![client_key.public_key().clone()],
        ..Policy::default()
    })
    .await;
    (server, dir, path)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_passphrase_is_asked_for_by_identity_name() {
    let (server, _dir, path) = encrypted_setup().await;
    let user = ScriptedUser::new(vec![Some(vec!["s3cret"])]);
    let opened = open(server.params(vec![key_file(&path, Some(identity()))], user.clone())).await;
    opened.result.expect("unlocked").close().await.unwrap();

    let seen = user.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].kind,
        AuthPromptKind::Passphrase {
            identity: Some(identity()),
            retry: false
        }
    );
    assert_eq!(seen[0].prompts.len(), 1);
    assert_eq!(seen[0].prompts[0].text, "Passphrase for work key");
    assert!(!seen[0].prompts[0].echo);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_stored_passphrase_falls_back_to_a_question_marked_as_retry() {
    let (server, _dir, path) = encrypted_setup().await;
    let user = ScriptedUser::new(vec![Some(vec!["s3cret"])]);
    let chain = vec![AuthChoice::PrivateKey {
        key_path: path,
        passphrase: Some(SecretValue::new("stale".to_string())),
        identity: Some(identity()),
    }];
    let opened = open(server.params(chain, user.clone())).await;
    opened.result.expect("unlocked").close().await.unwrap();
    let seen = user.seen();
    assert_eq!(seen.len(), 1);
    assert!(matches!(
        seen[0].kind,
        AuthPromptKind::Passphrase { retry: true, .. }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_passphrase_is_asked_three_times_at_most() {
    let (server, _dir, path) = encrypted_setup().await;
    let user = ScriptedUser::new(vec![
        Some(vec!["a"]),
        Some(vec!["b"]),
        Some(vec!["c"]),
        Some(vec!["s3cret"]),
    ]);
    let opened = open(server.params(vec![key_file(&path, None)], user.clone())).await;
    let error = opened.result.expect_err("three wrong answers");
    assert!(
        matches!(error, SshError::WrongPassphrase { .. }),
        "{error:?}"
    );
    assert_eq!(user.seen().len(), 3);
    // Without an identity the prompt names the key file.
    assert_eq!(user.seen()[0].prompts[0].text, "Passphrase for id_locked");
    assert!(!error.to_string().contains("s3cret"));
}

/// Without a person to ask the passphrase is simply missing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_an_interactive_prompt_a_missing_passphrase_is_an_error() {
    let (server, _dir, path) = encrypted_setup().await;
    let opened = open(server.params(vec![key_file(&path, None)], Arc::new(NoAuthPrompts))).await;
    let error = opened.result.expect_err("nobody to ask");
    assert!(matches!(error, SshError::PassphraseRequired), "{error:?}");
}
