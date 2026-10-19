//! Online backup and restore.
//!
//! Backup uses SQLite's `VACUUM INTO`, which produces a consistent snapshot
//! of a live database without stopping writers (the store's connection is
//! serialized with other operations by the caller, so the snapshot reflects a
//! transactionally consistent point in time).

use std::path::Path;

use rusqlite::Connection;

use crate::error::StoreError;

fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Writes a consistent snapshot of `store` to `dest`. Overwrites an existing
/// file only after the new snapshot is fully written (writes to a temp file
/// in the destination directory, then renames).
pub fn backup_to(store: &crate::Store, dest: &Path) -> Result<(), StoreError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
    }
    let tmp = dest.with_extension("tmp-backup");
    if tmp.exists() {
        std::fs::remove_file(&tmp).map_err(StoreError::Io)?;
    }
    {
        let conn = store.lock();
        conn.execute_batch(&format!(
            "VACUUM INTO {};",
            quote_sql_string(&tmp.to_string_lossy())
        ))
        .map_err(StoreError::Sqlite)?;
    }
    // Verify the snapshot before putting it in place.
    {
        let snap = Connection::open(&tmp).map_err(StoreError::Sqlite)?;
        let check: String = snap
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(StoreError::Sqlite)?;
        if check != "ok" {
            let _ = std::fs::remove_file(&tmp);
            return Err(StoreError::Corruption(format!(
                "backup verification failed: {check}"
            )));
        }
    }
    std::fs::rename(&tmp, dest).map_err(StoreError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Replaces the database at `db_path` with the contents of `backup_path`.
///
/// Safety properties:
/// * The backup is verified (integrity check + schema version) **before** the
///   current database is touched.
/// * The current database is moved aside (not deleted) so a failed restore is
///   recoverable.
/// * Stale WAL/SHM files of the old database are removed so the restored file
///   cannot be replayed against.
///
/// The store must not be open on another connection while restoring.
pub fn restore_from_backup(db_path: &Path, backup_path: &Path) -> Result<(), StoreError> {
    // 1. Verify the backup thoroughly first.
    {
        let snap = Connection::open(backup_path).map_err(StoreError::Sqlite)?;
        let check: String = snap
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(StoreError::Sqlite)?;
        if check != "ok" {
            return Err(StoreError::Corruption(format!(
                "backup file is corrupted: {check}"
            )));
        }
        crate::migrations::verify_not_from_future(&snap)?;
    }

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
    }

    // 2. Move the current database aside.
    if db_path.exists() {
        let aside = db_path.with_extension("pre-restore");
        if aside.exists() {
            std::fs::remove_file(&aside).map_err(StoreError::Io)?;
        }
        std::fs::rename(db_path, &aside).map_err(StoreError::Io)?;
    }
    for suffix in ["-wal", "-shm"] {
        let stale_name = format!("{}{suffix}", db_path.display());
        let stale = Path::new(&stale_name);
        if stale.exists() {
            std::fs::remove_file(stale).map_err(StoreError::Io)?;
        }
    }

    // 3. Put the verified backup in place.
    std::fs::copy(backup_path, db_path).map_err(StoreError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(db_path, std::fs::Permissions::from_mode(0o600));
    }

    // 4. Re-verify in place.
    {
        let restored = Connection::open(db_path).map_err(StoreError::Sqlite)?;
        crate::migrations::verify_not_from_future(&restored)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use termru_core::Host;

    #[test]
    fn backup_and_restore_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("termru.sqlite3");
        let store = Store::open(&db).unwrap();
        let mut host = Host::new("web-1".into(), "web-1.example.com".into(), "deploy".into());
        store.repos().hosts().save(&mut host).unwrap();

        let backup = dir.path().join("backup.sqlite3");
        backup_to(&store, &backup).unwrap();
        assert!(backup.exists());

        // Modify after backup, then restore: the change must be gone.
        let mut host2 = Host::new("web-2".into(), "web-2.example.com".into(), "deploy".into());
        store.repos().hosts().save(&mut host2).unwrap();
        assert_eq!(store.repos().hosts().list().unwrap().len(), 2);

        drop(store);
        restore_from_backup(&db, &backup).unwrap();
        let restored = Store::open(&db).unwrap();
        let hosts = restored.repos().hosts().list().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "web-1");
        assert!(dir.path().join("termru.pre-restore").exists());
    }

    #[test]
    fn restore_rejects_corrupt_backup() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("termru.sqlite3");
        let backup = dir.path().join("bad.sqlite3");
        std::fs::write(&backup, b"definitely not a sqlite database").unwrap();
        let result = restore_from_backup(&db, &backup);
        assert!(result.is_err());
        assert!(!db.exists(), "corrupt backup must not replace the database");
    }
}
