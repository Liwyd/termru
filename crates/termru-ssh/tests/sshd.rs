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
    NoAuthPrompts, ReconnectPolicy, SecretValue, Session, SessionEvent, SessionOptions,
    SessionStatus, SshError,
};

const ENC_PASSPHRASE: &str = "kite-pass-test";
const WRONG_PASSPHRASE: &str = "definitely-wrong-passphrase";
const WRONG_PASSWORD: &str = "definitely-not-the-password";
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);
const FLOOD_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------- tooling --

/// A key-file method without identity or passphrase.
fn key_auth(path: &Path) -> AuthChoice {
    AuthChoice::PrivateKey {
        key_path: path.to_path_buf(),
        passphrase: None,
        identity: None,
    }
}

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
        Self::start_with("", |_| {})
    }

    /// Starts a server whose `sshd_config` begins with `extra` (`{dir}` is
    /// replaced by the server's directory). sshd keeps the **first** value of
    /// most keywords, so the extra lines override the defaults below. `setup`
    /// runs before sshd starts and may create the files `extra` refers to.
    fn start_with(extra: &str, setup: impl FnOnce(&Path)) -> Option<Self> {
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
        setup(&dir);
        let extra = extra.replace("{dir}", &dir.display().to_string());

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
                "{extra}\n\
                 Port {port}\n\
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
    ///
    /// The connection processes die **before** the shell they serve: killing
    /// the shell first makes sshd report an exit signal to the client (a
    /// clean, final close) before it is killed itself, which is a different
    /// scenario from a vanished transport.
    fn interrupt(&mut self) {
        let mut pids = vec![self.child.id()];
        collect_descendants(self.child.id(), &mut pids);
        for pid in pids {
            kill_pid(pid);
        }
        self.killed = true;
    }

    fn client_auth(&self) -> AuthChoice {
        key_auth(&self.client_key)
    }

    fn params(&self, prompt: Arc<dyn HostKeyPrompt>, auth: AuthChoice) -> ConnectParams {
        self.chain_params(prompt, vec![auth])
    }

    fn chain_params(&self, prompt: Arc<dyn HostKeyPrompt>, auth: Vec<AuthChoice>) -> ConnectParams {
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
                ..ConnectTimeouts::default()
            },
            known_hosts_path: self.known_hosts.clone(),
            host_key_prompt: prompt,
            auth_prompt: Arc::new(NoAuthPrompts),
        }
    }

    /// Authorizes one more public key (sshd re-reads the file per attempt).
    fn authorize(&self, public_key_file: &Path) {
        use std::io::Write;
        let path = self.dir.join("authorized_keys");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open authorized_keys");
        let mut line = std::fs::read(public_key_file).expect("read public key");
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        file.write_all(&line).expect("append key");
    }

    fn sshd_log(&self) -> String {
        std::fs::read_to_string(self.dir.join("sshd.log")).unwrap_or_default()
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
    banners: Vec<String>,
    statuses: Vec<SessionStatus>,
    exits: Vec<Option<i32>>,
}

