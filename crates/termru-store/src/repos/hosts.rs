//! Host repository.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row};

use termru_core::ids::{GroupId, HostId, TagId};
use termru_core::sync::RecordKind;
use termru_core::{AuthMethod, Host};

use crate::Store;
use crate::error::StoreError;
use crate::repos::{enqueue_mutation, to_json};
use crate::util::{bool_to_int, datetime_from_ms, datetime_to_ms, now_ms};

pub struct Hosts<'a> {
    pub(crate) store: &'a Store,
}

pub(crate) const HOST_COLUMNS: &str = "id, name, address, port, username, auth, group_id, favorite, \
     description, term, keepalive_seconds, sort_order, created_at, updated_at, deleted, \
     server_rev, dirty";

pub(crate) fn row_to_host(row: &Row<'_>) -> rusqlite::Result<Host> {
    let id: String = row.get(0)?;
    let auth_json: String = row.get(5)?;
    let group_id: Option<String> = row.get(6)?;
    let favorite: i64 = row.get(7)?;
    let description: Option<String> = row.get(8)?;
    let created_ms: i64 = row.get(12)?;
    let updated_ms: i64 = row.get(13)?;
    let deleted: i64 = row.get(14)?;
    Ok(Host {
        id: id.parse().unwrap_or_default(),
        name: row.get(1)?,
        address: row.get(2)?,
        port: row.get(3)?,
        username: row.get(4)?,
        // Corrupt auth JSON is caught by callers that need the value; storing
        // Password as the safe default keeps reads resilient.
        auth: serde_json::from_str(&auth_json).unwrap_or(AuthMethod::Password),
        group_id: group_id.and_then(|g| g.parse().ok()),
        tags: Vec::new(),
        favorite: favorite != 0,
        description,
        term: row.get(9)?,
        keepalive_seconds: row.get(10)?,
        sort_order: row.get(11)?,
        created_at: datetime_from_ms(created_ms),
        updated_at: datetime_from_ms(updated_ms),
        deleted: deleted != 0,
        server_rev: row.get(15)?,
        dirty: row.get::<_, i64>(16)? != 0,
    })
}

pub(crate) fn fetch_tags(conn: &Connection, hosts: &mut [Host]) -> Result<(), StoreError> {
    if hosts.is_empty() {
        return Ok(());
    }
    let mut stmt = conn
        .prepare("SELECT host_id, tag_id FROM host_tags ORDER BY tag_id")
        .map_err(StoreError::Sqlite)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(StoreError::Sqlite)?;
    let mut by_host: HashMap<String, Vec<TagId>> = HashMap::new();
    for row in rows {
        let (host_id, tag_id) = row.map_err(StoreError::Sqlite)?;
        if let Ok(tag) = tag_id.parse() {
            by_host.entry(host_id).or_default().push(tag);
        }
    }
    for host in hosts.iter_mut() {
        if let Some(tags) = by_host.remove(&host.id.to_string()) {
            host.tags = tags;
        }
    }
    Ok(())
}

fn upsert_host_row(conn: &Connection, host: &Host) -> Result<(), StoreError> {
    let auth = to_json(&host.auth, "host auth")?;
    conn.execute(
        "INSERT INTO hosts (id, name, address, port, username, auth, group_id, favorite, \
         description, term, keepalive_seconds, sort_order, created_at, updated_at, deleted, \
         server_rev, dirty) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) \
         ON CONFLICT(id) DO UPDATE SET \
           name = excluded.name, address = excluded.address, port = excluded.port, \
           username = excluded.username, auth = excluded.auth, group_id = excluded.group_id, \
           favorite = excluded.favorite, description = excluded.description, term = excluded.term, \
           keepalive_seconds = excluded.keepalive_seconds, sort_order = excluded.sort_order, \
           created_at = excluded.created_at, updated_at = excluded.updated_at, \
           deleted = excluded.deleted, server_rev = excluded.server_rev, dirty = excluded.dirty",
        rusqlite::params![
            host.id.to_string(),
            host.name,
            host.address,
            host.port,
            host.username,
            auth,
            host.group_id.map(|g| g.to_string()),
            bool_to_int(host.favorite),
            host.description,
            host.term,
            host.keepalive_seconds,
            host.sort_order,
            datetime_to_ms(&host.created_at),
            datetime_to_ms(&host.updated_at),
            bool_to_int(host.deleted),
            host.server_rev,
            bool_to_int(host.dirty),
        ],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

fn replace_host_tags(conn: &Connection, host_id: HostId, tags: &[TagId]) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM host_tags WHERE host_id = ?1",
        [host_id.to_string()],
    )
    .map_err(StoreError::Sqlite)?;
    for tag in tags {
        conn.execute(
            "INSERT OR IGNORE INTO host_tags (host_id, tag_id) VALUES (?1, ?2)",
            rusqlite::params![host_id.to_string(), tag.to_string()],
        )
        .map_err(StoreError::Sqlite)?;
    }
    Ok(())
}

