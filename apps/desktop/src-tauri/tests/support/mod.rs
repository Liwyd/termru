//! Minimal OpenSSH server harness for the desktop app's end-to-end test.
//!
//! Starts a private `sshd` as the current (unprivileged) user on a high
//! port with its own host key, authorized key and known-hosts file. When the
//! OpenSSH tooling is missing the caller must skip loudly instead of
//! silently passing.

#![allow(dead_code)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test harness setup failures must panic loudly"
)]

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub struct TestServer {
    pub dir: PathBuf,
    pub port: u16,
    pub client_key: PathBuf,
    /// A key that is **not** in `authorized_keys` (used for auth failures).
    pub unauthorized_key: PathBuf,
    pub known_hosts: PathBuf,
    child: Child,
    killed: bool,
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

pub fn find_sshd() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("TERMru_SSHD") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    for candidate in ["/usr/sbin/sshd", "/usr/bin/sshd", "/sbin/sshd"] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    which("sshd")
}

pub fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

fn keygen(path: &Path) {
    let output = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-f"])
        .arg(path)
        .output()
        .expect("run ssh-keygen");
    assert!(
        output.status.success(),
        "ssh-keygen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn pub_of(key: &Path) -> PathBuf {
    key.with_extension("pub")
}

impl TestServer {
    /// Starts the server, or returns `None` when the OpenSSH tooling is
    /// unavailable (the caller then prints a `SKIPPED:` line).
    pub fn start() -> Option<Self> {
        let Some(sshd) = find_sshd() else {
            eprintln!("SKIPPED: sshd not found — app-layer E2E not exercised");
            return None;
        };
        if which("ssh-keygen").is_none() {
            eprintln!("SKIPPED: ssh-keygen not found — app-layer E2E not exercised");
            return None;
        }

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "termru-app-e2e-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let host_key = dir.join("host_ed25519");
        let client_key = dir.join("id_client");
        let unauthorized_key = dir.join("id_unauthorized");
        keygen(&host_key);
        keygen(&client_key);
        keygen(&unauthorized_key);

        let authorized_keys = dir.join("authorized_keys");
        std::fs::copy(pub_of(&client_key), &authorized_keys).expect("authorized_keys");
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
                 PasswordAuthentication no\n\
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
            client_key,
            unauthorized_key,
            known_hosts,
            child,
            killed: false,
        };
        server.wait_ready(&log_path);
        server.seed_known_hosts();
        Some(server)
    }

    fn wait_ready(&mut self, log_path: &Path) {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        for _ in 0..200 {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
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

    /// Writes the server's host key into the known-hosts file so the
    /// connection is trusted without a UI prompt.
    fn seed_known_hosts(&self) {
        let output = Command::new("ssh-keygen")
            .args(["-y", "-f"])
            .arg(self.dir.join("host_ed25519"))
            .output()
            .expect("run ssh-keygen -y");
        assert!(output.status.success(), "ssh-keygen -y failed");
        let line = String::from_utf8_lossy(&output.stdout);
        let key_line = line
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        std::fs::write(
            &self.known_hosts,
            format!("[127.0.0.1]:{} {key_line}\n", self.port),
        )
        .expect("seed known_hosts");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if self.killed {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
