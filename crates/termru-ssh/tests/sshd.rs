//! Integration tests against a **real OpenSSH server**.
//!
//! Harness: a private `sshd` instance per test, running as the current
//! unprivileged user on a high port with its own host key, authorized_keys,
//! and known_hosts inside a temporary directory. No root required.
//!
//! Prerequisites: OpenSSH tooling (`sshd`, `ssh-keygen`; `ssh-agent`/
//! `ssh-add` for the agent test). When a binary is missing the affected test
//! prints a loud `SKIPPED:` line and returns without asserting — it never
//! silently passes a scenario it failed to set up.
//!
//! Covered (TODO Phase 4): shell success, wrong password, wrong key,
//! passphrase-protected key (wrong + correct), host-key TOFU write, changed
//! host-key detection (decline + accept + subsequently known), agent auth,
//! PTY resize, large output flood, re-attach replay ordering, interrupted
//! connection classification, binary output preservation, clean exit
//! classification, bounded automatic reconnect give-up.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "harness setup failures must panic loudly in tests"
)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use termru_ssh::{
    AuthChoice, ConnectParams, ConnectTimeouts, ErrorCategory, HostKeyPrompt, HostKeyPromptRequest,
    ReconnectPolicy, SecretValue, Session, SessionEvent, SessionOptions, SessionStatus, SshError,
};

const ENC_PASSPHRASE: &str = "kite-pass-test";
const WRONG_PASSPHRASE: &str = "definitely-wrong-passphrase";
const WRONG_PASSWORD: &str = "definitely-not-the-password";
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);
const FLOOD_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------- tooling --

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

fn find_sshd() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("TERMRU_SSHD_PATH") {
        return Some(PathBuf::from(explicit));
    }
    // sshd re-executes itself from argv[0] and refuses relative paths.
    for candidate in [
        "/usr/sbin/sshd",
        "/usr/bin/sshd",
        "/usr/local/sbin/sshd",
        "/opt/homebrew/sbin/sshd",
    ] {
        if Path::new(candidate).is_file() {
            return Some(PathBuf::from(candidate));
        }
    }
    which("sshd").filter(|p| p.is_absolute())
}

fn current_user() -> String {
    if let Ok(output) = Command::new("id").arg("-un").output() {
        if output.status.success() {
            let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !user.is_empty() {
                return user;
            }
        }
    }
    std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
}

fn pub_of(key: &Path) -> PathBuf {
    let mut name = key.as_os_str().to_os_string();
    name.push(".pub");
    PathBuf::from(name)
}

fn keygen(path: &Path, passphrase: &str) {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", passphrase, "-f"])
        .arg(path)
        .status()
        .expect("ssh-keygen must be available for integration tests");
    assert!(status.success(), "ssh-keygen failed for {}", path.display());
}

/// First two fields of a `.pub` line: `<key-type> <base64>`.
fn pubkey_line(path: &Path) -> String {
    let text = std::fs::read_to_string(path).expect("read public key");
    let mut parts = text
        .lines()
        .next()
        .expect("public key line")
        .split_whitespace();
    let key_type = parts.next().expect("key type");
    let key = parts.next().expect("key data");
    format!("{key_type} {key}")
}

/// `ssh-keygen -lf` fingerprint — the reference Termru claims equality with.
fn fingerprint_of(pub_path: &Path) -> String {
    let out = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(pub_path)
        .output()
        .expect("ssh-keygen -lf");
    assert!(out.status.success(), "ssh-keygen -lf failed");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .nth(1)
        .expect("fingerprint field")
        .to_string()
}

fn collect_descendants(pid: u32, out: &mut Vec<u32>) {
    let Ok(output) = Command::new("ps")
        .args(["-o", "pid=", "--ppid", &pid.to_string()])
        .output()
    else {
        return;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Ok(child) = line.trim().parse::<u32>() {
            out.push(child);
            collect_descendants(child, out);
        }
    }
}

