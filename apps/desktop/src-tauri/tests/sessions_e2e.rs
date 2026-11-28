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
use std::sync::atomic::{AtomicBool, Ordering};
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
        auth: vec![AuthChoice::PrivateKey {
            key_path: server.client_key.clone(),
            passphrase: None,
            identity: None,
        }],
        term: "xterm-256color".into(),
        keepalive_seconds: 0,
        timeouts: ConnectTimeouts {
            connect: Duration::from_secs(5),
            auth: Duration::from_secs(5),
            host_key_decision: Duration::from_secs(10),
            ..ConnectTimeouts::default()
        },
        known_hosts_path: server.known_hosts.clone(),
        extra_known_hosts: Vec::new(),
        host_key_prompt: Arc::new(termru_ssh::config::RejectUnknownHostKeys),
        auth_prompt: Arc::new(termru_ssh::NoAuthPrompts),
        setup: Default::default(),
    };

    let (channel, mut updates) = channel_to_sender();
    let (id, cancel) = manager.register(
        termru_core::ids::HostId::new(),
        params.clone(),
        "main",
        channel,
    );
    spawn_connection(
        manager.clone(),
        store,
        id.clone(),
        params,
        SessionOptions::default(),
        cancel,
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

/// The server's login banner is plain terminal output at the start of the
/// session: before `Ready`, line by line, with the colour codes of the
/// server's text gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn app_session_prints_the_server_banner_before_the_shell() {
    use base64::Engine;

    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };
    assert!(
        support::BANNER.contains('\x1b'),
        "the fixture must have escapes"
    );

    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());
    let params = connect_params(&server, Arc::new(termru_ssh::config::RejectUnknownHostKeys));
    let (channel, mut updates) = channel_to_sender();
    let id = start_session(&manager, &store, params, channel);

    let mut before_ready = Vec::new();
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        let update = tokio::time::timeout_at(deadline, updates.recv())
            .await
            .expect("the session never became ready")
            .expect("the update channel closed");
        match update {
            SessionUpdate::Data { base64 } => before_ready.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(base64)
                    .expect("valid base64"),
            ),
            SessionUpdate::Status {
                status: SessionStatusDto::Ready,
            } => break,
            _ => {}
        }
    }
    assert_eq!(
        String::from_utf8_lossy(&before_ready),
        "Termru e2e banner\r\nsecond line\r\n"
    );

    manager.close(&id).await.expect("close session");
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
        auth: vec![AuthChoice::PrivateKey {
            key_path: server.unauthorized_key.clone(),
            passphrase: None,
            identity: None,
        }],
        term: "xterm-256color".into(),
        keepalive_seconds: 0,
        timeouts: ConnectTimeouts {
            connect: Duration::from_secs(5),
            auth: Duration::from_secs(5),
            host_key_decision: Duration::from_secs(5),
            ..ConnectTimeouts::default()
        },
        known_hosts_path: server.known_hosts.clone(),
        extra_known_hosts: Vec::new(),
        host_key_prompt: Arc::new(termru_ssh::config::RejectUnknownHostKeys),
        auth_prompt: Arc::new(termru_ssh::NoAuthPrompts),
        setup: Default::default(),
    };

    let (channel, mut updates) = channel_to_sender();
    let (id, cancel) = manager.register(
        termru_core::ids::HostId::new(),
        params.clone(),
        "main",
        channel,
    );
    spawn_connection(
        manager.clone(),
        store,
        id.clone(),
        params,
        SessionOptions::default(),
        cancel,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_view_attaches_and_receives_replay_and_live_output() {
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
        auth: vec![AuthChoice::PrivateKey {
            key_path: server.client_key.clone(),
            passphrase: None,
            identity: None,
        }],
        term: "xterm-256color".into(),
        keepalive_seconds: 0,
        timeouts: ConnectTimeouts {
            connect: Duration::from_secs(5),
            auth: Duration::from_secs(5),
            host_key_decision: Duration::from_secs(10),
            ..ConnectTimeouts::default()
        },
        known_hosts_path: server.known_hosts.clone(),
        extra_known_hosts: Vec::new(),
        host_key_prompt: Arc::new(termru_ssh::config::RejectUnknownHostKeys),
        auth_prompt: Arc::new(termru_ssh::NoAuthPrompts),
        setup: Default::default(),
    };

    let (channel, mut updates) = channel_to_sender();
    let (id, cancel) = manager.register(
        termru_core::ids::HostId::new(),
        params.clone(),
        "main",
        channel,
    );
    spawn_connection(
        manager.clone(),
        store,
        id.clone(),
        params,
        SessionOptions::default(),
        cancel,
    );

    assert!(
        wait_for_status(&mut updates, |status| matches!(
            status,
            SessionStatusDto::Ready
        ))
        .await,
        "session never became ready"
    );

    manager
        .write(
            &id,
            b"echo first-view-marker
"
            .to_vec(),
        )
        .await
        .expect("write to session");
    assert!(
        wait_for_data(&mut updates, b"first-view-marker").await,
        "first view never saw its own output"
    );

    // A second view attaches (a tab moved to another window): it must
    // receive the replay buffer, then the live stream.
    let (second_channel, mut second_updates) = channel_to_sender();
    manager
        .attach_view(&id, "main", second_channel)
        .await
        .expect("attach second view");
    assert_eq!(manager.view_count(&id), 2);

    assert!(
        wait_for_data(&mut second_updates, b"first-view-marker").await,
        "second view did not receive the replay buffer"
    );

    manager
        .write(
            &id,
            b"echo second-view-marker
"
            .to_vec(),
        )
        .await
        .expect("write to session");
    assert!(
        wait_for_data(&mut second_updates, b"second-view-marker").await,
        "second view did not receive live output"
    );
    assert!(
        wait_for_data(&mut updates, b"second-view-marker").await,
        "first view missed the live output"
    );

    manager.close(&id).await.expect("close session");
    assert!(manager.info(&id).is_none());
}

