//! Local SQLite persistence for Termru.
//!
//! Design notes:
//!
//! * **Single connection behind a mutex.** The desktop app has modest write
//!   rates; serializing access avoids `SQLITE_BUSY` complexity and matches the
//!   single-process, multi-window Tauri architecture (all windows share one
//!   app state). WAL mode still allows safe access from external tools.
//! * **Sync bookkeeping lives in the same transaction as the user's edit.**
//!   A local mutation is only queued if the row write succeeded, so a crash
//!   can never produce a queued-but-missing edit or vice versa.
//! * **Secrets never touch this database.** Passwords and passphrases live in
//!   the OS credential vault; the schema has no field that could hold them.
//! * **Migrations are forward-only and version-checked.** Opening a database
//!   created by a newer Termru fails with an actionable error instead of
//!   silently corrupting newer schema.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

mod backup;
mod error;
mod migrations;
mod repos;
mod util;

pub use backup::{backup_to, restore_from_backup};
pub use error::StoreError;
pub use repos::conflicts::ConflictRecord;
pub use repos::sync::{ApplyOutcome, PendingMutation, PushRecord, SyncMeta};
pub use repos::{DevicePrefs, Repos};

/// The local application database.
pub struct Store {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl Store {
    /// Opens (creating if necessary) the database at `path` and brings it to
    /// the current schema version.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
        }
        let conn = Self::connect(path)?;
        let store = Self {
            conn: Mutex::new(conn),
            path: Some(path.to_path_buf()),
        };
        store.init()?;
        Ok(store)
    }

    /// In-memory database for tests.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(StoreError::Sqlite)?;
        let store = Self {
            conn: Mutex::new(conn),
            path: None,
        };
        store.init()?;
        Ok(store)
    }

    fn connect(path: &Path) -> Result<Connection, StoreError> {
        let conn = Connection::open(path).map_err(StoreError::Sqlite)?;
        Self::configure(&conn)?;
        Ok(conn)
    }

    fn configure(conn: &Connection) -> Result<(), StoreError> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(StoreError::Sqlite)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(StoreError::Sqlite)?;
        conn.pragma_update(None, "busy_timeout", 5_000u32)
            .map_err(StoreError::Sqlite)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(StoreError::Sqlite)?;
        Ok(())
    }

    fn init(&self) -> Result<(), StoreError> {
        let mut conn = self.lock();
        // Detect databases written by a newer Termru before touching anything.
        migrations::verify_not_from_future(&conn)?;
        migrations::run(&mut conn)?;
        // Restrict permissions on POSIX: the database may reference hostnames
        // and usernames; keep it private to the user.
        #[cfg(unix)]
        if let Some(path) = &self.path {
            restrict_permissions(path);
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Runs a quick integrity check; used by the UI to surface corruption
    /// early with an actionable "restore from backup" message.
    pub fn integrity_check(&self) -> Result<(), StoreError> {
        let conn = self.lock();
        let result: String = conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(StoreError::Sqlite)?;
        if result == "ok" {
            Ok(())
        } else {
            Err(StoreError::Corruption(result))
        }
    }

    /// Access to all repositories. Handles borrow the store for the duration
    /// of a (short) synchronous operation.
    pub fn repos(&self) -> Repos<'_> {
        Repos { store: self }
    }

    pub(crate) fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let conn = self.lock();
        f(&conn)
    }

    /// Runs `f` inside an immediate transaction (write path).
    pub(crate) fn with_tx<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(StoreError::Sqlite)?;
        let out = f(&tx)?;
        tx.commit().map_err(StoreError::Sqlite)?;
        Ok(out)
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    let _ = std::fs::set_permissions(path, perms);
}
