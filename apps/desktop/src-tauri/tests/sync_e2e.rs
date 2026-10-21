//! End-to-end test of the desktop sync client against the **real** cloud
//! server.
//!
//! The test starts the `termru-cloud-server` binary against a PostgreSQL
//! server, signs in with the desktop sync engine, pushes a host and pulls it
//! back. It proves that both sides agree on the protocol.
//!
//! Prerequisites:
//!
//! * `TERMRU_TEST_DATABASE_URL` — a PostgreSQL URL the server may use
//!   (the test creates its own database).
//! * `TERMRU_CLOUD_SERVER_BIN` — the path of the built server binary.
//!
//! Without them the test prints a loud `SKIPPED:` line and returns.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "harness setup failures must panic loudly in tests"
)]

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use termru_desktop_lib::sync::SyncEngine;
use termru_desktop_lib::sync::client::{SyncError, SyncTransport};
use termru_desktop_lib::sync::tokens::MemoryTokens;
use termru_store::Store;
use uuid::Uuid;

/// A running cloud server.
struct Server {
    child: Child,
    url: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A free TCP port on the loopback interface.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a port");
    listener.local_addr().expect("the local address").port()
}

/// Starts the cloud server and waits until it is ready.
///
/// Returns `None` when a prerequisite is missing (the test then skips). A
/// prerequisite that is present but broken is a failure, not a skip.
async fn start_server(admin_url: &str) -> Option<Server> {
    let binary = std::env::var("TERMRU_CLOUD_SERVER_BIN").ok()?;
    let binary = PathBuf::from(binary);
    if !binary.exists() {
        eprintln!("SKIPPED: {} does not exist", binary.display());
        return None;
    }

    let database = format!("termru_e2e_{}", Uuid::new_v4().simple());
    let psql = Command::new("psql")
        .arg(admin_url)
        .arg("-c")
        .arg(format!(r#"CREATE DATABASE "{database}""#))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    assert!(
        psql.success(),
        "could not create the test database {database} on {admin_url}"
    );

    let port = free_port();
    let base = admin_url.split('?').next().unwrap_or(admin_url);
    let head = base.rsplit_once('/').map(|(head, _)| head).unwrap_or(base);
    let database_url = format!("{head}/{database}");

    let child = Command::new(&binary)
        .env("TERMRU_DATABASE_URL", &database_url)
        .env("TERMRU_BIND_ADDR", format!("127.0.0.1:{port}"))
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the cloud server");

    let server = Server {
        child,
        url: format!("http://127.0.0.1:{port}"),
    };

    // Wait for the readiness probe.
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if ready(&server.url).await {
            return Some(server);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("the cloud server did not become ready on {}", server.url);
}

/// Asks the server whether it is ready.
async fn ready(url: &str) -> bool {
    let Ok(transport) = termru_desktop_lib::sync::client::HttpTransport::new(url) else {
        return false;
    };
    // The transport has no raw GET, so the probe uses a request that is
    // expected to be refused: a *network* error means the server is not up
    // yet, any other answer means it is.
    match transport.pull("not-a-token", 0, 1).await {
        Ok(_) => true,
        Err(SyncError::Network(_)) => false,
        Err(_) => true,
    }
}

#[tokio::test]
async fn a_host_survives_the_round_trip_through_the_real_server() {
    let Some(admin_url) = std::env::var("TERMRU_TEST_DATABASE_URL").ok() else {
        eprintln!("SKIPPED: set TERMRU_TEST_DATABASE_URL to run the sync end-to-end test");
        return;
    };

    let Some(server) = start_server(&admin_url).await else {
        return;
    };

    let dir = tempfile::tempdir().expect("a temp dir");
    let store = std::sync::Arc::new(
        Store::open(&dir.path().join("sync.sqlite")).expect("open the local store"),
    );
    let tokens = std::sync::Arc::new(MemoryTokens::new());
    let engine = SyncEngine::with_token_store(store.clone(), tokens);

    // Sign in: the server creates the account and the device.
    engine
        .sign_in(
            &server.url,
            "e2e@example.com",
            "correct horse battery staple",
            "e2e device",
        )
        .await
        .expect("sign in");

    // A local host that has to travel to the server.
    let mut host = termru_core::Host::new(
        "web-1".to_string(),
        "web-1.example.com".to_string(),
        "deploy".to_string(),
    );
    store
        .repos()
        .hosts()
        .save(&mut host)
        .expect("save the host");

    // Push it and pull it back.
    let outcome = engine.run_cycle().await.expect("the cycle must succeed");
    assert!(outcome.pushed >= 1, "the host must be pushed");
    assert_eq!(outcome.conflicts, 0);

    // The server has the host: a second device (a fresh local store) sees it.
    let other_dir = tempfile::tempdir().expect("a temp dir");
    let other_store = std::sync::Arc::new(
        Store::open(&other_dir.path().join("sync.sqlite")).expect("open the second store"),
    );
    let other_tokens = std::sync::Arc::new(MemoryTokens::new());
    let other = SyncEngine::with_token_store(other_store.clone(), other_tokens);
    other
        .sign_in(
            &server.url,
            "e2e@example.com",
            "correct horse battery staple",
            "second device",
        )
        .await
        .expect("sign in on the second device");

    let outcome = other.run_cycle().await.expect("the cycle must succeed");
    assert!(
        outcome.pulled >= 1,
        "the second device must receive the host"
    );

    let hosts = other_store.repos().hosts().list().expect("list the hosts");
    let received = hosts
        .iter()
        .find(|entry| entry.id == host.id)
        .expect("the host arrived on the second device");
    assert_eq!(received.name, "web-1");
    assert_eq!(received.address, "web-1.example.com");
    assert_eq!(received.username, "deploy");
}