impl Collector {
    fn push(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Data(bytes) => self.data.extend_from_slice(&bytes),
            SessionEvent::Banner(text) => self.banners.push(text),
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
async fn replay_snapshot_returns_the_buffered_output() {
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
        .write(b"echo replay-marker\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "replay-marker", |data| {
        count(data, b"replay-marker") > 0
    })
    .await;

    let snapshot = session.replay_snapshot().await.expect("replay snapshot");
    let text = String::from_utf8_lossy(&snapshot);
    assert!(
        text.contains("replay-marker"),
        "replay snapshot lacks the marker: {text:?}"
    );
}

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
        HostKeyPromptRequest::Unknown {
            host, port, key, ..
        } => {
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
    let auth = key_auth(&server.bad_key);
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
        identity: None,
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
        identity: None,
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
        prefer_fingerprint: None,
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

// ------------------------------------------------- engine robustness tests --

/// Regression test (large paste deadlock): the actor used to await the write
/// of a pasted chunk itself. While the remote echoed the data back, nobody
/// drained the channel, russh stalled on its bounded queue, the window adjust
/// was never processed, and both directions froze. Reading and writing are
/// separate tasks now; a 2 MiB paste into `cat` (echoed by the tty *and*
/// copied by `cat`: well over 4 MiB of output) must complete.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_paste_while_the_remote_echoes_does_not_deadlock() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(64);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    session.write(b"cat\n".to_vec()).await.expect("write");

    // Short lines: a canonical-mode tty discards lines longer than 4 KiB.
    let line = format!("{}\n", "0123456789abcdef".repeat(4));
    let chunk = line.repeat(1000).into_bytes();
    let chunks = (2 * 1024 * 1024) / chunk.len() + 1;
    let pasted = chunk.len() * chunks;
    let writer = {
        let session = session.clone();
        tokio::spawn(async move {
            for _ in 0..chunks {
                session.write(chunk.clone()).await?;
            }
            // End of input for `cat` (Ctrl-D at a line start), then a marker
            // that only the shell's *output* contains.
            session
                .write(b"\x04echo PASTE_DONE_$((1+1))\n".to_vec())
                .await
        })
    };

    let started = tokio::time::Instant::now();
    let result = tokio::time::timeout(FLOOD_TIMEOUT, async {
        loop {
            if count(&col.data, b"PASTE_DONE_2") >= 1 {
                return;
            }
            let event = rx.recv().await.expect("event channel closed");
            col.push(event);
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        panic!(
            "paste deadlocked after {} output bytes of {pasted} pasted",
            col.data.len()
        )
    });
    eprintln!(
        "paste: {pasted} bytes in, {} bytes out, {:?}",
        col.data.len(),
        started.elapsed()
    );
    assert!(
        col.data.len() >= pasted,
        "every pasted byte comes back at least once"
    );
    writer.await.expect("writer joined").expect("paste queued");
    session.close().await.expect("close");
}

/// Regression test: `close` used to queue behind a blocked write in the
/// actor's single command loop, so a tab whose remote had stopped reading
/// input could not be closed. The remote here never reads its input (the
/// shell is busy in a loop) while megabytes of it are queued; the close must
/// still go through and end with `Closed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_is_not_stuck_behind_blocked_input() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    let (tx, mut rx) = mpsc::channel(64);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    session
        .write(b"echo BUSY_$((1+1)); while true; do sleep 0.2; done\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "busy loop running", |d| count(d, b"BUSY_2") >= 1)
        .await;

    // Input nobody reads: far more than the receive window.
    let flood = "0123456789abcdef".repeat(4).into_bytes();
    let mut block = Vec::new();
    for _ in 0..1000 {
        block.extend_from_slice(&flood);
        block.push(b'\n');
    }
    for _ in 0..80 {
        session.write(block.clone()).await.expect("queue flood");
    }
    tokio::time::sleep(Duration::from_secs(1)).await;

    session.close().await.expect("close");
    col.until_status(&mut rx, "Closed", |s| {
        matches!(s, SessionStatus::Closed { .. })
    })
    .await;
}

/// Regression test: 1000 small writes, half through `write`, half through
/// the synchronous `try_write`, reach the remote in call order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordered_small_writes_arrive_in_order() {
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

    // Echo off first, so the output is `cat`'s clean line-by-line copy and
    // not interleaved with the tty's character echo.
    session
        .write(b"stty -echo; echo ECHO_OFF_$((1+1)); cat\n".to_vec())
        .await
        .expect("write");
    col.until_data(&mut rx, "echo disabled", |d| count(d, b"ECHO_OFF_2") >= 1)
        .await;
    let before = col.data.len();
    for i in 1..=1000u32 {
        let line = format!("ORD{i:04}\n").into_bytes();
        if i % 2 == 0 {
            session.try_write(line).expect("try_write");
        } else {
            session.write(line).await.expect("write");
        }
    }
    col.until_data(&mut rx, "last token copied", |d| count(d, b"ORD1000") >= 1)
        .await;

    let mut seen: Vec<u32> = Vec::new();
    for window in col.data[before..].windows(7) {
        if &window[..3] == b"ORD" && window[3..].iter().all(u8::is_ascii_digit) {
            seen.push(
                std::str::from_utf8(&window[3..])
                    .expect("digits")
                    .parse()
                    .expect("number"),
            );
        }
    }
    assert_eq!(
        seen,
        (1..=1000).collect::<Vec<u32>>(),
        "writes were lost, duplicated or reordered"
    );
    session.close().await.expect("close");
}

/// Regression test: a shell killed by a signal used to be reported as a
/// retriable network failure (so the app reconnected in a loop). It is a
/// final close with a readable reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killed_shell_is_a_final_close_not_a_network_failure() {
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
        .write(b"kill -9 $$\n".to_vec())
        .await
        .expect("write");
    let closed = col
        .until_status(&mut rx, "Closed", |s| {
            matches!(s, SessionStatus::Closed { .. })
        })
        .await;
    let SessionStatus::Closed { reason } = closed else {
        unreachable!("until_status returned the matching status");
    };
    assert!(
        reason.contains("signal") && reason.contains("KILL"),
        "the reason names the signal: {reason}"
    );
    assert!(
        !col.statuses.iter().any(|s| matches!(
            s,
            SessionStatus::Failed { .. } | SessionStatus::Reconnecting { .. }
        )),
        "a killed shell is not a connection failure: {:?}",
        col.statuses
    );
}

