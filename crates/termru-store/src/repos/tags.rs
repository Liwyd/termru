//! Tag repository.

use rusqlite::{Connection, OptionalExtension, Row};

use termru_core::Tag;
use termru_core::sync::RecordKind;

use crate::Store;
use crate::error::StoreError;
use crate::repos::enqueue_mutation;
use crate::util::{datetime_from_ms, datetime_to_ms, now_ms};

pub struct Tags<'a> {
    pub(crate) store: &'a Store,
}

const TAG_COLUMNS: &str = "id, name, color, created_at, updated_at, deleted, server_rev, dirty";

fn row_to_tag(row: &Row<'_>) -> rusqlite::Result<Tag> {
    let id: String = row.get(0)?;
    Ok(Tag {
        id: id.parse().unwrap_or_default(),
        name: row.get(1)?,
        color: row.get(2)?,
        created_at: datetime_from_ms(row.get(3)?),
        updated_at: datetime_from_ms(row.get(4)?),
        deleted: row.get::<_, i64>(5)? != 0,
        server_rev: row.get(6)?,
        dirty: row.get::<_, i64>(7)? != 0,
    })
}

pub(crate) fn get_row(conn: &Connection, id: &str) -> Result<Option<Tag>, StoreError> {
    conn.query_row(
        &format!("SELECT {TAG_COLUMNS} FROM tags WHERE id = ?1"),
        [id],
        row_to_tag,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

pub(crate) fn upsert_tag_row(conn: &Connection, tag: &Tag) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO tags (id, name, color, created_at, updated_at, deleted, server_rev, dirty) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(id) DO UPDATE SET \
           name = excluded.name, color = excluded.color, created_at = excluded.created_at, \
           updated_at = excluded.updated_at, deleted = excluded.deleted, \
           server_rev = excluded.server_rev, dirty = excluded.dirty",
        rusqlite::params![
            tag.id.to_string(),
            tag.name,
            tag.color,
            datetime_to_ms(&tag.created_at),
            datetime_to_ms(&tag.updated_at),
            i64::from(tag.deleted),
            tag.server_rev,
            i64::from(tag.dirty),
        ],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

impl Tags<'_> {
    pub fn list(&self) -> Result<Vec<Tag>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {TAG_COLUMNS} FROM tags WHERE deleted = 0 \
                     ORDER BY name COLLATE NOCASE, id"
                ))
                .map_err(StoreError::Sqlite)?;
            let tags = stmt
                .query_map([], row_to_tag)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::Sqlite)?;
            Ok(tags)
        })
    }

    pub fn get(&self, id: termru_core::ids::TagId) -> Result<Option<Tag>, StoreError> {
        self.store.with_conn(|conn| {
            let tag = get_row(conn, &id.to_string())?;
            Ok(tag.filter(|t| !t.deleted))
        })
    }

    pub fn save(&self, tag: &mut Tag) -> Result<(), StoreError> {
        tag.validate()?;
        tag.updated_at = chrono::Utc::now();
        // A local edit marks the row dirty (see the host repository).
        tag.dirty = true;
        let id = tag.id;
        self.store.with_tx(move |conn| {
            upsert_tag_row(conn, tag)?;
            enqueue_mutation(conn, &id.to_string(), RecordKind::Tag)?;
            Ok(())
        })
    }

    /// Soft-deletes a tag and detaches it from hosts. Affected hosts are
    /// marked dirty so the cleaned tag list propagates to other devices.
    pub fn soft_delete(&self, id: termru_core::ids::TagId) -> Result<(), StoreError> {
        let key = id.to_string();
        self.store.with_tx(move |conn| {
            let exists: i64 = conn
                .query_row("SELECT 1 FROM tags WHERE id = ?1", [&key], |r| r.get(0))
                .optional()
                .map_err(StoreError::Sqlite)?
                .unwrap_or(0);
            if exists == 0 {
                return Err(StoreError::NotFound("tag"));
            }
            let affected_hosts: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT host_id FROM host_tags WHERE tag_id = ?1")
                    .map_err(StoreError::Sqlite)?;
                stmt.query_map([&key], |r| r.get(0))
                    .map_err(StoreError::Sqlite)?
                    .collect::<Result<_, _>>()
                    .map_err(StoreError::Sqlite)?
            };
            conn.execute("DELETE FROM host_tags WHERE tag_id = ?1", [&key])
                .map_err(StoreError::Sqlite)?;
            conn.execute(
                "UPDATE tags SET deleted = 1, dirty = 1, updated_at = ?2 WHERE id = ?1",
                rusqlite::params![key, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            for host_id in &affected_hosts {
                conn.execute(
                    "UPDATE hosts SET dirty = 1, updated_at = ?2 WHERE id = ?1 AND deleted = 0",
                    rusqlite::params![host_id, now_ms()],
                )
                .map_err(StoreError::Sqlite)?;
                enqueue_mutation(conn, host_id, RecordKind::Host)?;
            }
            enqueue_mutation(conn, &key, RecordKind::Tag)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn tag_lifecycle_and_detach_on_delete() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        let mut tag = Tag::new("prod".into());
        repos.tags().save(&mut tag).unwrap();
        let mut host = termru_core::Host::new("h1".into(), "h1.example.com".into(), "u".into());
        host.tags = vec![tag.id];
        repos.hosts().save(&mut host).unwrap();

        repos.tags().soft_delete(tag.id).unwrap();
        assert!(repos.tags().list().unwrap().is_empty());
        let host = repos.hosts().get(host.id).unwrap().unwrap();
        assert!(host.tags.is_empty());
        assert!(host.dirty, "detach must mark the host dirty for sync");
    }

    #[test]
    fn invalid_tag_color_rejected() {
        let store = Store::open_in_memory().unwrap();
        let mut tag = Tag::new("x".into());
        tag.color = Some("nope".into());
        assert!(store.repos().tags().save(&mut tag).is_err());
    }
}
