//! Operational tests: migrations, limits, backups.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "harness setup failures must panic loudly in tests"
)]

mod common;

use axum::http::StatusCode;
use common::{
    DATABASE_URL_ENV, TestServer, create_database, database_url, drop_database, pool_for, send,
};
use serde_json::json;
use std::process::Command;
use termru_cloud_server::db;
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

#[tokio::test]
async fn migrations_are_idempotent() {
    let server = server!();

    // The server already migrated the database; running the migrations again
    // must not fail and must not change the schema version.
    db::run_migrations(&server.pool)
        .await
        .expect("re-running the migrations");
    db::run_migrations(&server.pool)
        .await
        .expect("re-running the migrations");

    let client = server.pool.get().await.expect("a connection");
    let row = client
        .query_one("SELECT count(*) AS n FROM schema_migrations", &[])
        .await
        .expect("the migration table");
    let count: i64 = row.get("n");
    assert!(count >= 1, "the migrations must be recorded");
}

#[tokio::test]
async fn the_rate_limiter_stops_a_flood() {
    let Some(server) = TestServer::start_with(Some(5)).await else {
        eprintln!(
            "SKIPPED: set {DATABASE_URL_ENV} to a PostgreSQL URL to run the integration tests"
        );
        return;
    };

    let mut limited = false;
    for _ in 0..10 {
        let response = send(&server.router, "GET", "/healthz", None, None).await;
        if response.status == StatusCode::TOO_MANY_REQUESTS {
            limited = true;
            break;
        }
    }
    assert!(limited, "the rate limiter must reject the flood");
}

#[tokio::test]
async fn an_oversized_body_is_rejected() {
    let server = server!();
    let (access, _, _) = {
        let response = send(
            &server.router,
            "POST",
            "/v1/auth/register",
            None,
            Some(json!({
                "email": "big@example.com",
                "password": "correct horse battery staple",
                "device_id": Uuid::new_v4(),
                "device_name": "test device",
            })),
        )
        .await;
        assert_eq!(response.status, StatusCode::OK);
        (
            response.str_field("access_token"),
            response.str_field("refresh_token"),
            Uuid::nil(),
        )
    };

    // The test server accepts bodies up to 1 MiB.
    let huge = json!({ "mutations": [], "padding": "x".repeat(2 * 1024 * 1024) });
    let response = send(
        &server.router,
        "POST",
        "/v1/sync/changes",
        Some(&access),
        Some(huge),
    )
    .await;
    assert_eq!(response.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_backup_can_be_restored() {
    let Some(server) = TestServer::start().await else {
        eprintln!(
            "SKIPPED: set {DATABASE_URL_ENV} to a PostgreSQL URL to run the integration tests"
        );
        return;
    };

    if Command::new("pg_dump").arg("--version").output().is_err() {
        eprintln!("SKIPPED: pg_dump is not installed");
        return;
    }

    // Put a row into the database.
    let client = server.pool.get().await.expect("a connection");
    client
        .execute(
            "INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)",
            &[&Uuid::new_v4(), &"backup@example.com", &"hash"],
        )
        .await
        .expect("insert a user");

    let directory = std::env::temp_dir().join(format!("termru-backup-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).expect("create the backup directory");
    let dump = directory.join("termru.dump");

    let status = Command::new("pg_dump")
        .args([
            "--dbname",
            server.database_url(),
            "--format=custom",
            "--no-owner",
            "--file",
        ])
        .arg(&dump)
        .status()
        .expect("run pg_dump");
    assert!(status.success(), "pg_dump must succeed");

    // Restore into a second database and check the row survived.
    let admin_url = std::env::var(DATABASE_URL_ENV).expect("the admin url");
    let target = format!("termru_restore_{}", Uuid::new_v4().simple());
    assert!(create_database(&admin_url, &target).await);
    let target_url = database_url(&admin_url, &target);

    let status = Command::new("pg_restore")
        .args([
            "--dbname",
            &target_url,
            "--clean",
            "--if-exists",
            "--no-owner",
            "--exit-on-error",
        ])
        .arg(&dump)
        .status()
        .expect("run pg_restore");
    assert!(status.success(), "pg_restore must succeed");

    let pool = pool_for(&target_url);
    let client = pool.get().await.expect("a connection");
    let row = client
        .query_one(
            "SELECT count(*) AS n FROM users WHERE email = 'backup@example.com'",
            &[],
        )
        .await
        .expect("query the restored database");
    let count: i64 = row.get("n");
    assert_eq!(count, 1, "the restored database must contain the user");

    pool.close();
    drop_database(&admin_url, &target).await;
    let _ = std::fs::remove_dir_all(&directory);
}