fn kill_pid(pid: u32) {
    // `kill` is a POSIX shell builtin: no dependency on procps' kill binary.
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!("kill -9 {pid}"))
        .status();
}

// ------------------------------------------------------------- test server --

struct TestServer {
    dir: PathBuf,
    port: u16,
    child: Child,
    killed: bool,
    host_key: PathBuf,
    client_key: PathBuf,
    encrypted_key: PathBuf,
    bad_key: PathBuf,
    decoy_key: PathBuf,
    known_hosts: PathBuf,
}

impl TestServer {
    fn start() -> Option<Self> {
        let Some(sshd) = find_sshd() else {
            eprintln!(
                "SKIPPED: sshd not found (install openssh-server) — integration test not exercised"
            );
            return None;
        };
        if which("ssh-keygen").is_none() {
            eprintln!("SKIPPED: ssh-keygen not found — integration test not exercised");
            return None;
        }

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "termru-ssh-it-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let host_key = dir.join("host_ed25519");
        let client_key = dir.join("id_client");
        let encrypted_key = dir.join("id_encrypted");
        let bad_key = dir.join("id_unauthorized");
        let decoy_key = dir.join("id_decoy");
        keygen(&host_key, "");
        keygen(&client_key, "");
        keygen(&encrypted_key, ENC_PASSPHRASE);
        keygen(&bad_key, "");
        keygen(&decoy_key, "");

        let authorized_keys = dir.join("authorized_keys");
        std::fs::copy(pub_of(&client_key), &authorized_keys).expect("authorized_keys");
        // The encrypted key must be authorized too (only its private half is
        // passphrase-protected), otherwise the correct-passphrase test would
        // be rejected by the server with an unrelated AuthFailed error.
        let mut ak = std::fs::read(&authorized_keys).expect("read authorized_keys");
        if !ak.ends_with(b"\n") {
            ak.push(b'\n');
        }
        ak.extend(std::fs::read(pub_of(&encrypted_key)).expect("encrypted pub"));
        std::fs::write(&authorized_keys, &ak).expect("append encrypted pub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&authorized_keys, std::fs::Permissions::from_mode(0o600))
                .expect("chmod authorized_keys");
        }

        let port = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind :0");
            listener.local_addr().expect("local addr").port()
        };

