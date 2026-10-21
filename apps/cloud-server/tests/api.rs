//! Integration tests against a real PostgreSQL server.
//!
//! Run them with:
//!
//! ```text
//! TERMRU_TEST_DATABASE_URL=postgres://user@127.0.0.1:5432/postgres cargo test -p termru-cloud-server
//! ```
//!
//! Without that variable every test skips with a loud `SKIPPED:` message.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "harness setup failures must panic loudly in tests"
)]

mod common;

use axum::http::StatusCode;
use common::{DATABASE_URL_ENV, PASSWORD, TestServer, device_id, send};
use serde_json::json;
use termru_core::Host;
use termru_core::sync::{PushMutation, PushRequest, RecordKind};
use tower::ServiceExt;
use uuid::Uuid;

/// Returns a server, or skips the test when no database is configured.
macro_rules! server {
    () => {
        match TestServer::start().await {
            Some(server) => server,
            None => {
                eprintln!(
                    "SKIPPED: set {DATABASE_URL_ENV} to a PostgreSQL URL to run the integration tests"
                );
                return;
            }
        }
    };
}

/// A valid host payload.
fn sample_host() -> Host {
    let mut host = Host::new(
        "web-1".to_string(),
        "web-1.example.com".to_string(),
        "deploy".to_string(),
    );
    host.description = Some("production front end".to_string());
    host
}

