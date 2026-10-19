//! Store error types.

use thiserror::Error;

use termru_core::CoreError;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("failed to access database file: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid data: {0}")]
    Validation(#[from] CoreError),

    #[error("database is corrupted: {0}")]
    Corruption(String),

    #[error("migration failed: {0}")]
    Migration(String),

    #[error(
        "database schema version {found} is newer than this version of Termru supports ({supported}); upgrade Termru to open this database"
    )]
    SchemaFromFuture { found: u32, supported: u32 },

    #[error("{0} not found")]
    NotFound(&'static str),
}

impl StoreError {
    pub fn migration(msg: impl Into<String>) -> Self {
        Self::Migration(msg.into())
    }
}
