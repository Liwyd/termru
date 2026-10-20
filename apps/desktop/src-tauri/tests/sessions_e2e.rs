//! End-to-end test of the **app layer**: the desktop `SessionManager` drives
//! a real OpenSSH server, and terminal bytes come back through the same
//! `Channel` the frontend uses.
//!
//! This is the layer between the Tauri command surface and `termru-ssh`: it
//! proves that connecting, streaming, writing and closing work through the
//! app's own session ownership.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test failures must panic loudly"
)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use tauri::ipc::{Channel, InvokeResponseBody};
use termru_desktop_lib::{SessionManager, SessionStatusDto, SessionUpdate, spawn_connection};
use termru_ssh::{AuthChoice, ConnectParams, ConnectTimeouts, SessionOptions};
use tokio::sync::mpsc;

const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

fn temp_store() -> (tempfile::TempDir, Arc<termru_store::Store>) {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = termru_store::Store::open(&dir.path().join("app-e2e.sqlite")).expect("store");
    (dir, Arc::new(store))
}

fn channel_to_sender() -> (
    Channel<SessionUpdate>,
    mpsc::UnboundedReceiver<SessionUpdate>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let channel = Channel::new(move |body| {
        if let InvokeResponseBody::Json(value) = body {
            if let Ok(update) = serde_json::from_str::<SessionUpdate>(&value) {
                let _ = tx.send(update);
            }
        }
        Ok(())
    });
    (channel, rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn app_session_connects_streams_and_closes() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };

    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());

    let params = ConnectParams {
        host: "127.0.0.1".into(),
        port: server.port,
        username: support::current_user(),
        auth: AuthChoice::PrivateKey {
            key_path: server.client_key.clone(),
            passphrase: None,
        },
        term: "xterm-256color".into(),
        keepalive_seconds: 0,
        timeouts: ConnectTimeouts {
            connect: Duration::from_secs(5),
            auth: Duration::from_secs(5),
            host_key_decision: Duration::from_secs(10),
        },
        known_hosts_path: server.known_hosts.clone(),
        host_key_prompt: Arc::new(termru_ssh::config::RejectUnknownHostKeys),
    };

    let (channel, mut updates) = channel_to_sender();
    let (id, cancel) = manager.register(termru_core::ids::HostId::new(), params.clone());
    spawn_connection(
        manager.clone(),
        store,
        id.clone(),
        params,
        SessionOptions::default(),
        cancel,
        channel,
    );

    // 1. The session reaches `Ready` and the manager reflects it.
    let ready = wait_for_status(&mut updates, |status| {
        matches!(status, SessionStatusDto::Ready)
    })
    .await;
    assert!(ready, "session never became ready");

    // 2. Writing to the session produces output on the channel.
    manager
        .write(&id, b"echo termru-app-e2e-marker\n".to_vec())
        .await
        .expect("write to session");

    let marker = wait_for_data(&mut updates, b"termru-app-e2e-marker").await;
    assert!(
        marker,
        "terminal output never contained the marker; saw: {updates:?}"
    );

    // 3. The manager reports the session as connected.
    let info = manager.info(&id).expect("session info");
    assert_eq!(info.id, id);
    assert!(matches!(info.status, SessionStatusDto::Ready));

    // 4. Closing removes the session from the owner (no orphan).
    manager.close(&id).await.expect("close session");
    assert!(manager.info(&id).is_none(), "session must be released");
    assert!(manager.list().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn app_session_reports_a_failed_authentication() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };

    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());

    // A key that the server does not authorize: the failure is not
    // retriable, so the app must surface it right away.
    let params = ConnectParams {
        host: "127.0.0.1".into(),
        port: server.port,
        username: support::current_user(),
        auth: AuthChoice::PrivateKey {
            key_path: server.unauthorized_key.clone(),
            passphrase: None,
        },
        term: "xterm-256color".into(),
        keepalive_seconds: 0,
        timeouts: ConnectTimeouts {
            connect: Duration::from_secs(5),
            auth: Duration::from_secs(5),
            host_key_decision: Duration::from_secs(5),
        },
        known_hosts_path: server.known_hosts.clone(),
        host_key_prompt: Arc::new(termru_ssh::config::RejectUnknownHostKeys),
    };

    let (channel, mut updates) = channel_to_sender();
    let (id, cancel) = manager.register(termru_core::ids::HostId::new(), params.clone());
    spawn_connection(
        manager.clone(),
        store,
        id.clone(),
        params,
        SessionOptions::default(),
        cancel,
        channel,
    );

    let failed = wait_for_status(&mut updates, |status| {
        matches!(status, SessionStatusDto::Failed { .. })
    })
    .await;
    assert!(failed, "a rejected key must surface a failure status");

    // The session stays registered (so the UI can offer "Reconnect") but is
    // no longer connected.
    let info = manager.info(&id).expect("session info");
    assert!(info.status.is_terminal());
    assert!(manager.status(&id).is_some_and(|s| s.is_terminal()));

    manager.close(&id).await.expect("close session");
    assert!(manager.info(&id).is_none());
}

async fn wait_for_status(
    updates: &mut mpsc::UnboundedReceiver<SessionUpdate>,
    predicate: impl Fn(&SessionStatusDto) -> bool,
) -> bool {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        let Ok(received) = tokio::time::timeout_at(deadline, updates.recv()).await else {
            return false;
        };
        let Some(update) = received else {
            return false;
        };
        if let SessionUpdate::Status { status } = update {
            if predicate(&status) {
                return true;
            }
        }
    }
    false
}

async fn wait_for_data(
    updates: &mut mpsc::UnboundedReceiver<SessionUpdate>,
    needle: &[u8],
) -> bool {
    use base64::Engine;
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    let mut seen = String::new();
    while tokio::time::Instant::now() < deadline {
        let Ok(received) = tokio::time::timeout_at(deadline, updates.recv()).await else {
            break;
        };
        let Some(update) = received else {
            break;
        };
        if let SessionUpdate::Data { base64 } = update {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(base64)
                .unwrap_or_default();
            seen.push_str(&String::from_utf8_lossy(&bytes));
            if seen.contains(std::str::from_utf8(needle).unwrap_or_default()) {
                return true;
            }
        }
    }
    false
}