fn connect_params(
    server: &support::TestServer,
    prompt: Arc<dyn termru_ssh::HostKeyPrompt>,
) -> ConnectParams {
    ConnectParams {
        host: "127.0.0.1".into(),
        port: server.port,
        username: support::current_user(),
        auth: vec![AuthChoice::PrivateKey {
            key_path: server.client_key.clone(),
            passphrase: None,
            identity: None,
        }],
        term: "xterm-256color".into(),
        keepalive_seconds: 0,
        timeouts: ConnectTimeouts {
            connect: Duration::from_secs(5),
            auth: Duration::from_secs(5),
            host_key_decision: Duration::from_secs(10),
            ..ConnectTimeouts::default()
        },
        known_hosts_path: server.known_hosts.clone(),
        extra_known_hosts: Vec::new(),
        host_key_prompt: prompt,
        auth_prompt: Arc::new(termru_ssh::NoAuthPrompts),
        setup: Default::default(),
    }
}

/// Registers a session with `channel` as its first view and starts it.
fn start_session(
    manager: &Arc<SessionManager>,
    store: &Arc<termru_store::Store>,
    params: ConnectParams,
    channel: Channel<SessionUpdate>,
) -> String {
    let (id, cancel) = manager.register(
        termru_core::ids::HostId::new(),
        params.clone(),
        "main",
        channel,
    );
    spawn_connection(
        manager.clone(),
        store.clone(),
        id.clone(),
        params,
        SessionOptions::default(),
        cancel,
    );
    id
}

/// A view whose window can "disappear": once `alive` is cleared, every send
/// fails like it does for a closed webview.
fn switchable_channel() -> (
    Channel<SessionUpdate>,
    Arc<AtomicBool>,
    mpsc::UnboundedReceiver<SessionUpdate>,
) {
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    let (tx, rx) = mpsc::unbounded_channel();
    let channel = Channel::new(move |body| {
        if !flag.load(Ordering::SeqCst) {
            return Err(tauri::Error::WebviewNotFound);
        }
        if let InvokeResponseBody::Json(value) = body {
            if let Ok(update) = serde_json::from_str::<SessionUpdate>(&value) {
                let _ = tx.send(update);
            }
        }
        Ok(())
    });
    (channel, alive, rx)
}

/// Regression test (session dies silently when the view is briefly gone): the
/// engine used to drop its consumer after a 5 s stalled send while the pump
/// waited up to 10 s for a view, and the session ended without any `Closed`.
/// With no view for 6.5 s (inside the 10 s grace) the session must keep
/// running, and a view that attaches then gets the scrollback produced in the
/// meantime plus the live stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_survives_a_view_that_is_briefly_gone() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };
    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());
    let (channel, alive, mut first_updates) = switchable_channel();
    let id = start_session(
        &manager,
        &store,
        connect_params(&server, Arc::new(termru_ssh::config::RejectUnknownHostKeys)),
        channel,
    );
    assert!(
        wait_for_status(&mut first_updates, |s| matches!(s, SessionStatusDto::Ready)).await,
        "session never became ready"
    );

    manager
        .write(
            &id,
            b"while true; do echo tick-$((1+1)); sleep 0.2; done\n".to_vec(),
        )
        .await
        .expect("write");
    assert!(wait_for_data(&mut first_updates, b"tick-2").await);

    // The window goes away; output keeps flowing for longer than the old
    // 5 s consumer timeout, shorter than the 10 s view grace.
    alive.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(6500)).await;
    assert!(
        matches!(manager.status(&id), Some(SessionStatusDto::Ready)),
        "the session must still be running: {:?}",
        manager.status(&id)
    );

    let (second, mut second_updates) = channel_to_sender();
    manager
        .attach_view(&id, "main", second)
        .await
        .expect("attach a new view");
    assert!(
        wait_for_data(&mut second_updates, b"tick-2").await,
        "the new view gets the scrollback and the live stream"
    );
    manager
        .write(&id, b"\x03echo ALIVE_$((1+1))\n".to_vec())
        .await
        .expect("write after the gap");
    assert!(
        wait_for_data(&mut second_updates, b"ALIVE_2").await,
        "the session still answers"
    );
    manager.close(&id).await.expect("close");
}