/// Regression test: the actor used to drop its consumer when a send took
/// longer than 5 s (a stalled UI), silently losing all later output and the
/// final status. A consumer that stalls for longer must still get every
/// byte, in order, afterwards.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_consumer_is_never_dropped() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let prompt = RecordingPrompt::new(true);
    // Room for the connect statuses only; once those are read, nobody reads
    // and every further send would block.
    let (tx, mut rx) = mpsc::channel(4);
    let mut col = Collector::default();

    let params = server.params(prompt, server.client_auth());
    let session = Session::open(params, SessionOptions::default(), tx)
        .await
        .expect("connect");
    col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
        .await;

    session
        .write(
            b"i=0; while [ $i -lt 20000 ]; do echo line-$i; i=$((i+1)); done; \
              echo STALL_DONE_$((1+1))\n"
                .to_vec(),
        )
        .await
        .expect("write");
    // Longer than the old 5 s send timeout.
    tokio::time::sleep(Duration::from_secs(6)).await;

    col.until_data(&mut rx, "output after the stall", |d| {
        count(d, b"STALL_DONE_2") >= 1
    })
    .await;
    assert!(
        count(&col.data, b"line-19999\r\n") >= 1,
        "output produced during the stall was lost"
    );
    // The session is still alive and answers.
    session
        .write(b"echo AFTER_STALL_$((1+1))\n".to_vec())
        .await
        .expect("write after the stall");
    col.until_data(&mut rx, "responsive after the stall", |d| {
        count(d, b"AFTER_STALL_2") >= 1
    })
    .await;
    session.close().await.expect("close");
}

// ------------------------------------------------- the authentication chain --

fn keygen_typed(path: &Path, key_type: &str, bits: &str) {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", key_type, "-b", bits, "-N", "", "-f"])
        .arg(path)
        .status()
        .expect("ssh-keygen must be available for integration tests");
    assert!(status.success(), "ssh-keygen failed for {}", path.display());
}

/// Opens a session and returns once it is `Ready`; the error otherwise.
async fn open_ready(
    params: ConnectParams,
) -> (
    Result<Session, SshError>,
    Collector,
    mpsc::Receiver<SessionEvent>,
) {
    let (tx, mut rx) = mpsc::channel(256);
    let mut col = Collector::default();
    let result = Session::open(params, SessionOptions::default(), tx).await;
    if result.is_ok() {
        col.until_status(&mut rx, "Ready", |s| matches!(s, SessionStatus::Ready))
            .await;
    } else {
        col.drain(&mut rx);
    }
    (result, col, rx)
}

