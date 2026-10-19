//! Identity repository + device-local key path bindings.

use rusqlite::{Connection, OptionalExtension, Row};

use termru_core::Identity;
use termru_core::sync::RecordKind;

use crate::Store;
use crate::error::StoreError;
use crate::repos::enqueue_mutation;
use crate::util::{datetime_from_ms, datetime_to_ms, now_ms};

pub struct Identities<'a> {
    pub(crate) store: &'a Store,
}

const IDENTITY_COLUMNS: &str =
    "id, name, comment, fingerprint, created_at, updated_at, deleted, server_rev, dirty";

fn row_to_identity(row: &Row<'_>) -> rusqlite::Result<Identity> {
    let id: String = row.get(0)?;
    Ok(Identity {
        id: id.parse().unwrap_or_default(),
        name: row.get(1)?,
        comment: row.get(2)?,
        fingerprint: row.get(3)?,
        created_at: datetime_from_ms(row.get(4)?),
        updated_at: datetime_from_ms(row.get(5)?),
        deleted: row.get::<_, i64>(6)? != 0,
        server_rev: row.get(7)?,
        dirty: row.get::<_, i64>(8)? != 0,
    })
}

pub(crate) fn get_row(conn: &Connection, id: &str) -> Result<Option<Identity>, StoreError> {
    conn.query_row(
        &format!("SELECT {IDENTITY_COLUMNS} FROM identities WHERE id = ?1"),
        [id],
        row_to_identity,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

pub(crate) fn upsert_identity_row(
    conn: &Connection,
    identity: &Identity,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO identities (id, name, comment, fingerprint, created_at, updated_at, deleted, server_rev, dirty) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(id) DO UPDATE SET \
           name = excluded.name, comment = excluded.comment, fingerprint = excluded.fingerprint, \
           created_at = excluded.created_at, updated_at = excluded.updated_at, \
           deleted = excluded.deleted, server_rev = excluded.server_rev, dirty = excluded.dirty",
        rusqlite::params![
            identity.id.to_string(),
            identity.name,
            identity.comment,
            identity.fingerprint,
            datetime_to_ms(&identity.created_at),
            datetime_to_ms(&identity.updated_at),
            i64::from(identity.deleted),
            identity.server_rev,
            i64::from(identity.dirty),
        ],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Number of non-deleted hosts that authenticate with this identity.
fn referencing_host_count(conn: &Connection, identity_id: &str) -> Result<i64, StoreError> {
    conn.query_row(
        "SELECT COUNT(*) FROM hosts WHERE deleted = 0 \
         AND json_extract(auth, '$.type') = 'private_key' \
         AND json_extract(auth, '$.identity_id') = ?1",
        [identity_id],
        |row| row.get(0),
    )
    .map_err(StoreError::Sqlite)
}

impl Identities<'_> {
    pub fn list(&self) -> Result<Vec<Identity>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {IDENTITY_COLUMNS} FROM identities WHERE deleted = 0 \
                     ORDER BY name COLLATE NOCASE, id"
                ))
                .map_err(StoreError::Sqlite)?;
            let rows = stmt
                .query_map([], row_to_identity)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::Sqlite)?;
            Ok(rows)
        })
    }

    pub fn get(&self, id: termru_core::ids::IdentityId) -> Result<Option<Identity>, StoreError> {
        self.store.with_conn(|conn| {
            let identity = get_row(conn, &id.to_string())?;
            Ok(identity.filter(|i| !i.deleted))
        })
    }

    pub fn save(&self, identity: &mut Identity) -> Result<(), StoreError> {
        identity.validate()?;
        identity.updated_at = chrono::Utc::now();
        let id = identity.id;
        self.store.with_tx(move |conn| {
            upsert_identity_row(conn, identity)?;
            enqueue_mutation(conn, &id.to_string(), RecordKind::Identity)?;
            Ok(())
        })
    }

    /// Soft-deletes an identity. Refuses while live hosts still reference it:
    /// silently switching a host's auth method would be a security-relevant
    /// surprise (the UI reassigns hosts first).
    pub fn soft_delete(&self, id: termru_core::ids::IdentityId) -> Result<(), StoreError> {
        let key = id.to_string();
        self.store.with_tx(move |conn| {
            let exists: i64 = conn
                .query_row("SELECT 1 FROM identities WHERE id = ?1", [&key], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(StoreError::Sqlite)?
                .unwrap_or(0);
            if exists == 0 {
                return Err(StoreError::NotFound("identity"));
            }
            let refs = referencing_host_count(conn, &key)?;
            if refs > 0 {
                return Err(StoreError::migration(format!(
                    "identity is still used by {refs} host(s); reassign them first"
                )));
            }
            conn.execute(
                "UPDATE identities SET deleted = 1, dirty = 1, updated_at = ?2 WHERE id = ?1",
                rusqlite::params![key, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            conn.execute(
                "DELETE FROM identity_bindings WHERE identity_id = ?1",
                [&key],
            )
            .map_err(StoreError::Sqlite)?;
            enqueue_mutation(conn, &key, RecordKind::Identity)?;
            Ok(())
        })
    }

    // ----- device-local key path binding (never synced) -----

    pub fn binding(&self, id: termru_core::ids::IdentityId) -> Result<Option<String>, StoreError> {
        self.store.with_conn(|conn| {
            conn.query_row(
                "SELECT key_path FROM identity_bindings WHERE identity_id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)
        })
    }

    pub fn bind(&self, id: termru_core::ids::IdentityId, key_path: &str) -> Result<(), StoreError> {
        if key_path.is_empty() || key_path.chars().any(|c| c == '\0') {
            return Err(StoreError::Validation(termru_core::CoreError::validation(
                "key path is invalid",
            )));
        }
        let key = id.to_string();
        let path = key_path.to_string();
        self.store.with_tx(move |conn| {
            let identity_exists: i64 = conn
                .query_row("SELECT 1 FROM identities WHERE id = ?1", [&key], |r| r.get(0))
                .optional()
                .map_err(StoreError::Sqlite)?
                .unwrap_or(0);
            if identity_exists == 0 {
                return Err(StoreError::NotFound("identity"));
            }
            conn.execute(
                "INSERT INTO identity_bindings (identity_id, key_path, updated_at) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(identity_id) DO UPDATE SET key_path = excluded.key_path, \
                 updated_at = excluded.updated_at",
                rusqlite::params![key, path, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            Ok(())
        })
    }

    pub fn unbind(&self, id: termru_core::ids::IdentityId) -> Result<(), StoreError> {
        self.store.with_conn(move |conn| {
            conn.execute(
                "DELETE FROM identity_bindings WHERE identity_id = ?1",
                [id.to_string()],
            )
            .map_err(StoreError::Sqlite)?;
            Ok(())
        })
    }

    /// All bindings as a map (used by connection code to resolve paths).
    pub fn bindings(
        &self,
    ) -> Result<std::collections::HashMap<termru_core::ids::IdentityId, String>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT identity_id, key_path FROM identity_bindings")
                .map_err(StoreError::Sqlite)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(StoreError::Sqlite)?;
            let mut out = std::collections::HashMap::new();
            for row in rows {
                let (id, path) = row.map_err(StoreError::Sqlite)?;
                if let Ok(id) = id.parse() {
                    out.insert(id, path);
                }
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use termru_core::{AuthMethod, Host};

    #[test]
    fn binding_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        let mut id = Identity::new("work key".into());
        repos.identities().save(&mut id).unwrap();
        repos
            .identities()
            .bind(id.id, "/home/u/.ssh/id_ed25519")
            .unwrap();
        assert_eq!(
            repos.identities().binding(id.id).unwrap().as_deref(),
            Some("/home/u/.ssh/id_ed25519")
        );
        repos.identities().unbind(id.id).unwrap();
        assert!(repos.identities().binding(id.id).unwrap().is_none());
    }

    #[test]
    fn delete_refused_while_hosts_reference_identity() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        let mut id = Identity::new("work key".into());
        repos.identities().save(&mut id).unwrap();
        let mut host = Host::new("h1".into(), "h1.example.com".into(), "u".into());
        host.auth = AuthMethod::PrivateKey { identity_id: id.id };
        repos.hosts().save(&mut host).unwrap();

        assert!(repos.identities().soft_delete(id.id).is_err());

        host.auth = AuthMethod::Agent;
        repos.hosts().save(&mut host).unwrap();
        repos.identities().soft_delete(id.id).unwrap();
        assert!(repos.identities().list().unwrap().is_empty());
    }
}