impl Hosts<'_> {
    pub fn list(&self) -> Result<Vec<Host>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {HOST_COLUMNS} FROM hosts WHERE deleted = 0 \
                     ORDER BY sort_order, name COLLATE NOCASE, id"
                ))
                .map_err(StoreError::Sqlite)?;
            let mut hosts = stmt
                .query_map([], row_to_host)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::Sqlite)?;
            fetch_tags(conn, &mut hosts)?;
            Ok(hosts)
        })
    }

    pub fn get(&self, id: HostId) -> Result<Option<Host>, StoreError> {
        self.store.with_conn(|conn| {
            let mut host = get_row(conn, &id.to_string())?;
            if let Some(h) = host.as_mut() {
                fetch_tags(conn, std::slice::from_mut(h))?;
                if h.deleted {
                    return Ok(None);
                }
            }
            Ok(host)
        })
    }

    pub fn get_including_deleted(&self, id: HostId) -> Result<Option<Host>, StoreError> {
        self.store.with_conn(|conn| {
            let mut host = get_row(conn, &id.to_string())?;
            if let Some(h) = host.as_mut() {
                fetch_tags(conn, std::slice::from_mut(h))?;
            }
            Ok(host)
        })
    }

    /// Escapes LIKE metacharacters and searches name/address/username/tags.
    pub fn search(&self, query: &str) -> Result<Vec<Host>, StoreError> {
        let pattern = format!("%{}%", termru_core::validate::escape_like_pattern(query));
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {HOST_COLUMNS} FROM hosts \
                     WHERE deleted = 0 AND (name LIKE ?1 ESCAPE '\\' \
                        OR address LIKE ?1 ESCAPE '\\' \
                        OR username LIKE ?1 ESCAPE '\\' \
                        OR EXISTS (SELECT 1 FROM host_tags ht JOIN tags t ON t.id = ht.tag_id \
                                   WHERE ht.host_id = hosts.id AND t.deleted = 0 \
                                   AND t.name LIKE ?1 ESCAPE '\\')) \
                     ORDER BY favorite DESC, name COLLATE NOCASE, id"
                ))
                .map_err(StoreError::Sqlite)?;
            let mut hosts = stmt
                .query_map([&pattern], row_to_host)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::Sqlite)?;
            fetch_tags(conn, &mut hosts)?;
            Ok(hosts)
        })
    }

    /// Saves a host as a **local edit**: validates, stamps `updated_at`,
    /// marks it dirty, and queues a sync mutation — atomically.
    pub fn save(&self, host: &mut Host) -> Result<(), StoreError> {
        host.validate()?;
        host.updated_at = chrono::Utc::now();
        let id = host.id;
        self.store.with_tx(move |conn| {
            ensure_group_exists(conn, host.group_id)?;
            ensure_tags_exist(conn, &host.tags)?;
            upsert_host_row(conn, host)?;
            replace_host_tags(conn, id, &host.tags)?;
            enqueue_mutation(conn, &id.to_string(), RecordKind::Host)?;
            Ok(())
        })
    }

    /// Soft-deletes a host (tombstone) and queues the deletion for sync.
    pub fn soft_delete(&self, id: HostId) -> Result<(), StoreError> {
        let key = id.to_string();
        self.store.with_tx(move |conn| {
            let exists: Option<i64> = conn
                .query_row("SELECT 1 FROM hosts WHERE id = ?1", [&key], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(StoreError::Sqlite)?;
            if exists.is_none() {
                return Err(StoreError::NotFound("host"));
            }
            conn.execute(
                "UPDATE hosts SET deleted = 1, dirty = 1, updated_at = ?2 WHERE id = ?1",
                rusqlite::params![key, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            enqueue_mutation(conn, &key, RecordKind::Host)?;
            Ok(())
        })
    }

    pub fn set_favorite(&self, id: HostId, favorite: bool) -> Result<(), StoreError> {
        let mut host = self.get(id)?.ok_or(StoreError::NotFound("host"))?;
        host.favorite = favorite;
        self.save(&mut host)
    }

    pub fn touch_recent(&self, id: HostId) -> Result<(), StoreError> {
        crate::repos::recents::Recents { store: self.store }.touch(id)
    }
}

pub(crate) fn get_row(conn: &Connection, id: &str) -> Result<Option<Host>, StoreError> {
    conn.query_row(
        &format!("SELECT {HOST_COLUMNS} FROM hosts WHERE id = ?1"),
        [id],
        row_to_host,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

fn ensure_group_exists(conn: &Connection, group: Option<GroupId>) -> Result<(), StoreError> {
    if let Some(gid) = group {
        let exists: i64 = conn
            .query_row(
                "SELECT 1 FROM groups WHERE id = ?1",
                [gid.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)?
            .unwrap_or(0);
        if exists == 0 {
            return Err(StoreError::migration(format!(
                "host references missing group {gid}"
            )));
        }
    }
    Ok(())
}

fn ensure_tags_exist(conn: &Connection, tags: &[TagId]) -> Result<(), StoreError> {
    for tag in tags {
        let exists: i64 = conn
            .query_row("SELECT 1 FROM tags WHERE id = ?1", [tag.to_string()], |r| {
                r.get(0)
            })
            .optional()
            .map_err(StoreError::Sqlite)?
            .unwrap_or(0);
        if exists == 0 {
            return Err(StoreError::migration(format!(
                "host references missing tag {tag}"
            )));
        }
    }
    Ok(())
}

/// Applies a host payload received from the sync server. Caller must have
/// decided this is not a conflict. Clears local dirtiness for the record.
pub(crate) fn apply_remote(conn: &Connection, host: &Host) -> Result<(), StoreError> {
    upsert_host_row(conn, host)?;
    replace_host_tags(conn, host.id, &host.tags)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn sample_host(name: &str) -> Host {
        Host::new(
            name.to_string(),
            format!("{name}.example.com"),
            "deploy".to_string(),
        )
    }

    #[test]
    fn save_and_list_roundtrip() {
        let store = store();
        let mut host = sample_host("web-1");
        store.repos().hosts().save(&mut host).unwrap();
        let listed = store.repos().hosts().list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "web-1");
        assert!(!listed[0].dirty);
        assert_eq!(listed[0].server_rev, None);
    }

    #[test]
    fn save_rejects_invalid_host_without_writing() {
        let store = store();
        let mut host = sample_host("web-1");
        host.address = "bad address".to_string();
        assert!(store.repos().hosts().save(&mut host).is_err());
        assert!(store.repos().hosts().list().unwrap().is_empty());
    }

    #[test]
    fn save_queues_exactly_one_coalesced_mutation() {
        let store = store();
        let mut host = sample_host("web-1");
        store.repos().hosts().save(&mut host).unwrap();
        host.name = "web-1-renamed".to_string();
        store.repos().hosts().save(&mut host).unwrap();
        let pending = store.repos().sync().pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].record_id, host.id.to_string());
    }

    #[test]
    fn soft_delete_hides_host_and_queues_delete() {
        let store = store();
        let mut host = sample_host("web-1");
        store.repos().hosts().save(&mut host).unwrap();
        store.repos().hosts().soft_delete(host.id).unwrap();
        assert!(store.repos().hosts().list().unwrap().is_empty());
        assert!(
            store
                .repos()
                .hosts()
                .get_including_deleted(host.id)
                .unwrap()
                .is_some_and(|h| h.deleted)
        );
        let pending = store.repos().sync().pending().unwrap();
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn tags_and_favorites_roundtrip() {
        let store = store();
        let mut tag = termru_core::Tag::new("prod".to_string());
        store.repos().tags().save(&mut tag).unwrap();
        let mut host = sample_host("db-1");
        host.tags = vec![tag.id];
        host.favorite = true;
        store.repos().hosts().save(&mut host).unwrap();
        let loaded = store.repos().hosts().get(host.id).unwrap().unwrap();
        assert_eq!(loaded.tags, vec![tag.id]);
        assert!(loaded.favorite);
    }

    #[test]
    fn search_matches_across_fields_and_escapes_wildcards() {
        let store = store();
        let mut a = sample_host("alpha");
        let mut b = sample_host("beta");
        store.repos().hosts().save(&mut a).unwrap();
        store.repos().hosts().save(&mut b).unwrap();
        assert_eq!(store.repos().hosts().search("alp").unwrap().len(), 1);
        // A literal '%' in the query must not match everything.
        assert_eq!(store.repos().hosts().search("%").unwrap().len(), 0);
        assert_eq!(store.repos().hosts().search("").unwrap().len(), 2);
    }

    #[test]
    fn save_with_unknown_group_or_tag_fails() {
        let store = store();
        let mut host = sample_host("web-1");
        host.group_id = Some(GroupId::new());
        assert!(store.repos().hosts().save(&mut host).is_err());
        host.group_id = None;
        host.tags = vec![TagId::new()];
        assert!(store.repos().hosts().save(&mut host).is_err());
    }
}
