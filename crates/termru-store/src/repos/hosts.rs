//! Host repository.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row};

use termru_core::ids::{GroupId, HostId, TagId};
use termru_core::sync::RecordKind;
use termru_core::validate::JumpNode;
use termru_core::{AuthMethod, Host};

use crate::Store;
use crate::error::StoreError;
use crate::repos::{enqueue_mutation, to_json};
use crate::util::{bool_to_int, datetime_from_ms, datetime_to_ms, now_ms};

pub struct Hosts<'a> {
    pub(crate) store: &'a Store,
}

/// An identity an import creates, with the key file it is bound to on this
/// device (the path only: the key itself is never read or copied).
#[derive(Debug, Clone)]
pub struct ImportedIdentity {
    pub identity: termru_core::Identity,
    pub key_path: String,
}

/// What [`Hosts::import`] stores in one transaction.
#[derive(Debug, Clone, Default)]
pub struct ImportBatch {
    pub identities: Vec<ImportedIdentity>,
    pub hosts: Vec<Host>,
}

pub(crate) const HOST_COLUMNS: &str = "id, name, address, port, username, auth, group_id, favorite, \
     description, term, keepalive_seconds, sort_order, created_at, updated_at, deleted, \
     server_rev, dirty, jump_chain, forwards, env, startup_command, agent_forward, auth_order";

/// Number of columns in [`HOST_COLUMNS`]: queries that select more columns
/// after the host's own (a join) find theirs from this index on.
pub(crate) const HOST_COLUMN_COUNT: usize = 23;

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
        jump_chain: json_column(row, 17)?.unwrap_or_default(),
        forwards: json_column(row, 18)?.unwrap_or_default(),
        env: json_column(row, 19)?.unwrap_or_default(),
        startup_command: row.get(20)?,
        agent_forward: row.get::<_, i64>(21)? != 0,
        auth_order: json_column(row, 22)?,
    })
}

/// A JSON text column, or `None` when it is NULL or does not parse. The
/// columns are only ever written by this crate; a value that fails to parse
/// (a hand-edited database) reads as "not set" rather than making the whole
/// host list unreadable.
fn json_column<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<T>> {
    let text: Option<String> = row.get(index)?;
    Ok(text.and_then(|text| serde_json::from_str(&text).ok()))
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
    let jump_chain = to_json(&host.jump_chain, "host jump chain")?;
    let forwards = to_json(&host.forwards, "host forwards")?;
    let env = to_json(&host.env, "host env")?;
    let auth_order = host
        .auth_order
        .as_ref()
        .map(|order| to_json(order, "host auth order"))
        .transpose()?;
    conn.execute(
        "INSERT INTO hosts (id, name, address, port, username, auth, group_id, favorite, \
         description, term, keepalive_seconds, sort_order, created_at, updated_at, deleted, \
         server_rev, dirty, jump_chain, forwards, env, startup_command, agent_forward, \
         auth_order) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
                 ?18, ?19, ?20, ?21, ?22, ?23) \
         ON CONFLICT(id) DO UPDATE SET \
           name = excluded.name, address = excluded.address, port = excluded.port, \
           username = excluded.username, auth = excluded.auth, group_id = excluded.group_id, \
           favorite = excluded.favorite, description = excluded.description, term = excluded.term, \
           keepalive_seconds = excluded.keepalive_seconds, sort_order = excluded.sort_order, \
           created_at = excluded.created_at, updated_at = excluded.updated_at, \
           deleted = excluded.deleted, server_rev = excluded.server_rev, dirty = excluded.dirty, \
           jump_chain = excluded.jump_chain, forwards = excluded.forwards, env = excluded.env, \
           startup_command = excluded.startup_command, agent_forward = excluded.agent_forward, \
           auth_order = excluded.auth_order",
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
            jump_chain,
            forwards,
            env,
            host.startup_command,
            bool_to_int(host.agent_forward),
            auth_order,
        ],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// The part of a stored host the jump-chain walk needs (`None`: no such live
/// host). Unparseable chains read as empty, like everywhere else.
fn jump_node(conn: &Connection, id: HostId) -> Option<JumpNode> {
    conn.query_row(
        "SELECT name, jump_chain FROM hosts WHERE id = ?1 AND deleted = 0",
        [id.to_string()],
        |row| {
            let chain: String = row.get(1)?;
            Ok(JumpNode {
                name: row.get(0)?,
                jump_chain: serde_json::from_str(&chain).unwrap_or_default(),
            })
        },
    )
    .ok()
}