/// An RSA key must sign with the hash the server announced (`server-sig-algs`),
/// not a hard-coded one: this server accepts exactly one RSA signature
/// algorithm, so a wrong guess is a rejected login.
async fn rsa_login_with(accepted: &str) {
    let Some(server) =
        TestServer::start_with(&format!("PubkeyAcceptedAlgorithms {accepted}"), |_| {})
    else {
        return;
    };
    let rsa = server.dir.join("id_rsa");
    keygen_typed(&rsa, "rsa", "2048");
    server.authorize(&pub_of(&rsa));

    let params = server.params(RecordingPrompt::new(true), key_auth(&rsa));
    let (result, _col, _rx) = open_ready(params).await;
    let session = result.unwrap_or_else(|e| panic!("RSA login with {accepted} failed: {e}"));
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rsa_key_uses_sha512_when_the_server_only_takes_sha512() {
    rsa_login_with("rsa-sha2-512").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rsa_key_uses_sha256_when_the_server_only_takes_sha256() {
    rsa_login_with("rsa-sha2-256").await;
}

/// The password method is configured first but the server does not allow
/// it: the chain must go on with the key instead of failing on the password.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_method_the_server_does_not_allow_is_skipped() {
    let Some(server) = TestServer::start_with("PasswordAuthentication no", |_| {}) else {
        return;
    };
    let chain = vec![
        AuthChoice::Password {
            password: SecretValue::new(WRONG_PASSWORD.to_string()),
        },
        server.client_auth(),
    ];
    let (result, _col, _rx) =
        open_ready(server.chain_params(RecordingPrompt::new(true), chain)).await;
    let session = result.expect("the key must carry the chain");
    session.close().await.expect("close");
    assert!(
        !server.sshd_log().contains("Failed password"),
        "the password must not have been tried: {}",
        server.sshd_log()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_nothing_is_allowed_the_error_says_what_the_server_wants() {
    let Some(server) = TestServer::start_with("PasswordAuthentication no", |_| {}) else {
        return;
    };
    let params = server.params(
        RecordingPrompt::new(true),
        AuthChoice::Password {
            password: SecretValue::new(WRONG_PASSWORD.to_string()),
        },
    );
    let (result, _col, _rx) = open_ready(params).await;
    let error = result.expect_err("no usable method");
    let message = error.to_string();
    assert!(error.is_auth_failure(), "{message}");
    assert!(message.contains("set up for password"), "{message}");
    assert!(message.contains("the server allows publickey"), "{message}");
    assert!(!message.contains(WRONG_PASSWORD), "{message}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_chain_lists_every_method_tried_and_what_the_server_allows() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let chain = vec![
        AuthChoice::Password {
            password: SecretValue::new(WRONG_PASSWORD.to_string()),
        },
        key_auth(&server.bad_key),
    ];
    let (result, _col, _rx) =
        open_ready(server.chain_params(RecordingPrompt::new(true), chain)).await;
    let error = result.expect_err("both methods must fail");
    let message = error.to_string();
    assert!(
        matches!(
            &error,
            SshError::AuthFailed {
                method: "multiple methods",
                ..
            }
        ),
        "{error:?}"
    );
    assert!(
        message.contains("password (rejected by the server)"),
        "{message}"
    );
    assert!(
        message.contains("publickey (rejected by the server)"),
        "{message}"
    );
    assert!(message.contains("the server allows"), "{message}");
    assert!(!message.contains(WRONG_PASSWORD), "{message}");
}

/// A certificate next to the key (`<key>-cert.pub`) is used. The key itself
/// is not in `authorized_keys`: only the CA the server trusts can let it in.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_certificate_next_to_the_key_is_used() {
    let user = current_user();
    let Some(server) = TestServer::start_with("TrustedUserCAKeys {dir}/ca.pub", |dir| {
        keygen(&dir.join("ca"), "");
        keygen(&dir.join("id_certified"), "");
        let signed = Command::new("ssh-keygen")
            .args(["-q", "-s"])
            .arg(dir.join("ca"))
            .args(["-I", "termru-test", "-n", &user, "-V", "+1h"])
            .arg(dir.join("id_certified.pub"))
            .status()
            .expect("ssh-keygen -s");
        assert!(signed.success(), "could not sign the user certificate");
    }) else {
        return;
    };
    let key = server.dir.join("id_certified");
    assert!(server.dir.join("id_certified-cert.pub").is_file());

    let (result, _col, _rx) =
        open_ready(server.params(RecordingPrompt::new(true), key_auth(&key))).await;
    let session = result.expect("the certificate must authenticate the key");
    session.close().await.expect("close");
    assert!(
        server.sshd_log().contains("ED25519-CERT"),
        "sshd should report a certificate login: {}",
        server.sshd_log()
    );
}

const BANNER_FILE: &str = "Authorized use only.\n\x1b[2J\x1b]0;pwned\x07second line\n";

fn banner_setup(dir: &Path) {
    std::fs::write(dir.join("banner.txt"), BANNER_FILE).expect("write banner");
}

/// The server's banner reaches the consumer **before** `Ready`, as plain
/// text: the escape sequences in it must not survive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_server_banner_is_forwarded_as_plain_text_before_ready() {
    let Some(server) = TestServer::start_with("Banner {dir}/banner.txt", banner_setup) else {
        return;
    };
    let (result, col, _rx) =
        open_ready(server.params(RecordingPrompt::new(true), server.client_auth())).await;
    let session = result.expect("connect");
    let banner = col.banners.concat();
    assert!(banner.contains("Authorized use only."), "{banner:?}");
    assert!(banner.contains("second line"), "{banner:?}");
    assert!(
        !banner.chars().any(|c| c.is_control() && c != '\n'),
        "no control characters may survive: {banner:?}"
    );
    session.close().await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_banner_is_forwarded_when_authentication_fails_too() {
    let Some(server) = TestServer::start_with("Banner {dir}/banner.txt", banner_setup) else {
        return;
    };
    let (result, col, _rx) =
        open_ready(server.params(RecordingPrompt::new(true), key_auth(&server.bad_key))).await;
    assert!(result.is_err());
    assert!(
        col.banners.concat().contains("Authorized use only."),
        "a refused login is where the banner matters: {:?}",
        col.banners
    );
}

// ---------------------------------------------------------- a fake SSH agent --

/// A minimal SSH agent that lists its keys **in a fixed order** (the agent
/// russh ships keeps them in a hash map) and refuses to sign with some of
/// them.
struct FakeAgent {
    sock: PathBuf,
    /// Fingerprints the agent signed with, in order.
    signed: Arc<Mutex<Vec<String>>>,
    /// Fingerprints it was asked to sign with but refused.
    refused: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeAgent {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn put_string(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).expect("length").to_be_bytes());
    out.extend_from_slice(data);
}

fn take_string<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let (len, rest) = input.split_first_chunk::<4>()?;
    let len = usize::try_from(u32::from_be_bytes(*len)).ok()?;
    let (data, rest) = rest.split_at_checked(len)?;
    *input = rest;
    Some(data)
}

fn start_fake_agent(sock: &Path, keys: Vec<(russh::keys::PrivateKey, bool)>) -> FakeAgent {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::UnixListener::bind(sock).expect("bind agent socket");
    let keys = Arc::new(keys);
    let signed = Arc::new(Mutex::new(Vec::new()));
    let refused = Arc::new(Mutex::new(Vec::new()));
    let (signed_log, refused_log) = (signed.clone(), refused.clone());

    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (keys, signed, refused) = (keys.clone(), signed_log.clone(), refused_log.clone());
            tokio::spawn(async move {
                loop {
                    let mut len = [0u8; 4];
                    if stream.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let mut body = vec![0u8; usize::try_from(u32::from_be_bytes(len)).unwrap()];
                    if stream.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    let reply = match body.split_first() {
                        // SSH_AGENTC_REQUEST_IDENTITIES
                        Some((11, _)) => {
                            let mut out = vec![12];
                            out.extend_from_slice(
                                &u32::try_from(keys.len()).unwrap().to_be_bytes(),
                            );
                            for (key, _) in keys.iter() {
                                put_string(&mut out, &key.public_key().to_bytes().unwrap());
                                put_string(&mut out, b"fake");
                            }
                            out
                        }
                        // SSH_AGENTC_SIGN_REQUEST
                        Some((13, mut rest)) => {
                            let blob = take_string(&mut rest).unwrap_or_default().to_vec();
                            let data = take_string(&mut rest).unwrap_or_default().to_vec();
                            match keys
                                .iter()
                                .find(|(k, _)| k.public_key().to_bytes().ok() == Some(blob.clone()))
                            {
                                Some((key, refuse)) => {
                                    let fp = format!(
                                        "{}",
                                        key.public_key().fingerprint(russh::keys::HashAlg::Sha256)
                                    );
                                    if *refuse {
                                        refused.lock().unwrap().push(fp);
                                        vec![5] // SSH_AGENT_FAILURE
                                    } else {
                                        let signature: russh::keys::ssh_key::Signature =
                                            russh::keys::signature::Signer::try_sign(key, &data)
                                                .expect("sign");
                                        signed.lock().unwrap().push(fp);
                                        let mut out = vec![14];
                                        put_string(
                                            &mut out,
                                            &russh::keys::ssh_encoding::Encode::encode_vec(
                                                &signature,
                                            )
                                            .expect("encode signature"),
                                        );
                                        out
                                    }
                                }
                                None => vec![5],
                            }
                        }
                        _ => vec![5],
                    };
                    let mut frame = u32::try_from(reply.len()).unwrap().to_be_bytes().to_vec();
                    frame.extend_from_slice(&reply);
                    if stream.write_all(&frame).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    FakeAgent {
        sock: sock.to_path_buf(),
        signed,
        refused,
        task,
    }
}

fn load_key(path: &Path) -> russh::keys::PrivateKey {
    russh::keys::load_secret_key(path, None).expect("load test key")
}

fn agent_auth(agent: &FakeAgent, prefer: Option<String>) -> AuthChoice {
    AuthChoice::Agent {
        socket: Some(agent.sock.clone()),
        prefer_fingerprint: prefer,
    }
}

/// The agent lists an authorized key that it then declines to sign with (a
/// confirmation was refused, the key was locked...): the next key must be
/// tried instead of giving up on the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_key_that_cannot_sign_is_skipped() {
    let Some(server) = TestServer::start() else {
        return;
    };
    server.authorize(&pub_of(&server.decoy_key));
    let agent = start_fake_agent(
        &server.dir.join("fake-agent.sock"),
        vec![
            (load_key(&server.decoy_key), true),
            (load_key(&server.client_key), false),
        ],
    );

    let (result, _col, _rx) =
        open_ready(server.params(RecordingPrompt::new(true), agent_auth(&agent, None))).await;
    let session = result.expect("the second key must authenticate");
    session.close().await.expect("close");

    assert_eq!(
        agent.refused.lock().unwrap().as_slice(),
        [fingerprint_of(&pub_of(&server.decoy_key))],
        "the first key was asked to sign and declined"
    );
    assert_eq!(
        agent.signed.lock().unwrap().as_slice(),
        [fingerprint_of(&pub_of(&server.client_key))]
    );
}

/// An agent with many keys must not be offered in full: sshd drops the
/// connection after `MaxAuthTries` failures, and the later methods would
/// never get their turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_first_agent_keys_are_offered() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let mut keys = Vec::new();
    for index in 0..7 {
        let path = server.dir.join(format!("id_unauthorized_{index}"));
        keygen(&path, "");
        keys.push((load_key(&path), false));
    }
    keys.push((load_key(&server.client_key), false));
    let agent = start_fake_agent(&server.dir.join("fake-agent.sock"), keys);

    let (result, _col, _rx) =
        open_ready(server.params(RecordingPrompt::new(true), agent_auth(&agent, None))).await;
    let error = result.expect_err("the authorized key is beyond the cap");
    let message = error.to_string();
    assert!(
        message.contains("rejected 5 of the agent's 8 key(s)"),
        "{message}"
    );
    assert!(message.contains("only the first 5"), "{message}");
    assert!(
        agent.signed.lock().unwrap().is_empty(),
        "the authorized key was never reached"
    );
}

/// With a key tied to the connection, that key goes first: this server
/// allows two attempts, so the right key at the end of the agent's list is
/// only reached when it is moved to the front.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_preferred_agent_key_is_offered_first() {
    let Some(server) = TestServer::start_with("MaxAuthTries 2", |_| {}) else {
        return;
    };
    let mut keys = Vec::new();
    for index in 0..3 {
        let path = server.dir.join(format!("id_unauthorized_{index}"));
        keygen(&path, "");
        keys.push((load_key(&path), false));
    }
    keys.push((load_key(&server.client_key), false));
    let agent = start_fake_agent(&server.dir.join("fake-agent.sock"), keys);

    let without = open_ready(server.params(RecordingPrompt::new(true), agent_auth(&agent, None)))
        .await
        .0;
    assert!(
        without.is_err(),
        "without a preference the right key is out of reach"
    );

    let preferred = fingerprint_of(&pub_of(&server.client_key));
    let (result, _col, _rx) = open_ready(server.params(
        RecordingPrompt::new(true),
        agent_auth(&agent, Some(preferred)),
    ))
    .await;
    let session = result.expect("the preferred key goes first");
    session.close().await.expect("close");
}

// ------------------------------------------------------------------ host keys --

/// The host is known under ECDSA; the server (ed25519 only) presents a type
/// we have not seen. The question must say so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_prompt_lists_the_other_key_types_known_for_the_host() {
    let Some(server) = TestServer::start() else {
        return;
    };
    let other = server.dir.join("other_ecdsa");
    keygen_typed(&other, "ecdsa", "256");
    server.seed_known_hosts(&pubkey_line(&pub_of(&other)));

    let prompt = RecordingPrompt::new(false);
    let (result, _col, _rx) = open_ready(server.params(prompt.clone(), server.client_auth())).await;
    assert!(matches!(result, Err(SshError::HostKeyRejected)));
    match &prompt.seen()[0] {
        HostKeyPromptRequest::Unknown {
            key,
            other_key_types,
            ..
        } => {
            assert_eq!(key.key_type, "ssh-ed25519");
            assert_eq!(other_key_types, &["ecdsa-sha2-nistp256".to_string()]);
        }
        other => panic!("expected an Unknown prompt, got {other:?}"),
    }
}

/// The server owns an ed25519 and an ECDSA host key; we know the ECDSA one.
/// Asking for the known type first makes it present that one, so there is no
/// prompt at all (russh's own order would have shown the ed25519 key).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_known_host_key_type_is_requested_first() {
    let Some(server) = TestServer::start_with("HostKey {dir}/host_ecdsa", |dir| {
        keygen_typed(&dir.join("host_ecdsa"), "ecdsa", "256");
    }) else {
        return;
    };
    server.seed_known_hosts(&pubkey_line(&pub_of(&server.dir.join("host_ecdsa"))));

    let prompt = RecordingPrompt::new(false);
    let (result, _col, _rx) = open_ready(server.params(prompt.clone(), server.client_auth())).await;
    let session = result.expect("the known ECDSA key must be presented and trusted");
    assert!(prompt.seen().is_empty(), "{:?}", prompt.seen());
    session.close().await.expect("close");
}