/// Regression test: a shell killed by a signal used to be classified as a
/// network failure, and the app reconnected in a loop. It must end with one
/// `Closed` and nothing after it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killed_shell_closes_without_reconnect_attempts() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };
    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());
    let (channel, mut updates) = channel_to_sender();
    let id = start_session(
        &manager,
        &store,
        connect_params(&server, Arc::new(termru_ssh::config::RejectUnknownHostKeys)),
        channel,
    );
    assert!(
        wait_for_status(&mut updates, |s| matches!(s, SessionStatusDto::Ready)).await,
        "session never became ready"
    );

    manager
        .write(&id, b"kill -9 $$\n".to_vec())
        .await
        .expect("write");

    // Everything the UI is told, until a quiet period after `Closed`.
    let mut statuses: Vec<SessionStatusDto> = Vec::new();
    let mut closed_at: Option<tokio::time::Instant> = None;
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        let wait = match closed_at {
            Some(at) => {
                (at + Duration::from_secs(3)).saturating_duration_since(tokio::time::Instant::now())
            }
            None => Duration::from_secs(1),
        };
        if closed_at.is_some() && wait.is_zero() {
            break;
        }
        match tokio::time::timeout(wait, updates.recv()).await {
            Ok(Some(SessionUpdate::Status { status })) => {
                if matches!(status, SessionStatusDto::Closed { .. }) && closed_at.is_none() {
                    closed_at = Some(tokio::time::Instant::now());
                }
                statuses.push(status);
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                if closed_at.is_some() {
                    break;
                }
            }
        }
    }
    let closed = statuses
        .iter()
        .find_map(|s| match s {
            SessionStatusDto::Closed { reason } => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Closed status; saw {statuses:?}"));
    assert!(closed.contains("signal"), "readable reason: {closed}");
    assert!(
        !statuses.iter().any(|s| matches!(
            s,
            SessionStatusDto::Failed { .. } | SessionStatusDto::Reconnecting { .. }
        )),
        "no failure or reconnect for a killed shell: {statuses:?}"
    );
    // The initial `Connecting` was consumed while waiting for `Ready`: any
    // `Connecting` seen now would be a second connection attempt.
    assert!(
        !statuses
            .iter()
            .any(|s| matches!(s, SessionStatusDto::Connecting)),
        "no second connection attempt: {statuses:?}"
    );
    assert!(matches!(
        manager.status(&id),
        Some(SessionStatusDto::Closed { .. })
    ));
    manager.close(&id).await.expect("close");
}

/// A host-key prompt that waits until the test lets it through, so a session
/// can be held in the middle of connecting.
struct GatedPrompt {
    gate: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl termru_ssh::HostKeyPrompt for GatedPrompt {
    async fn decide(&self, _request: termru_ssh::HostKeyPromptRequest) -> bool {
        self.gate.notified().await;
        true
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

/// Regression test (geometry): a resize while the session is still connecting
/// used to fail with "not ready" and be dropped, leaving the PTY at the size
/// from the open call. It is stored and applied when the session is ready.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resize_before_ready_is_applied_when_the_session_becomes_ready() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };
    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());
    let prompt = Arc::new(GatedPrompt {
        gate: tokio::sync::Notify::new(),
    });
    let (channel, mut updates) = channel_to_sender();
    // An empty known_hosts: the host key is unknown, so the connect parks at
    // the prompt until the test lets it through.
    let mut params = connect_params(&server, prompt.clone());
    params.known_hosts_path = _dir.path().join("known_hosts_empty");
    let id = start_session(&manager, &store, params, channel);

    // Let the connect reach the (unanswered) host-key prompt: the PTY size
    // of this attempt is fixed at 80x24 by now.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !matches!(manager.status(&id), Some(SessionStatusDto::Ready)),
        "still connecting"
    );
    manager
        .resize(&id, 132, 43)
        .expect("a resize while connecting is stored, not an error");
    prompt.gate.notify_one();

    assert!(
        wait_for_status(&mut updates, |s| matches!(s, SessionStatusDto::Ready)).await,
        "session never became ready"
    );
    manager
        .write(&id, b"stty size\n".to_vec())
        .await
        .expect("write");
    assert!(
        wait_for_data(&mut updates, b"43 132").await,
        "the stored size was not applied on ready"
    );
    manager.close(&id).await.expect("close");
}

/// Regression test (keystroke ordering): 1000 writes enqueued through the
/// synchronous path reach the shell in call order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thousand_ordered_writes_arrive_in_order() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };
    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());
    let (channel, mut updates) = channel_to_sender();
    let id = start_session(
        &manager,
        &store,
        connect_params(&server, Arc::new(termru_ssh::config::RejectUnknownHostKeys)),
        channel,
    );
    assert!(
        wait_for_status(&mut updates, |s| matches!(s, SessionStatusDto::Ready)).await,
        "session never became ready"
    );

    manager.try_write(&id, b"cat\n".to_vec()).expect("write");
    for i in 1..=1000u32 {
        manager
            .try_write(&id, format!("ORD{i:04}\n").into_bytes())
            .expect("enqueue");
    }
    assert!(wait_for_data(&mut updates, b"ORD1000").await);
    manager.close(&id).await.expect("close");
}