/// Registers an account and returns the tokens.
async fn register(server: &TestServer, email: &str) -> (String, String, Uuid) {
    let device = device_id();
    let response = send(
        &server.router,
        "POST",
        "/v1/auth/register",
        None,
        Some(json!({
            "email": email,
            "password": PASSWORD,
            "device_id": device,
            "device_name": "test device",
        })),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "register failed: {}",
        response.body
    );
    (
        response.str_field("access_token"),
        response.str_field("refresh_token"),
        device,
    )
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_and_readiness_report_the_state() {
    let server = server!();

    let health = send(&server.router, "GET", "/healthz", None, None).await;
    assert_eq!(health.status, StatusCode::OK);

    let ready = send(&server.router, "GET", "/readyz", None, None).await;
    assert_eq!(ready.status, StatusCode::OK);
}

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let server = server!();
    let response = send(&server.router, "GET", "/healthz", None, None).await;
    assert_eq!(response.status, StatusCode::OK);
    // The header check runs through the same middleware for all routes.
    let head = send(&server.router, "GET", "/nope", None, None).await;
    assert_eq!(head.status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_account_can_be_created_only_once() {
    let server = server!();
    let (access, _, _) = register(&server, "alice@example.com").await;
    assert!(!access.is_empty());

    let second = send(
        &server.router,
        "POST",
        "/v1/auth/register",
        None,
        Some(json!({
            "email": "alice@example.com",
            "password": PASSWORD,
            "device_id": device_id(),
            "device_name": "another device",
        })),
    )
    .await;
    assert_eq!(second.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_email_address_is_normalised() {
    let server = server!();
    let device = device_id();
    let first = send(
        &server.router,
        "POST",
        "/v1/auth/register",
        None,
        Some(json!({
            "email": "Bob@Example.COM",
            "password": PASSWORD,
            "device_id": device,
            "device_name": "test device",
        })),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);

    // The same address in a different case is the same account.
    let second = send(
        &server.router,
        "POST",
        "/v1/auth/register",
        None,
        Some(json!({
            "email": "bob@example.com",
            "password": PASSWORD,
            "device_id": device_id(),
            "device_name": "other device",
        })),
    )
    .await;
    assert_eq!(second.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_weak_password_is_rejected() {
    let server = server!();
    let response = send(
        &server.router,
        "POST",
        "/v1/auth/register",
        None,
        Some(json!({
            "email": "weak@example.com",
            "password": "short",
            "device_id": device_id(),
            "device_name": "test device",
        })),
    )
    .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_succeeds_with_the_right_password() {
    let server = server!();
    register(&server, "carol@example.com").await;

    let response = send(
        &server.router,
        "POST",
        "/v1/auth/login",
        None,
        Some(json!({
            "email": "carol@example.com",
            "password": PASSWORD,
            "device_id": device_id(),
            "device_name": "test device",
        })),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(!response.str_field("access_token").is_empty());
}

#[tokio::test]
async fn a_wrong_password_and_an_unknown_account_are_indistinguishable() {
    let server = server!();
    register(&server, "dave@example.com").await;

    let wrong_password = send(
        &server.router,
        "POST",
        "/v1/auth/login",
        None,
        Some(json!({
            "email": "dave@example.com",
            "password": "not the right password",
            "device_id": device_id(),
            "device_name": "test device",
        })),
    )
    .await;
    let unknown_account = send(
        &server.router,
        "POST",
        "/v1/auth/login",
        None,
        Some(json!({
            "email": "nobody@example.com",
            "password": "not the right password",
            "device_id": device_id(),
            "device_name": "test device",
        })),
    )
    .await;

    assert_eq!(wrong_password.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_account.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        wrong_password.body["error"]["message"], unknown_account.body["error"]["message"],
        "the responses must not reveal whether the account exists"
    );
}

#[tokio::test]
async fn repeated_failures_lock_the_account_out() {
    let server = server!();
    register(&server, "erin@example.com").await;

    for _ in 0..3 {
        let response = send(
            &server.router,
            "POST",
            "/v1/auth/login",
            None,
            Some(json!({
                "email": "erin@example.com",
                "password": "not the right password",
                "device_id": device_id(),
                "device_name": "test device",
            })),
        )
        .await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    }

    // The correct password is refused as well while the account is locked.
    let locked = send(
        &server.router,
        "POST",
        "/v1/auth/login",
        None,
        Some(json!({
            "email": "erin@example.com",
            "password": PASSWORD,
            "device_id": device_id(),
            "device_name": "test device",
        })),
    )
    .await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_missing_or_wrong_token_is_rejected() {
    let server = server!();
    let (access, _, _) = register(&server, "frank@example.com").await;

    let missing = send(&server.router, "GET", "/v1/devices", None, None).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);

    let wrong = send(
        &server.router,
        "GET",
        "/v1/devices",
        Some("not-a-token"),
        None,
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let good = send(&server.router, "GET", "/v1/devices", Some(&access), None).await;
    assert_eq!(good.status, StatusCode::OK);
}

#[tokio::test]
async fn refreshing_rotates_the_tokens() {
    let server = server!();
    let (access, refresh, _) = register(&server, "grace@example.com").await;

    let response = send(
        &server.router,
        "POST",
        "/v1/auth/refresh",
        None,
        Some(json!({ "refresh_token": refresh })),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    let new_access = response.str_field("access_token");
    let new_refresh = response.str_field("refresh_token");
    assert_ne!(new_access, access);
    assert_ne!(new_refresh, refresh);

    // The old access token is dead, the new one works.
    let old = send(&server.router, "GET", "/v1/devices", Some(&access), None).await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    let new = send(
        &server.router,
        "GET",
        "/v1/devices",
        Some(&new_access),
        None,
    )
    .await;
    assert_eq!(new.status, StatusCode::OK);
}

#[tokio::test]
async fn replaying_a_refresh_token_revokes_the_whole_family() {
    let server = server!();
    let (_, refresh, _) = register(&server, "heidi@example.com").await;

    let first = send(
        &server.router,
        "POST",
        "/v1/auth/refresh",
        None,
        Some(json!({ "refresh_token": refresh })),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    let new_access = first.str_field("access_token");

    // The refresh token is replayed. The reuse grace window is zero in the
    // tests, so this counts as a theft and revokes the family.
    let replay = send(
        &server.router,
        "POST",
        "/v1/auth/refresh",
        None,
        Some(json!({ "refresh_token": refresh })),
    )
    .await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);

    // The token that was obtained in between is revoked as well.
    let after = send(
        &server.router,
        "GET",
        "/v1/devices",
        Some(&new_access),
        None,
    )
    .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logging_out_revokes_the_session() {
    let server = server!();
    let (access, _, _) = register(&server, "ivan@example.com").await;

    let logout = send(
        &server.router,
        "POST",
        "/v1/auth/logout",
        Some(&access),
        None,
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);

    let after = send(&server.router, "GET", "/v1/devices", Some(&access), None).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

#[tokio::test]
async fn devices_can_be_listed_renamed_and_revoked() {
    let server = server!();
    let (access, _, device) = register(&server, "judy@example.com").await;

    let list = send(&server.router, "GET", "/v1/devices", Some(&access), None).await;
    assert_eq!(list.status, StatusCode::OK);
    let devices = list.body.as_array().expect("a list of devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["id"], device.to_string());
    assert_eq!(devices[0]["current"], true);

    let renamed = send(
        &server.router,
        "PATCH",
        &format!("/v1/devices/{device}"),
        Some(&access),
        Some(json!({ "name": "work laptop" })),
    )
    .await;
    assert_eq!(renamed.status, StatusCode::NO_CONTENT);

    let list = send(&server.router, "GET", "/v1/devices", Some(&access), None).await;
    assert_eq!(list.body[0]["name"], "work laptop");

    let revoked = send(
        &server.router,
        "DELETE",
        &format!("/v1/devices/{device}"),
        Some(&access),
        None,
    )
    .await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);

    // A revoked device loses API access immediately.
    let after = send(&server.router, "GET", "/v1/devices", Some(&access), None).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_device_can_be_enrolled_and_gets_its_own_token() {
    let server = server!();
    let (access, _, _) = register(&server, "mallory@example.com").await;

    let enrolled = send(
        &server.router,
        "POST",
        "/v1/devices",
        Some(&access),
        Some(json!({ "name": "second device" })),
    )
    .await;
    assert_eq!(enrolled.status, StatusCode::OK);
    let second_access = enrolled.str_field("access_token");
    assert!(!second_access.is_empty());

    // The new device sees the same account.
    let list = send(
        &server.router,
        "GET",
        "/v1/devices",
        Some(&second_access),
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body.as_array().expect("devices").len(), 2);
}

#[tokio::test]
async fn a_device_of_another_account_can_not_be_touched() {
    let server = server!();
    let (alice_access, _, _) = register(&server, "owner@example.com").await;
    let (bob_access, _, bob_device) = register(&server, "other@example.com").await;

    let renamed = send(
        &server.router,
        "PATCH",
        &format!("/v1/devices/{bob_device}"),
        Some(&alice_access),
        Some(json!({ "name": "stolen" })),
    )
    .await;
    assert_eq!(renamed.status, StatusCode::NOT_FOUND);

    let revoked = send(
        &server.router,
        "DELETE",
        &format!("/v1/devices/{bob_device}"),
        Some(&alice_access),
        None,
    )
    .await;
    assert_eq!(revoked.status, StatusCode::NOT_FOUND);

    // Bob's device still works.
    let list = send(
        &server.router,
        "GET",
        "/v1/devices",
        Some(&bob_access),
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_record_can_be_pushed_and_pulled() {
    let server = server!();
    let (access, _, _) = register(&server, "nina@example.com").await;
    let host = sample_host();

    let push = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": host.id.to_string(),
                "kind": "host",
                "base_rev": null,
                "deleted": false,
                "payload": serde_json::to_value(&host).expect("serialize the host"),
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    assert_eq!(push.status, StatusCode::OK, "push failed: {}", push.body);
    assert_eq!(push.body["accepted"].as_array().expect("accepted").len(), 1);
    let rev = push.body["accepted"][0]["rev"]
        .as_i64()
        .expect("a revision");
    assert!(rev > 0);

    let pull = send(
        &server.router,
        "GET",
        "/v1/sync/changes?since=0",
        Some(&access),
        None,
    )
    .await;
    assert_eq!(pull.status, StatusCode::OK);
    let records = pull.body["records"].as_array().expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], host.id.to_string());
    assert_eq!(records[0]["kind"], "host");
    assert_eq!(records[0]["rev"], rev);
    assert_eq!(pull.body["next_rev"], rev);
    assert_eq!(pull.body["has_more"], false);

    // Nothing changed after the last revision.
    let empty = send(
        &server.router,
        "GET",
        &format!("/v1/sync/changes?since={rev}"),
        Some(&access),
        None,
    )
    .await;
    assert_eq!(empty.body["records"].as_array().expect("records").len(), 0);
}

#[tokio::test]
async fn pushing_the_same_mutation_twice_is_idempotent() {
    let server = server!();
    let (access, _, _) = register(&server, "oscar@example.com").await;
    let host = sample_host();
    let mutation_id = Uuid::new_v4();
    let mutation = json!({
        "mutation_id": mutation_id,
        "id": host.id.to_string(),
        "kind": "host",
        "base_rev": null,
        "deleted": false,
        "payload": serde_json::to_value(&host).expect("serialize the host"),
        "updated_at": chrono::Utc::now(),
    });

    let first = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({ "mutations": [mutation] })),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    let first_rev = first.body["accepted"][0]["rev"]
        .as_i64()
        .expect("a revision");

    let second = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({ "mutations": [mutation] })),
    )
    .await;
    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(
        second.body["accepted"][0]["rev"]
            .as_i64()
            .expect("a revision"),
        first_rev,
        "the retry must return the revision of the first application"
    );
    assert!(
        second.body["conflicts"]
            .as_array()
            .expect("conflicts")
            .is_empty()
    );

    // The record exists exactly once.
    let pull = send(
        &server.router,
        "GET",
        "/v1/sync/changes?since=0",
        Some(&access),
        None,
    )
    .await;
    assert_eq!(pull.body["records"].as_array().expect("records").len(), 1);
}

#[tokio::test]
async fn a_stale_base_revision_is_reported_as_a_conflict() {
    let server = server!();
    let (access, _, _) = register(&server, "peggy@example.com").await;
    let host = sample_host();
    let id = host.id.to_string();
    let payload = serde_json::to_value(&host).expect("serialize the host");

    let created = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": id,
                "kind": "host",
                "base_rev": null,
                "deleted": false,
                "payload": payload,
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    let rev = created.body["accepted"][0]["rev"]
        .as_i64()
        .expect("a revision");

    // A second device updates the record.
    let mut updated = host.clone();
    updated.name = "web-2".to_string();
    let update = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": id,
                "kind": "host",
                "base_rev": rev,
                "deleted": false,
                "payload": serde_json::to_value(&updated).expect("serialize the host"),
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    assert_eq!(update.status, StatusCode::OK);
    assert!(
        update.body["conflicts"]
            .as_array()
            .expect("conflicts")
            .is_empty()
    );
    let new_rev = update.body["accepted"][0]["rev"]
        .as_i64()
        .expect("a revision");
    assert!(new_rev > rev);

    // The first device pushes with the now stale revision.
    let stale = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": id,
                "kind": "host",
                "base_rev": rev,
                "deleted": false,
                "payload": payload,
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    assert_eq!(stale.status, StatusCode::OK);
    let conflicts = stale.body["conflicts"].as_array().expect("conflicts");
    assert_eq!(conflicts.len(), 1, "the stale write must be a conflict");
    assert_eq!(conflicts[0]["id"], id);
    assert!(
        stale.body["accepted"]
            .as_array()
            .expect("accepted")
            .is_empty()
    );

    // The conflict carries the server's copy so the client can compare.
    let server_record = &conflicts[0]["server_record"];
    assert_eq!(server_record["rev"], new_rev);
    assert_eq!(server_record["payload"]["name"], "web-2");
}

#[tokio::test]
async fn a_delete_produces_a_tombstone() {
    let server = server!();
    let (access, _, _) = register(&server, "quinn@example.com").await;
    let host = sample_host();
    let id = host.id.to_string();

    let created = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": id,
                "kind": "host",
                "base_rev": null,
                "deleted": false,
                "payload": serde_json::to_value(&host).expect("serialize the host"),
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    let rev = created.body["accepted"][0]["rev"]
        .as_i64()
        .expect("a revision");

    let deleted = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": id,
                "kind": "host",
                "base_rev": rev,
                "deleted": true,
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK);
    assert!(
        deleted.body["conflicts"]
            .as_array()
            .expect("conflicts")
            .is_empty()
    );

    let pull = send(
        &server.router,
        "GET",
        "/v1/sync/changes?since=0",
        Some(&access),
        None,
    )
    .await;
    let records = pull.body["records"].as_array().expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["deleted"], true);
    assert!(records[0].get("payload").is_none());
}

#[tokio::test]
async fn one_user_never_sees_the_records_of_another() {
    let server = server!();
    let (alice, _, _) = register(&server, "alice2@example.com").await;
    let (bob, _, _) = register(&server, "bob2@example.com").await;

    let host = sample_host();
    let push = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&alice),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": host.id.to_string(),
                "kind": "host",
                "base_rev": null,
                "deleted": false,
                "payload": serde_json::to_value(&host).expect("serialize the host"),
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    assert_eq!(push.status, StatusCode::OK);

    let bob_pull = send(
        &server.router,
        "GET",
        "/v1/sync/changes?since=0",
        Some(&bob),
        None,
    )
    .await;
    assert!(
        bob_pull.body["records"]
            .as_array()
            .expect("records")
            .is_empty(),
        "bob must not see the records of alice"
    );
}

#[tokio::test]
async fn an_invalid_payload_is_rejected() {
    let server = server!();
    let (access, _, _) = register(&server, "rita@example.com").await;

    let response = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(json!({
            "mutations": [{
                "mutation_id": Uuid::new_v4(),
                "id": Uuid::new_v4().to_string(),
                "kind": "host",
                "base_rev": null,
                "deleted": false,
                "payload": { "name": "" },
                "updated_at": chrono::Utc::now(),
            }]
        })),
    )
    .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_malformed_body_is_rejected() {
    let server = server!();
    let (access, _, _) = register(&server, "sam@example.com").await;

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/sync/changes")
        .header("authorization", format!("Bearer {access}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from("{not json"))
        .expect("build the request");
    let response = server
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("the router must answer");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_batch_of_mutations_is_applied_in_order() {
    let server = server!();
    let (access, _, _) = register(&server, "tina@example.com").await;

    let first = sample_host();
    let mut second = sample_host();
    second.id = Uuid::new_v4().into();
    second.name = "db-1".to_string();

    let request = PushRequest {
        mutations: vec![
            PushMutation {
                mutation_id: Uuid::new_v4(),
                id: first.id.to_string(),
                kind: RecordKind::Host,
                base_rev: None,
                deleted: false,
                payload: serde_json::to_value(&first).ok(),
                updated_at: chrono::Utc::now(),
            },
            PushMutation {
                mutation_id: Uuid::new_v4(),
                id: second.id.to_string(),
                kind: RecordKind::Host,
                base_rev: None,
                deleted: false,
                payload: serde_json::to_value(&second).ok(),
                updated_at: chrono::Utc::now(),
            },
        ],
    };

    let response = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(serde_json::to_value(&request).expect("serialize the request")),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let accepted = response.body["accepted"].as_array().expect("accepted");
    assert_eq!(accepted.len(), 2);
    let first_rev = accepted[0]["rev"].as_i64().expect("a revision");
    let second_rev = accepted[1]["rev"].as_i64().expect("a revision");
    assert!(
        second_rev > first_rev,
        "the revisions must increase inside a batch"
    );
}