        let config = dir.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "Port {port}\n\
                 ListenAddress 127.0.0.1\n\
                 HostKey {host_key}\n\
                 PidFile {pid_file}\n\
                 StrictModes no\n\
                 AuthorizedKeysFile {authorized_keys}\n\
                 PasswordAuthentication yes\n\
                 PubkeyAuthentication yes\n\
                 KbdInteractiveAuthentication no\n\
                 PermitRootLogin no\n\
                 UsePAM no\n\
                 PermitEmptyPasswords no\n\
                 PrintMotd no\n\
                 LogLevel INFO\n",
                host_key = host_key.display(),
                pid_file = dir.join("sshd.pid").display(),
                authorized_keys = authorized_keys.display(),
            ),
        )
        .expect("write sshd_config");

        let log_path = dir.join("sshd.log");
        let log = std::fs::File::create(&log_path).expect("sshd log");
        let log_err = log.try_clone().expect("clone log handle");
        let child = Command::new(&sshd)
            .arg("-f")
            .arg(&config)
            .args(["-D", "-e"])
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {sshd:?}: {e}"));

        let known_hosts = dir.join("known_hosts");
        let mut server = Self {
            dir,
            port,
            child,
            killed: false,
            host_key,
            client_key,
            encrypted_key,
            bad_key,
            decoy_key,
            known_hosts,
        };
        server.wait_ready(&log_path);
        Some(server)
    }

    fn wait_ready(&mut self, log_path: &Path) {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        for _ in 0..200 {
            if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
                return;
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                let log = std::fs::read_to_string(log_path).unwrap_or_default();
                panic!("sshd exited early ({status}); log:\n{log}");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let log = std::fs::read_to_string(log_path).unwrap_or_default();
        panic!("sshd never became ready; log:\n{log}");
    }

    /// Kills the listener and every server-side process of live connections.
    fn interrupt(&mut self) {
        let mut pids = Vec::new();
        collect_descendants(self.child.id(), &mut pids);
        pids.push(self.child.id());
        for pid in pids.into_iter().rev() {
            kill_pid(pid);
        }
        self.killed = true;
    }

    fn client_auth(&self) -> AuthChoice {
        AuthChoice::PrivateKey {
            key_path: self.client_key.clone(),
            passphrase: None,
        }
    }

    fn params(&self, prompt: Arc<dyn HostKeyPrompt>, auth: AuthChoice) -> ConnectParams {
        ConnectParams {
            host: "127.0.0.1".into(),
            port: self.port,
            username: current_user(),
            auth,
            term: "xterm-256color".into(),
            keepalive_seconds: 0,
            timeouts: ConnectTimeouts {
                connect: Duration::from_secs(5),
                auth: Duration::from_secs(5),
                host_key_decision: Duration::from_secs(10),
            },
            known_hosts_path: self.known_hosts.clone(),
            host_key_prompt: prompt,
        }
    }

    fn seed_known_hosts(&self, key_line: &str) {
        std::fs::write(
            &self.known_hosts,
            format!("[127.0.0.1]:{} {key_line}\n", self.port),
        )
        .expect("seed known_hosts");
    }

    fn server_host_key_line(&self) -> String {
        let out = Command::new("ssh-keygen")
            .args(["-y", "-f"])
            .arg(&self.host_key)
            .output()
            .expect("ssh-keygen -y");
        assert!(out.status.success(), "ssh-keygen -y failed");
        // Recent ssh-keygen appends the key comment; known_hosts holds only
        // the `<algorithm> <base64>` fields.
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if !self.killed {
            let mut pids = Vec::new();
            collect_descendants(self.child.id(), &mut pids);
            pids.push(self.child.id());
            for pid in pids.into_iter().rev() {
                kill_pid(pid);
            }
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ----------------------------------------------------------------- prompts --

struct RecordingPrompt {
    accept: bool,
    seen: Arc<Mutex<Vec<HostKeyPromptRequest>>>,
}

impl RecordingPrompt {
    fn new(accept: bool) -> Arc<Self> {
        Arc::new(Self {
            accept,
            seen: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn seen(&self) -> Vec<HostKeyPromptRequest> {
        self.seen.lock().expect("prompt log poisoned").clone()
    }
}

#[async_trait::async_trait]
impl HostKeyPrompt for RecordingPrompt {
    async fn decide(&self, request: HostKeyPromptRequest) -> bool {
        self.seen.lock().expect("prompt log poisoned").push(request);
        self.accept
    }
}

// ---------------------------------------------------------------- collector --

#[derive(Default)]
struct Collector {
    data: Vec<u8>,
    statuses: Vec<SessionStatus>,
    exits: Vec<Option<i32>>,
}

impl Collector {
    fn push(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Data(bytes) => self.data.extend_from_slice(&bytes),
            SessionEvent::Status(status) => self.statuses.push(status),
            SessionEvent::Exit { code } => self.exits.push(code),
        }
    }

    fn drain(&mut self, rx: &mut mpsc::Receiver<SessionEvent>) {
        while let Ok(event) = rx.try_recv() {
            self.push(event);
        }
    }

    fn data_tail(&self) -> String {
        let tail = &self.data[self.data.len().saturating_sub(400)..];
        String::from_utf8_lossy(tail).into_owned()
    }

    async fn until_status(
        &mut self,
        rx: &mut mpsc::Receiver<SessionEvent>,
        what: &str,
        pred: impl Fn(&SessionStatus) -> bool,
    ) -> SessionStatus {
        let result = tokio::time::timeout(EVENT_TIMEOUT, async {
            loop {
                if let Some(status) = self.statuses.iter().find(|s| pred(s)) {
                    return status.clone();
                }
                let event = rx.recv().await.unwrap_or_else(|| {
                    panic!(
                        "event channel closed waiting for `{what}`; saw {:?}",
                        self.statuses
                    )
                });
                self.push(event);
            }
        })
        .await;
        result.unwrap_or_else(|_| {
            panic!(
                "timed out waiting for status `{what}`; saw {:?}",
                self.statuses
            )
        })
    }

    async fn until_data(
        &mut self,
        rx: &mut mpsc::Receiver<SessionEvent>,
        what: &str,
        pred: impl Fn(&[u8]) -> bool,
    ) {
        let result = tokio::time::timeout(EVENT_TIMEOUT, async {
            loop {
                if pred(&self.data) {
                    return;
                }
                let event = rx.recv().await.unwrap_or_else(|| {
                    panic!(
                        "event channel closed waiting for `{what}`; saw {:?}",
                        self.statuses
                    )
                });
                self.push(event);
            }
        })
        .await;
        result.unwrap_or_else(|_| {
            panic!(
                "timed out waiting for data `{what}`; {} bytes, tail {:?}, statuses {:?}",
                self.data.len(),
                self.data_tail(),
                self.statuses
            )
        })
    }

    async fn until_exit(&mut self, rx: &mut mpsc::Receiver<SessionEvent>) -> Option<i32> {
        let result = tokio::time::timeout(EVENT_TIMEOUT, async {
            loop {
                if let Some(code) = self.exits.first().copied() {
                    return code;
                }
                let event = rx.recv().await.unwrap_or_else(|| {
                    panic!(
                        "event channel closed waiting for exit; saw {:?}",
                        self.statuses
                    )
                });
                self.push(event);
            }
        })
        .await;
        result
            .unwrap_or_else(|_| panic!("timed out waiting for exit; statuses {:?}", self.statuses))
    }

    fn assert_terminal_failure(&self, category: ErrorCategory) {
        assert!(
            self.statuses
                .iter()
                .any(|s| matches!(s, SessionStatus::Failed { category: c, .. } if *c == category)),
            "expected Failed{{{category:?}}}, saw {:?}",
            self.statuses
        );
        assert!(
            self.statuses
                .iter()
                .any(|s| matches!(s, SessionStatus::Closed { .. })),
            "expected Closed, saw {:?}",
            self.statuses
        );
    }
}

fn count(data: &[u8], needle: &[u8]) -> usize {
    data.windows(needle.len()).filter(|w| *w == needle).count()
}

// ------------------------------------------------------------------- tests --

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_roundtrip_and_tofu_trust() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt.clone(), server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    let seen = prompt.seen();
    assert_eq!(seen.len(), 1, "exactly one trust prompt: {seen:?}");
    match &seen[0] {
        HostKeyPromptRequest::Unknown { host, port, key } => {
            assert_eq!(host, "127.0.0.1");
            assert_eq!(*port, server.port);
            assert!(
                key.fingerprint.starts_with("SHA256:"),
                "fingerprint format: {}",
                key.fingerprint
            );
            assert!(!key.key_type.is_empty());
        }
        other => panic!("expected Unknown prompt, got {other:?}"),
    }

    let kh = std::fs::read_to_string(&server.known_hosts).unwrap_or_default();
    assert!(kh.contains("|1|"), "TOFU writes hashed: {kh:?}");
    assert!(
        !kh.contains(&format!("[127.0.0.1]:{}", server.port)),
        "plaintext host must not appear: {kh:?}"
    );
    assert!(
        kh.contains(&server.server_host_key_line()),
        "entry holds the real key: {kh:?}"
    );

    session
        .write(b"echo TERM_OK\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "echo roundtrip", |d| count(d, b"TERM_OK") >= 2)
        .await;
    session.close().await.expect("close");

    // Behavioral proof that the persisted entry matches the server: a
    // refusing prompt must never be asked and the connection must succeed.
    let refusing = RecordingPrompt::new(false);
    let (tx, mut rx) = mpsc::channel(256);
    let params = server.params(refusing.clone(), server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("known host must connect without prompting");
    let mut col = Collector::default();
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;
    assert!(refusing.seen().is_empty(), "no prompt for a known key");
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_host_key_declined_is_hostkey_failure() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(false);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt.clone(), server.client_auth());
    let error = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect_err("declined trust must refuse the connection");

    assert!(matches!(error, SshError::HostKeyRejected), "got {error:?}");
    assert_eq!(ErrorCategory::of(&error), ErrorCategory::HostKey);
    assert!(!error.is_retriable());
    assert_eq!(prompt.seen().len(), 1);
    assert!(
        !server.known_hosts.exists(),
        "refused key must never be persisted"
    );

    col.drain(&mut rx);
    col.assert_terminal_failure(ErrorCategory::HostKey);
    assert!(
        !col.statuses
            .iter()
            .any(|s| matches!(s, SessionStatus::Reconnecting { .. })),
        "host-key failures must not auto-reconnect: {:?}",
        col.statuses
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_host_key_declined_reports_both_fingerprints() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let decoy_line = pubkey_line(&pub_of(&server.decoy_key));
    let decoy_fingerprint = fingerprint_of(&pub_of(&server.decoy_key));
    server.seed_known_hosts(&decoy_line);

    let prompt = RecordingPrompt::new(false);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt.clone(), server.client_auth());
    let error = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect_err("changed key must be refused when the user declines");

    let (old_fp, new_fp) = match &error {
        SshError::HostKeyChanged {
            old_fingerprint,
            new_fingerprint,
            ..
        } => (old_fingerprint.clone(), new_fingerprint.clone()),
        other => panic!("expected HostKeyChanged, got {other:?}"),
    };
    assert_eq!(
        old_fp, decoy_fingerprint,
        "old fingerprint is the stored key"
    );
    assert!(new_fp.starts_with("SHA256:"), "new fingerprint: {new_fp}");
    assert_ne!(old_fp, new_fp);
    assert_eq!(ErrorCategory::of(&error), ErrorCategory::HostKey);
    assert!(!error.is_retriable());

    let seen = prompt.seen();
    assert_eq!(seen.len(), 1);
    assert!(
        matches!(seen[0], HostKeyPromptRequest::Changed { .. }),
        "expected Changed prompt: {seen:?}"
    );

    let kh = std::fs::read_to_string(&server.known_hosts).expect("known_hosts");
    assert!(
        kh.contains(&decoy_line),
        "declined change must not replace the stored key: {kh:?}"
    );
    assert!(
        !kh.contains(&server.server_host_key_line()),
        "presented key must not be trusted after decline"
    );

    col.drain(&mut rx);
    col.assert_terminal_failure(ErrorCategory::HostKey);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_host_key_accepted_replaces_entry_and_is_subsequently_known() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let decoy_line = pubkey_line(&pub_of(&server.decoy_key));
    server.seed_known_hosts(&decoy_line);

    // Accept the change: connection succeeds and the entry is replaced.
    let accepting = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let params = server.params(accepting.clone(), server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("accepting changed key must connect");
    {
        let mut col = Collector::default();
        col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
            .await;
    }
    assert!(matches!(
        accepting.seen().as_slice(),
        [HostKeyPromptRequest::Changed { .. }]
    ));
    session.close().await.expect("close");

    let kh = std::fs::read_to_string(&server.known_hosts).expect("known_hosts");
    assert!(!kh.contains(&decoy_line), "decoy entry replaced: {kh:?}");
    assert!(
        kh.contains(&server.server_host_key_line()),
        "real host key persisted: {kh:?}"
    );

    // Now known: a refusing prompt is never asked and the connection succeeds.
    let refusing = RecordingPrompt::new(false);
    let (tx, mut rx) = mpsc::channel(256);
    let params = server.params(refusing.clone(), server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("known host must connect without prompting");
    let mut col = Collector::default();
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;
    assert!(refusing.seen().is_empty(), "no prompt for a known key");
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_password_is_auth_failure_without_leaking_the_password() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let auth = AuthChoice::Password {
        password: SecretValue::new(WRONG_PASSWORD.to_string()),
    };
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, auth);
    let error = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect_err("wrong password must fail");

    match &error {
        SshError::AuthFailed { method, .. } => assert_eq!(*method, "password"),
        other => panic!("expected AuthFailed, got {other:?}"),
    }
    assert!(error.is_auth_failure());
    assert!(!error.is_retriable());
    assert_eq!(ErrorCategory::of(&error), ErrorCategory::Auth);
    let message = error.to_string();
    assert!(
        !message.contains(WRONG_PASSWORD),
        "password leaked into error: {message}"
    );

    col.drain(&mut rx);
    col.assert_terminal_failure(ErrorCategory::Auth);
    assert!(
        !col.statuses
            .iter()
            .any(|s| matches!(s, SessionStatus::Reconnecting { .. })),
        "auth failures must not auto-reconnect: {:?}",
        col.statuses
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthorized_key_is_auth_failure() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let auth = AuthChoice::PrivateKey {
        key_path: server.bad_key.clone(),
        passphrase: None,
    };
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, auth);
    let error = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect_err("unauthorized key must fail");

    assert!(
        matches!(
            &error,
            SshError::AuthFailed {
                method: "publickey",
                ..
            }
        ),
        "got {error:?}"
    );
    assert!(error.is_auth_failure());
    assert_eq!(ErrorCategory::of(&error), ErrorCategory::Auth);

    col.drain(&mut rx);
    col.assert_terminal_failure(ErrorCategory::Auth);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_passphrase_is_auth_failure_without_leaking_it() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let auth = AuthChoice::PrivateKey {
        key_path: server.encrypted_key.clone(),
        passphrase: Some(SecretValue::new(WRONG_PASSPHRASE.to_string())),
    };
    let (tx, _rx) = mpsc::channel(256);

    let params = server.params(prompt, auth);
    let error = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect_err("wrong passphrase must fail");

    assert!(
        matches!(error, SshError::WrongPassphrase { .. }),
        "got {error:?}"
    );
    assert!(error.is_auth_failure());
    assert_eq!(ErrorCategory::of(&error), ErrorCategory::Auth);
    let message = error.to_string();
    assert!(
        !message.contains(WRONG_PASSPHRASE) && !message.contains(ENC_PASSPHRASE),
        "passphrase leaked into error: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn correct_passphrase_opens_shell() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let auth = AuthChoice::PrivateKey {
        key_path: server.encrypted_key.clone(),
        passphrase: Some(SecretValue::new(ENC_PASSPHRASE.to_string())),
    };
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, auth);
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("correct passphrase must authenticate");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;
    session
        .write(b"echo ENC_OK\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "echo roundtrip", |d| count(d, b"ENC_OK") >= 2)
        .await;
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_auth_via_explicit_socket() {
    let Some(server) = TestServer::start() else {
        return;
    };
    if which("ssh-agent").is_none() || which("ssh-add").is_none() {
        eprintln!("SKIPPED: ssh-agent/ssh-add not found — agent test not exercised");
        return;
    }

    // Foreground agent with an explicit socket: no process-global env edits.
    let sock = server.dir.join("agent.sock");
    let mut agent = KillOnDrop(
        Command::new("ssh-agent")
            .args(["-a"])
            .arg(&sock)
            .arg("-D")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ssh-agent"),
    );
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(sock.exists(), "agent socket never appeared");

    let added = Command::new("ssh-add")
        .arg(&server.client_key)
        .env("SSH_AUTH_SOCK", &sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run ssh-add");
    assert!(added.success(), "ssh-add must load the client key");

    let prompt = RecordingPrompt::new(true);
    let auth = AuthChoice::Agent {
        socket: Some(sock.clone()),
    };
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, auth);
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("agent authentication must succeed");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;
    session
        .write(b"echo AGENT_OK\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "echo roundtrip", |d| count(d, b"AGENT_OK") >= 2)
        .await;
    session.close().await.expect("close");
    agent.shutdown();
}

/// Kills its child process on drop so a failed assertion cannot leak it.
struct KillOnDrop(Child);

impl KillOnDrop {
    fn shutdown(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resize_updates_pty_size() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let options = SessionOptions {
        cols: 80,
        rows: 24,
        ..SessionOptions::default()
    };
    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, options, tx).await.expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    session.write(b"stty size\n".to_vec()).await.expect("write");
    col.until_data(&mut rx, "initial size 24 80", |d| {
        d.windows(5).any(|w| w == b"24 80")
    })
    .await;

    session.resize(120, 40).await.expect("resize");
    session.write(b"stty size\n".to_vec()).await.expect("write");
    col.until_data(&mut rx, "resized size 40 120", |d| {
        d.windows(6).any(|w| w == b"40 120")
    })
    .await;
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_output_flood_keeps_session_responsive() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    // Small queue on purpose: exercises backpressure between actor and UI.
    let (tx, mut rx) = mpsc::channel(64);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    // 16 * 262144 = 4 MiB of payload.
    session
        .write(b"awk 'BEGIN{for(i=0;i<262144;i++)printf \"0123456789abcdef\"}'\n".to_vec())
        .await
        .expect("write");

    let target = 4 * 1024 * 1024;
    let started = tokio::time::Instant::now();
    let result = tokio::time::timeout(FLOOD_TIMEOUT, async {
        loop {
            if col.data.len() >= target {
                return;
            }
            let event = rx.recv().await.expect("event channel closed");
            col.push(event);
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        panic!(
            "flood stalled after {} of {target} bytes (backpressure deadlock or detach)",
            col.data.len()
        )
    });
    eprintln!("flood: {target} bytes in {:?}", started.elapsed());

    // The session must still be alive and executing commands.
    session
        .write(b"echo FLOOD_DONE\n".to_vec())
        .await
        .expect("write after flood");
    col.until_data(&mut rx, "responsive after flood", |d| {
        count(d, b"FLOOD_DONE") >= 2
    })
    .await;
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reattach_replays_buffer_before_live_data() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    session
        .write(b"echo REPLAY_MARK\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "marker produced", |d| {
        count(d, b"REPLAY_MARK") >= 2
    })
    .await;

    // Move the "view": attach a fresh consumer (as a tab move would).
    let (btx, mut brx) = mpsc::channel(256);
    session.attach(btx).await.expect("attach");
    session
        .write(b"echo LIVE_MARK\n".to_vec())
        .await
        .expect("write");

    let mut buf: Vec<u8> = Vec::new();
    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            if count(&buf, b"LIVE_MARK") >= 1 && count(&buf, b"REPLAY_MARK") >= 1 {
                return;
            }
            let event = brx.recv().await.expect("new consumer channel closed");
            if let SessionEvent::Data(bytes) = event {
                buf.extend_from_slice(&bytes);
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out; {} bytes, tail {:?}",
            buf.len(),
            String::from_utf8_lossy(&buf[buf.len().saturating_sub(400)..])
        )
    });

    let replay_at = buf
        .windows(11)
        .position(|w| w == b"REPLAY_MARK")
        .expect("replay content");
    let live_at = buf
        .windows(9)
        .position(|w| w == b"LIVE_MARK")
        .expect("live content");
    assert!(
        replay_at < live_at,
        "replay must arrive before live bytes: replay@{replay_at} live@{live_at}"
    );
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_connection_reports_network_failure() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let mut server = server;
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    // Sanity: the shell works right before the network dies.
    session
        .write(b"echo ALIVE\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "pre-kill roundtrip", |d| count(d, b"ALIVE") >= 2)
        .await;

    server.interrupt();

    col.until_status(&mut rx, "Failed { Network }", |s| {
        matches!(
            s,
            SessionStatus::Failed {
                category: ErrorCategory::Network,
                ..
            }
        )
    })
    .await;
    col.until_status(&mut rx, "Closed", |s| {
        matches!(s, SessionStatus::Closed { .. })
    })
    .await;

    let failed = col
        .statuses
        .iter()
        .find_map(|s| match s {
            SessionStatus::Failed { message, .. } => Some(message.clone()),
            _ => None,
        })
        .expect("failure message");
    assert!(!failed.is_empty());
    assert!(
        !col.statuses.iter().any(|s| matches!(
            s,
            SessionStatus::Failed {
                category: ErrorCategory::Auth,
                ..
            }
        )),
        "network drop must not be misclassified: {:?}",
        col.statuses
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn binary_output_is_preserved_verbatim() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    session
        .write(b"printf 'A\\000\\377\\376BIN\\001Z'\n".to_vec())
        .await
        .expect("write");
    let expected = b"A\x00\xff\xfeBIN\x01Z";
    col.until_data(&mut rx, "binary blob", |d| {
        d.windows(expected.len()).any(|w| w == expected)
    })
    .await;
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clean_exit_reports_status_without_failure() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    session.write(b"exit 7\n".to_vec()).await.expect("write");
    let code = col.until_exit(&mut rx).await;
    assert_eq!(code, Some(7), "exit code propagated");
    col.until_status(
        &mut rx,
        "Closed",
        |s| matches!(s, SessionStatus::Closed { reason } if reason.contains("status 7")),
    )
    .await;
    assert!(
        !col.statuses
            .iter()
            .any(|s| matches!(s, SessionStatus::Failed { .. })),
        "clean exit must not be a failure: {:?}",
        col.statuses
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_reconnect_gives_up_with_actionable_error() {
    let Some(mut server) = TestServer::start() else {
        return;
    };
    // Server up during setup, then dead: every attempt is a retriable
    // connection refusal, so the policy must bound the retries and give up.
    server.interrupt();

    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(256);
    let (_cancel_tx, cancel) = watch::channel(false);
    let policy = ReconnectPolicy {
        max_attempts: 3,
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(300),
        multiplier: 2.0,
        jitter_ratio: 0.0,
    };

    let params = server.params(prompt, server.client_auth());
    let error = Session::open_with_reconnect(params, SessionOptions::default(), tx, policy, cancel)
        .await
        .expect_err("server is dead");

    assert!(error.is_retriable(), "got {error:?}");

    let mut col = Collector::default();
    col.drain(&mut rx);

    let reconnections: Vec<_> = col
        .statuses
        .iter()
        .filter_map(|s| match s {
            SessionStatus::Reconnecting {
                attempt,
                delay,
                last_error,
            } => Some((*attempt, *delay, last_error.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        reconnections,
        vec![
            (1, Duration::from_millis(100), error.to_string()),
            (2, Duration::from_millis(200), error.to_string()),
        ],
        "bounded, jitter-free backoff schedule: {:?}",
        reconnections
    );

    assert_eq!(
        col.statuses
            .iter()
            .filter(|s| matches!(s, SessionStatus::Failed { .. }))
            .count(),
        1
    );
    let closed = col
        .statuses
        .iter()
        .find_map(|s| match s {
            SessionStatus::Closed { reason } => Some(reason.clone()),
            _ => None,
        })
        .expect("Closed status");
    assert!(
        closed.contains("gave up after 3 attempts"),
        "give-up is explicit: {closed}"
    );
}