/// Regression test (moving a tab to a new window killed the session): the
/// frontend detaches the old window right after the new window was *created*,
/// i.e. before its webview attached. The session must survive being shown
/// nowhere, and a view that attaches within the 10 s grace period must keep it
/// alive for good — the grace period may not fire on it later.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tab_moved_to_a_new_window_survives_the_gap_before_it_attaches() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };
    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());
    let (channel, mut updates) = channel_to_sender();
    let id = start_session(
        &manager,
        &store,
        connect_params(&server, Arc::new(termru_ssh::config::RejectUnknownHostKeys)),
        channel,
    );
    assert!(
        wait_for_status(&mut updates, |s| matches!(s, SessionStatusDto::Ready)).await,
        "session never became ready"
    );

    // The old window lets go before the new one attached.
    manager.detach_window_views(&id, "main").expect("detach");
    assert_eq!(manager.view_count(&id), 0);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        matches!(manager.status(&id), Some(SessionStatusDto::Ready)),
        "the detach must not close the session: {:?}",
        manager.status(&id)
    );

    // The new window's webview has loaded.
    let (second, mut second_updates) = channel_to_sender();
    manager
        .attach_view(&id, "second", second)
        .await
        .expect("attach the new window's view");

    // Well past the 10 s grace period that started at the detach: the view
    // that attached in time cancelled it.
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(
        matches!(manager.status(&id), Some(SessionStatusDto::Ready)),
        "the session must still be running: {:?}",
        manager.status(&id)
    );
    manager
        .write(&id, b"echo MOVED_$((1+1))\n".to_vec())
        .await
        .expect("write after the move");
    assert!(
        wait_for_data(&mut second_updates, b"MOVED_2").await,
        "the moved tab still answers"
    );
    manager.close(&id).await.expect("close");
}

/// Regression test (an idle session shown nowhere never ended): the pump only
/// looked at its views when an event woke it, so a session without output
/// that lost its last view kept its SSH connection open forever. It must be
/// closed once the 10 s grace period is over — and not before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_idle_session_shown_nowhere_is_closed_after_the_grace_period() {
    let Some(server) = support::TestServer::start() else {
        eprintln!("SKIPPED: OpenSSH tooling unavailable — app-layer E2E not exercised");
        return;
    };
    let (_dir, store) = temp_store();
    let manager = Arc::new(SessionManager::new());
    let (channel, mut updates) = channel_to_sender();
    let id = start_session(
        &manager,
        &store,
        connect_params(&server, Arc::new(termru_ssh::config::RejectUnknownHostKeys)),
        channel,
    );
    assert!(
        wait_for_status(&mut updates, |s| matches!(s, SessionStatusDto::Ready)).await,
        "session never became ready"
    );
    // Let the shell finish printing its banner: from here on nothing arrives.
    manager
        .write(&id, b"echo SETTLED\n".to_vec())
        .await
        .expect("write");
    assert!(wait_for_data(&mut updates, b"SETTLED").await);
    tokio::time::sleep(Duration::from_millis(500)).await;

    manager.detach_window_views(&id, "main").expect("detach");
    let detached = tokio::time::Instant::now();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(manager.info(&id).is_some(), "still inside the grace period");

    // Gone shortly after the grace period (10 s) is over.
    let deadline = detached + Duration::from_secs(15);
    while manager.info(&id).is_some() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        manager.info(&id).is_none(),
        "an idle, orphaned session was never closed"
    );
    assert!(
        detached.elapsed() >= Duration::from_secs(9),
        "closed before the grace period was over: {:?}",
        detached.elapsed()
    );
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
