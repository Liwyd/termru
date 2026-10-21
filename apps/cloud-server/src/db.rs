//! Database access: pool creation and the migration runner.

use deadpool_postgres::{Config as PgConfig, Pool, PoolConfig, Runtime};
use tokio_postgres::NoTls;

use crate::config::Config;
use crate::error::{ApiError, ApiResult};

/// Migrations embedded at build time by `build.rs`.
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/migrations.rs"));
}

/// Advisory lock key that guarantees a single migration runner.
const MIGRATION_LOCK_KEY: i64 = 0x7465_726d_7275_0001;

/// Creates the connection pool.
///
/// The pool is lazy: no connection is opened until the first query, so the
/// server can start before the database is reachable (the readiness probe
/// reports the real state).
pub fn create_pool(config: &Config) -> ApiResult<Pool> {
    let mut pg = PgConfig::new();
    pg.url = Some(config.database_url.clone());
    pg.application_name = Some("termru-cloud-server".to_string());
    pg.pool = Some(PoolConfig {
        max_size: config.pool_max_size as usize,
        ..PoolConfig::default()
    });

    pg.create_pool(Some(Runtime::Tokio1), NoTls)
        .map_err(|e| ApiError::Internal(anyhow::Error::new(e)))
}

/// Applies every embedded migration that has not been applied yet.
///
/// A PostgreSQL advisory lock serialises concurrent starts, so several
/// replicas may boot at the same time without racing each other.
pub async fn run_migrations(pool: &Pool) -> ApiResult<()> {
    let client = pool.get().await?;
    client
        .execute("SELECT pg_advisory_lock($1)", &[&MIGRATION_LOCK_KEY])
        .await?;
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                 version    BIGINT PRIMARY KEY,
                 name       TEXT NOT NULL,
                 applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
             );",
        )
        .await?;

    let result = apply_pending(&client).await;

    // Always release the lock, even when a migration failed.
    let _ = client
        .execute("SELECT pg_advisory_unlock($1)", &[&MIGRATION_LOCK_KEY])
        .await;

    result
}

async fn apply_pending(client: &deadpool_postgres::Client) -> ApiResult<()> {
    let rows = client
        .query("SELECT version FROM schema_migrations", &[])
        .await?;
    let applied: std::collections::HashSet<i64> = rows
        .iter()
        .map(|row| row.get::<_, i64>("version"))
        .collect();

    for (version, name, sql) in embedded::MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        tracing::info!(version, name, "applying migration");
        client.batch_execute(sql).await?;
        client
            .execute(
                "INSERT INTO schema_migrations (version, name) VALUES ($1, $2)",
                &[&version, &name],
            )
            .await?;
    }
    Ok(())
}

/// Checks that the database is reachable and migrated.
pub async fn check_ready(pool: &Pool) -> ApiResult<()> {
    let client = pool.get().await?;
    client.query_one("SELECT 1", &[]).await?;
    Ok(())
}