/// A local save must leave a route that can be walked: every hop exists and
/// the chain is neither a cycle nor too long. (Synced records skip this on
/// purpose: they arrive in any order, so a hop may follow the host naming it.
/// The route is checked again whenever a connection is built.)
fn check_jump_chain(conn: &Connection, host: &Host, others: &[Host]) -> Result<(), StoreError> {
    termru_core::validate::resolve_jump_chain(host.id, &host.name, &host.jump_chain, &|id| {
        others
            .iter()
            .find(|other| other.id == id && !other.deleted)
            .map(|other| JumpNode {
                name: other.name.clone(),
                jump_chain: other.jump_chain.clone(),
            })
            .or_else(|| jump_node(conn, id))
    })?;
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
    ///
    /// The jump chain must lead somewhere: every hop exists and the route is
    /// neither a cycle nor longer than [`termru_core::validate::MAX_JUMP_HOPS`].
    pub fn save(&self, host: &mut Host) -> Result<(), StoreError> {
        self.save_with(host, true)
    }

    fn save_with(&self, host: &mut Host, check_route: bool) -> Result<(), StoreError> {
        host.validate()?;
        host.updated_at = chrono::Utc::now();
        // A local edit marks the row dirty: the sync layer needs that flag to
        // detect that both sides changed the record (a conflict) instead of
        // silently overwriting the local change.
        host.dirty = true;
        let id = host.id;
        self.store.with_tx(move |conn| {
            ensure_group_exists(conn, host.group_id)?;
            ensure_tags_exist(conn, &host.tags)?;
            if check_route {
                check_jump_chain(conn, host, &[])?;
            }
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
        // Starring a host must not fail because a jump host it names was
        // deleted since: that is for the editor (and the connect) to report.
        self.save_with(&mut host, false)
    }

    /// Stores everything of an import (identities with their key bindings,
    /// hosts) in **one transaction**: either all of it is there afterwards
    /// or none of it. The hosts may name each other as jump hosts; they are
    /// checked as a set. Key files are only referenced by path, never read.
    pub fn import(&self, batch: &mut ImportBatch) -> Result<(), StoreError> {
        for entry in &mut batch.identities {
            entry.identity.validate()?;
            if entry.key_path.is_empty() || entry.key_path.contains('\0') {
                return Err(StoreError::Validation(termru_core::CoreError::validation(
                    "key path is invalid",
                )));
            }
        }
        for host in &batch.hosts {
            host.validate()?;
        }
        let now = chrono::Utc::now();
        for entry in &mut batch.identities {
            entry.identity.updated_at = now;
            entry.identity.dirty = true;
        }
        for host in &mut batch.hosts {
            host.updated_at = now;
            host.dirty = true;
        }
        let batch = &*batch;
        self.store.with_tx(move |conn| {
            for entry in &batch.identities {
                let key = entry.identity.id.to_string();
                crate::repos::identities::upsert_identity_row(conn, &entry.identity)?;
                crate::repos::identities::bind_row(conn, &key, &entry.key_path)?;
                enqueue_mutation(conn, &key, RecordKind::Identity)?;
            }
            for host in &batch.hosts {
                ensure_group_exists(conn, host.group_id)?;
                ensure_tags_exist(conn, &host.tags)?;
                check_jump_chain(conn, host, &batch.hosts)?;
                upsert_host_row(conn, host)?;
                replace_host_tags(conn, host.id, &host.tags)?;
                enqueue_mutation(conn, &host.id.to_string(), RecordKind::Host)?;
            }
            Ok(())
        })
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
    use termru_core::sync::RecordKind;

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
        // A local edit marks the record dirty so that the sync layer can
        // detect a conflict instead of overwriting the local change.
        assert!(listed[0].dirty);
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

    #[test]
    fn the_column_count_matches_the_column_list() {
        assert_eq!(HOST_COLUMNS.split(',').count(), HOST_COLUMN_COUNT);
    }

    fn rich_host(name: &str) -> Host {
        use termru_core::{AuthKind, EnvVar, Forward, ForwardKind};
        let mut host = sample_host(name);
        host.forwards = vec![
            Forward {
                kind: ForwardKind::Local,
                bind_address: Some("127.0.0.1".into()),
                bind_port: 5433,
                target_host: Some("db.internal".into()),
                target_port: Some(5432),
                enabled: true,
            },
            Forward {
                kind: ForwardKind::Dynamic,
                bind_address: None,
                bind_port: 1080,
                target_host: None,
                target_port: None,
                enabled: false,
            },
        ];
        host.env = vec![EnvVar {
            name: "TERMRU_PROFILE".into(),
            value: "ops".into(),
        }];
        host.startup_command = Some("cd /srv && ls".into());
        host.agent_forward = true;
        host.auth_order = Some(vec![AuthKind::Agent, AuthKind::Password]);
        host
    }

    #[test]
    fn the_v2_fields_survive_a_save_and_a_reload() {
        let store = store();
        let mut jump = sample_host("bastion");
        store.repos().hosts().save(&mut jump).unwrap();
        let mut host = rich_host("web-1");
        host.jump_chain = vec![jump.id];
        store.repos().hosts().save(&mut host).unwrap();

        let loaded = store.repos().hosts().get(host.id).unwrap().unwrap();
        assert_eq!(loaded.jump_chain, vec![jump.id]);
        assert_eq!(loaded.forwards, host.forwards);
        assert_eq!(loaded.env, host.env);
        assert_eq!(loaded.startup_command.as_deref(), Some("cd /srv && ls"));
        assert!(loaded.agent_forward);
        assert_eq!(loaded.auth_order, host.auth_order);

        // Clearing them stores the empty state, not the previous value.
        let mut plain = loaded;
        plain.jump_chain.clear();
        plain.forwards.clear();
        plain.env.clear();
        plain.startup_command = None;
        plain.agent_forward = false;
        plain.auth_order = None;
        store.repos().hosts().save(&mut plain).unwrap();
        let loaded = store.repos().hosts().get(plain.id).unwrap().unwrap();
        assert!(loaded.jump_chain.is_empty() && loaded.forwards.is_empty());
        assert!(loaded.env.is_empty() && loaded.startup_command.is_none());
        assert!(!loaded.agent_forward && loaded.auth_order.is_none());
    }

    /// What goes to the sync server and what comes back is the same host,
    /// new fields included.
    #[test]
    fn the_v2_fields_are_in_the_sync_payload() {
        let store = store();
        let mut host = rich_host("web-1");
        store.repos().hosts().save(&mut host).unwrap();
        let payload = store
            .repos()
            .sync()
            .local_payload(RecordKind::Host, &host.id.to_string())
            .unwrap()
            .expect("payload");
        assert_eq!(payload["forwards"][0]["bind_port"], 5433);
        assert_eq!(payload["env"][0]["name"], "TERMRU_PROFILE");
        assert_eq!(payload["agent_forward"], true);
        assert_eq!(payload["startup_command"], "cd /srv && ls");

        // Another device applies that payload.
        let other = Store::open_in_memory().unwrap();
        let wire = termru_core::WireRecord {
            id: host.id.to_string(),
            kind: RecordKind::Host,
            rev: 1,
            updated_at: chrono::Utc::now(),
            deleted: false,
            payload: Some(payload),
        };
        other.repos().sync().apply_remote(&wire).unwrap();
        let received = other.repos().hosts().get(host.id).unwrap().unwrap();
        assert_eq!(received.forwards, host.forwards);
        assert_eq!(received.env, host.env);
        assert_eq!(received.auth_order, host.auth_order);
        assert!(received.agent_forward);
    }

    #[test]
    fn a_synced_host_may_name_a_jump_host_that_has_not_arrived() {
        let other = Store::open_in_memory().unwrap();
        let mut host = sample_host("web-1");
        host.jump_chain = vec![HostId::new()];
        let wire = termru_core::WireRecord {
            id: host.id.to_string(),
            kind: RecordKind::Host,
            rev: 1,
            updated_at: chrono::Utc::now(),
            deleted: false,
            payload: Some(serde_json::to_value(&host).unwrap()),
        };
        other.repos().sync().apply_remote(&wire).unwrap();
        assert!(other.repos().hosts().get(host.id).unwrap().is_some());
        // A local edit, though, has to leave a route that can be walked.
        let mut local = other.repos().hosts().get(host.id).unwrap().unwrap();
        let error = other
            .repos()
            .hosts()
            .save(&mut local)
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not exist"), "{error}");
    }

    #[test]
    fn a_local_save_refuses_cycles_and_missing_or_deleted_hops() {
        let store = store();
        let mut a = sample_host("a");
        let mut b = sample_host("b");
        store.repos().hosts().save(&mut a).unwrap();
        store.repos().hosts().save(&mut b).unwrap();
        b.jump_chain = vec![a.id];
        store.repos().hosts().save(&mut b).unwrap();

        // a -> b -> a
        a.jump_chain = vec![b.id];
        let error = store.repos().hosts().save(&mut a).unwrap_err().to_string();
        assert!(error.contains("cycle"), "{error}");
        // Self reference.
        a.jump_chain = vec![a.id];
        assert!(store.repos().hosts().save(&mut a).is_err());
        // Missing hop.
        a.jump_chain = vec![HostId::new()];
        let error = store.repos().hosts().save(&mut a).unwrap_err().to_string();
        assert!(error.contains("does not exist"), "{error}");
        // The refused saves wrote nothing.
        let stored = store.repos().hosts().get(a.id).unwrap().unwrap();
        assert!(stored.jump_chain.is_empty());

        // A deleted hop is as good as missing for a new save...
        let mut c = sample_host("c");
        c.jump_chain = vec![b.id];
        store.repos().hosts().soft_delete(b.id).unwrap();
        assert!(store.repos().hosts().save(&mut c).is_err());
    }

    /// Starring a host whose jump host was deleted must still work.
    #[test]
    fn favoriting_does_not_trip_over_a_deleted_jump_host() {
        let store = store();
        let mut jump = sample_host("bastion");
        store.repos().hosts().save(&mut jump).unwrap();
        let mut host = sample_host("web-1");
        host.jump_chain = vec![jump.id];
        store.repos().hosts().save(&mut host).unwrap();
        store.repos().hosts().soft_delete(jump.id).unwrap();
        store.repos().hosts().set_favorite(host.id, true).unwrap();
        assert!(
            store
                .repos()
                .hosts()
                .get(host.id)
                .unwrap()
                .unwrap()
                .favorite
        );
    }

    #[test]
    fn an_import_is_all_or_nothing() {
        let store = store();
        let identity = termru_core::Identity::new("imported key".into());
        let mut jump = sample_host("bastion");
        let mut target = sample_host("app");
        target.jump_chain = vec![jump.id];
        target.auth = AuthMethod::PrivateKey {
            identity_id: identity.id,
        };
        jump.auth = target.auth.clone();
        let mut batch = ImportBatch {
            identities: vec![ImportedIdentity {
                identity: identity.clone(),
                key_path: "/home/u/.ssh/id_ed25519".into(),
            }],
            hosts: vec![target.clone(), jump.clone()],
        };
        store.repos().hosts().import(&mut batch).unwrap();
        assert_eq!(store.repos().hosts().list().unwrap().len(), 2);
        assert_eq!(
            store
                .repos()
                .identities()
                .binding(identity.id)
                .unwrap()
                .as_deref(),
            Some("/home/u/.ssh/id_ed25519")
        );
        let saved = store.repos().hosts().get(target.id).unwrap().unwrap();
        assert_eq!(saved.jump_chain, vec![jump.id]);
        // Everything is queued for sync, as after any local edit.
        assert_eq!(store.repos().sync().pending().unwrap().len(), 3);

        // One bad host (a cycle) sinks the whole batch.
        let fresh = Store::open_in_memory().unwrap();
        let mut a = sample_host("a");
        let mut b = sample_host("b");
        a.jump_chain = vec![b.id];
        b.jump_chain = vec![a.id];
        let mut batch = ImportBatch {
            identities: vec![ImportedIdentity {
                identity: termru_core::Identity::new("k".into()),
                key_path: "/k".into(),
            }],
            hosts: vec![sample_host("fine"), a, b],
        };
        assert!(fresh.repos().hosts().import(&mut batch).is_err());
        assert!(fresh.repos().hosts().list().unwrap().is_empty());
        assert!(fresh.repos().identities().list().unwrap().is_empty());
        assert!(fresh.repos().sync().pending().unwrap().is_empty());
    }
}
