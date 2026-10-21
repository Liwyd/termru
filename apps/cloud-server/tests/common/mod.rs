//! Test helpers: a throwaway database and a small HTTP client.
//!
//! The tests need a real PostgreSQL server. Set `TERMRU_TEST_DATABASE_URL` to
//! the URL of a database the test may create and drop other databases in
//! (for example `postgres://user@127.0.0.1:5432/postgres`). Without that
//! variable the whole suite skips with a loud message, exactly like the SSH
//! integration suite does when OpenSSH is missing.

#![allow(dead_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "harness setup failures must panic loudly in tests"
)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use deadpool_postgres::{Config as PgConfig, Manager, Pool, PoolConfig, Runtime};
use serde_json::Value;
use tokio_postgres::NoTls;
use tower::ServiceExt;
use uuid::Uuid;

use termru_cloud_server::config::Config;
use termru_cloud_server::db;

/// The environment variable that points at the test database.
pub const DATABASE_URL_ENV: &str = "TERMRU_TEST_DATABASE_URL";

/// A running server with its own database.
pub struct TestServer {
    /// The application router.
    pub router: Router,
    /// The pool of the throwaway database.
    pub pool: Pool,
    /// The configuration in use.
    pub config: Config,
    /// The name of the throwaway database.
    pub database: String,
    /// The URL of the administrative database.
    admin_url: String,
}

/// Installs a tracing subscriber so that server side errors are visible in
/// the test output.
pub fn init_tracing() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error"));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .try_init();
    });
}

impl TestServer {
    /// Creates a server with a fresh, migrated database.
    pub async fn start() -> Option<Self> {
        Self::start_with(None).await
    }

    /// Creates a server with a fresh database and an optional rate limit.
    pub async fn start_with(rate_limit_per_minute: Option<u32>) -> Option<Self> {
        init_tracing();
        let admin_url = std::env::var(DATABASE_URL_ENV).ok()?;
        if admin_url.trim().is_empty() {
            return None;
        }

        let database = format!("termru_test_{}", Uuid::new_v4().simple());
        let admin = connect_admin(&admin_url).await?;
        admin
            .execute(&format!(r#"CREATE DATABASE "{database}""#), &[])
            .await
            .ok()?;

        let database_url = replace_database(&admin_url, &database);
        let pool = create_pool(&database_url, 8);
        if db::run_migrations(&pool).await.is_err() {
            return None;
        }

        let config = test_config(database_url, rate_limit_per_minute);
        let state = termru_cloud_server::build_state(config.clone(), pool.clone());

        Some(Self {
            router: termru_cloud_server::routes::router(state),
            pool,
            config,
            database,
            admin_url,
        })
    }

    /// The URL of the throwaway database.
    pub fn database_url(&self) -> &str {
        &self.config.database_url
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // The pool must be closed before the database can be dropped.
        let database = self.database.clone();
        let admin_url = self.admin_url.clone();
        self.pool.close();

        // The drop has to finish before the process moves on: a task that is
        // spawned on the test runtime would never run (the runtime is gone
        // when the test ends), and every leaked database would eat disk.
        // A separate thread with its own runtime avoids both problems.
        let _ = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                if let Some(admin) = connect_admin(&admin_url).await {
                    let _ = admin
                        .execute(
                            &format!(r#"DROP DATABASE IF EXISTS "{database}" WITH (FORCE)"#),
                            &[],
                        )
                        .await;
                }
            });
        })
        .join();
    }
}

/// The configuration used by the tests.
fn test_config(database_url: String, rate_limit_per_minute: Option<u32>) -> Config {
    Config {
        bind_addr: "127.0.0.1:0".parse().expect("bind address"),
        database_url,
        pool_max_size: 8,
        access_token_ttl: std::time::Duration::from_secs(3600),
        refresh_token_ttl: std::time::Duration::from_secs(30 * 86_400),
        refresh_reuse_grace: std::time::Duration::from_secs(0),
        rate_limit_per_minute: rate_limit_per_minute.unwrap_or(120),
        login_max_failures: 3,
        login_lockout: std::time::Duration::from_secs(900),
        request_timeout: std::time::Duration::from_secs(30),
        max_body_bytes: 1024 * 1024,
        trust_forwarded_for: false,
    }
}

async fn connect_admin(url: &str) -> Option<deadpool_postgres::Client> {
    let mut config = PgConfig::new();
    config.url = Some(url.to_string());
    config.application_name = Some("termru-test-admin".to_string());
    let manager = Manager::from_config(
        config.get_pg_config().ok()?,
        NoTls,
        deadpool_postgres::ManagerConfig::default(),
    );
    let pool = Pool::builder(manager).max_size(1).build().ok()?;
    let client = pool.get().await.ok()?;
    // The pool must outlive the client, so it is intentionally leaked here.
    std::mem::forget(pool);
    Some(client)
}

fn create_pool(url: &str, max_size: usize) -> Pool {
    let mut config = PgConfig::new();
    config.url = Some(url.to_string());
    config.application_name = Some("termru-test".to_string());
    config.pool = Some(PoolConfig {
        max_size,
        ..PoolConfig::default()
    });
    config
        .create_pool(Some(Runtime::Tokio1), NoTls)
        .expect("create the test pool")
}

/// Replaces the database part of a URL.
fn replace_database(url: &str, database: &str) -> String {
    // postgres://user:pass@host:port/old?params
    let (base, params) = match url.split_once('?') {
        Some((base, params)) => (base, Some(params)),
        None => (url, None),
    };
    let without_path = base.rsplit_once('/').map(|(head, _)| head).unwrap_or(base);
    let mut out = format!("{without_path}/{database}");
    if let Some(params) = params {
        out.push('?');
        out.push_str(params);
    }
    out
}

/// An HTTP response of the test client.
#[derive(Debug)]
pub struct TestResponse {
    /// The status code.
    pub status: StatusCode,
    /// The parsed body (empty responses give `Value::Null`).
    pub body: Value,
}

impl TestResponse {
    /// Reads a string field of the body.
    pub fn str_field(&self, name: &str) -> String {
        self.body
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }
}

/// Sends a request to the router.
pub async fn send(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> TestResponse {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .expect("build the request"),
        None => builder.body(Body::empty()).expect("build the request"),
    };

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router must answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read the body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    TestResponse { status, body }
}

/// Creates a pool for an arbitrary database URL.
pub fn pool_for(url: &str) -> Pool {
    create_pool(url, 4)
}

/// Connects to the administrative database.
pub async fn admin_client(url: &str) -> Option<deadpool_postgres::Client> {
    connect_admin(url).await
}

/// Creates a database with the given name.
pub async fn create_database(admin_url: &str, name: &str) -> bool {
    match connect_admin(admin_url).await {
        Some(client) => client
            .execute(&format!(r#"CREATE DATABASE "{name}""#), &[])
            .await
            .is_ok(),
        None => false,
    }
}

/// Drops a database with the given name.
pub async fn drop_database(admin_url: &str, name: &str) {
    if let Some(client) = connect_admin(admin_url).await {
        let _ = client
            .execute(
                &format!(r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#),
                &[],
            )
            .await;
    }
}

/// The URL of a database inside the same server.
pub fn database_url(admin_url: &str, name: &str) -> String {
    replace_database(admin_url, name)
}

/// A device id for a test client.
pub fn device_id() -> Uuid {
    Uuid::new_v4()
}

/// A password that satisfies the policy.
pub const PASSWORD: &str = "correct horse battery staple";
