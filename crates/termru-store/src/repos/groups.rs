//! Group (folder) repository with hierarchy validation and safe deletion.

use rusqlite::{Connection, OptionalExtension, Row};

use termru_core::Group;
use termru_core::sync::RecordKind;
use termru_core::validate::MAX_GROUP_DEPTH;

use crate::Store;
use crate::error::StoreError;
use crate::repos::enqueue_mutation;
use crate::util::{datetime_from_ms, datetime_to_ms, now_ms};

pub struct Groups<'a> {
    pub(crate) store: &'a Store,
}

const GROUP_COLUMNS: &str =
    "id, name, parent_id, sort_order, created_at, updated_at, deleted, server_rev, dirty";

fn row_to_group(row: &Row<'_>) -> rusqlite::Result<Group> {
    let id: String = row.get(0)?;
    let parent: Option<String> = row.get(2)?;
    Ok(Group {
        id: id.parse().unwrap_or_default(),
        name: row.get(1)?,
        parent_id: parent.and_then(|p| p.parse().ok()),
        sort_order: row.get(3)?,
        created_at: datetime_from_ms(row.get(4)?),
        updated_at: datetime_from_ms(row.get(5)?),
        deleted: row.get::<_, i64>(6)? != 0,
        server_rev: row.get(7)?,
        dirty: row.get::<_, i64>(8)? != 0,
    })
}

pub(crate) fn get_row(conn: &Connection, id: &str) -> Result<Option<Group>, StoreError> {
    conn.query_row(
        &format!("SELECT {GROUP_COLUMNS} FROM groups WHERE id = ?1"),
        [id],
        row_to_group,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

pub(crate) fn upsert_group_row(conn: &Connection, group: &Group) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO groups (id, name, parent_id, sort_order, created_at, updated_at, deleted, server_rev, dirty) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(id) DO UPDATE SET \
           name = excluded.name, parent_id = excluded.parent_id, sort_order = excluded.sort_order, \
           created_at = excluded.created_at, updated_at = excluded.updated_at, \
           deleted = excluded.deleted, server_rev = excluded.server_rev, dirty = excluded.dirty",
        rusqlite::params![
            group.id.to_string(),
            group.name,
            group.parent_id.map(|p| p.to_string()),
            group.sort_order,
            datetime_to_ms(&group.created_at),
            datetime_to_ms(&group.updated_at),
            i64::from(group.deleted),
            group.server_rev,
            i64::from(group.dirty),
        ],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Computes the depth `parent` would have if adopted; rejects cycles and
/// excessive depth. Depth of a root is 0.
fn validate_parent(
    conn: &Connection,
    parent: Option<termru_core::ids::GroupId>,
    self_id: termru_core::ids::GroupId,
) -> Result<(), StoreError> {
    let Some(mut next) = parent else {
        return Ok(());
    };
    if next == self_id {
        return Err(StoreError::Validation(termru_core::CoreError::validation(
            "a group cannot be its own parent",
        )));
    }
    let mut depth = 1usize;
    let mut seen = std::collections::HashSet::new();
    seen.insert(self_id);
    while depth <= MAX_GROUP_DEPTH {
        if !seen.insert(next) {
            return Err(StoreError::Validation(termru_core::CoreError::validation(
                "group hierarchy contains a cycle",
            )));
        }
        let parent_of: Option<Option<String>> = conn
            .query_row(
                "SELECT parent_id FROM groups WHERE id = ?1",
                [next.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)?;
        let Some(parent_of) = parent_of else {
            // Parent no longer exists: treat as root-level adopt.
            return Ok(());
        };
        match parent_of.and_then(|p| p.parse().ok()) {
            None => return Ok(()),
            Some(p) if p == self_id => {
                return Err(StoreError::Validation(termru_core::CoreError::validation(
                    "group hierarchy contains a cycle",
                )));
            }
            Some(p) => {
                next = p;
                depth += 1;
            }
        }
    }
    Err(StoreError::Validation(termru_core::CoreError::validation(
        format!("group nesting exceeds the maximum depth of {MAX_GROUP_DEPTH}"),
    )))
}

impl Groups<'_> {
    pub fn list(&self) -> Result<Vec<Group>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {GROUP_COLUMNS} FROM groups WHERE deleted = 0 \
                     ORDER BY sort_order, name COLLATE NOCASE, id"
                ))
                .map_err(StoreError::Sqlite)?;
            let groups = stmt
                .query_map([], row_to_group)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::Sqlite)?;
            Ok(groups)
        })
    }

    pub fn get(&self, id: termru_core::ids::GroupId) -> Result<Option<Group>, StoreError> {
        self.store.with_conn(|conn| {
            let group = get_row(conn, &id.to_string())?;
            Ok(group.filter(|g| !g.deleted))
        })
    }

    pub fn save(&self, group: &mut Group) -> Result<(), StoreError> {
        group.validate()?;
        group.updated_at = chrono::Utc::now();
        // A local edit marks the row dirty (see the host repository).
        group.dirty = true;
        let id = group.id;
        self.store.with_tx(move |conn| {
            validate_parent(conn, group.parent_id, id)?;
            upsert_group_row(conn, group)?;
            enqueue_mutation(conn, &id.to_string(), RecordKind::Group)?;
            Ok(())
        })
    }

    /// Soft-deletes a group. Hosts inside it move to the group's parent (or to
    /// the root) and child groups are re-parented accordingly, so no host is
    /// ever orphaned or silently lost. Affected rows are marked dirty so the
    /// re-parenting propagates through sync.
    pub fn soft_delete(&self, id: termru_core::ids::GroupId) -> Result<(), StoreError> {
        let key = id.to_string();
        self.store.with_tx(move |conn| {
            let group = get_row(conn, &key)?.ok_or(StoreError::NotFound("group"))?;
            let new_parent = group.parent_id.map(|p| p.to_string());

            // Capture affected rows before mutating so enqueues are exact.
            let moved_hosts: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT id FROM hosts WHERE group_id = ?1")
                    .map_err(StoreError::Sqlite)?;
                stmt.query_map([&key], |r| r.get(0))
                    .map_err(StoreError::Sqlite)?
                    .collect::<Result<_, _>>()
                    .map_err(StoreError::Sqlite)?
            };
            let moved_children: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT id FROM groups WHERE parent_id = ?1 AND id != ?1")
                    .map_err(StoreError::Sqlite)?;
                stmt.query_map([&key], |r| r.get(0))
                    .map_err(StoreError::Sqlite)?
                    .collect::<Result<_, _>>()
                    .map_err(StoreError::Sqlite)?
            };

            conn.execute(
                "UPDATE hosts SET group_id = ?2, dirty = 1, updated_at = ?3 WHERE group_id = ?1",
                rusqlite::params![key, new_parent, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            conn.execute(
                "UPDATE groups SET parent_id = ?2, dirty = 1, updated_at = ?3 WHERE parent_id = ?1",
                rusqlite::params![key, new_parent, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            conn.execute(
                "UPDATE groups SET deleted = 1, dirty = 1, updated_at = ?2 WHERE id = ?1",
                rusqlite::params![key, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;

            for host_id in &moved_hosts {
                enqueue_mutation(conn, host_id, RecordKind::Host)?;
            }
            for child_id in &moved_children {
                enqueue_mutation(conn, child_id, RecordKind::Group)?;
            }
            enqueue_mutation(conn, &key, RecordKind::Group)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn hierarchy_rules_and_cycle_detection() {
        let store = Store::open_in_memory().unwrap();
        let mut root = Group::new("prod".into());
        store.repos().groups().save(&mut root).unwrap();
        let mut child = Group::new("eu".into());
        child.parent_id = Some(root.id);
        store.repos().groups().save(&mut child).unwrap();

        // Self-parent rejected by validation.
        let mut bad = Group::new("loop".into());
        bad.parent_id = Some(bad.id);
        assert!(store.repos().groups().save(&mut bad).is_err());

        // Direct cycle (root -> child -> root) rejected by graph check.
        let mut root2 = root.clone();
        root2.parent_id = Some(child.id);
        assert!(store.repos().groups().save(&mut root2).is_err());
    }

    /// A folder cannot move under any of its descendants, however deep: the
    /// folders page offers only legal parents, and this is the check that
    /// still holds when something else (a sync, an old client) asks.
    #[test]
    fn a_folder_cannot_move_under_a_deeper_descendant() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        let mut a = Group::new("a".into());
        repos.groups().save(&mut a).unwrap();
        let mut b = Group::new("b".into());
        b.parent_id = Some(a.id);
        repos.groups().save(&mut b).unwrap();
        let mut c = Group::new("c".into());
        c.parent_id = Some(b.id);
        repos.groups().save(&mut c).unwrap();
        let mut d = Group::new("d".into());
        d.parent_id = Some(c.id);
        repos.groups().save(&mut d).unwrap();

        // a -> d (a grandchild's child) and a -> c (a grandchild) are cycles.
        for descendant in [c.id, d.id, b.id] {
            let mut moved = a.clone();
            moved.parent_id = Some(descendant);
            let error = repos.groups().save(&mut moved).unwrap_err();
            assert!(error.to_string().contains("cycle"), "{error}");
        }
        // Nothing changed: a is still a root.
        assert_eq!(repos.groups().get(a.id).unwrap().unwrap().parent_id, None);

        // A middle folder cannot move under its own child either.
        let mut moved = b.clone();
        moved.parent_id = Some(d.id);
        assert!(repos.groups().save(&mut moved).is_err());

        // Moving a branch to a sibling branch is fine.
        let mut other = Group::new("other".into());
        repos.groups().save(&mut other).unwrap();
        let mut moved = b.clone();
        moved.parent_id = Some(other.id);
        repos.groups().save(&mut moved).unwrap();
        assert_eq!(
            repos.groups().get(b.id).unwrap().unwrap().parent_id,
            Some(other.id)
        );
    }

    #[test]
    fn nesting_deeper_than_the_limit_is_refused() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        let mut parent: Option<termru_core::ids::GroupId> = None;
        let mut last = None;
        // A top-level folder has depth 0; the deepest legal one has depth
        // MAX_GROUP_DEPTH, so that many parents sit above it.
        for level in 0..=MAX_GROUP_DEPTH {
            let mut group = Group::new(format!("level-{level}"));
            group.parent_id = parent;
            repos.groups().save(&mut group).unwrap();
            parent = Some(group.id);
            last = Some(group);
        }
        // The chain is as deep as it may be: one level more is refused.
        let mut too_deep = Group::new("too-deep".into());
        too_deep.parent_id = Some(last.unwrap().id);
        let error = repos.groups().save(&mut too_deep).unwrap_err();
        assert!(error.to_string().contains("maximum depth"), "{error}");
    }

    #[test]
    fn deleting_group_moves_hosts_to_parent() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        let mut parent = Group::new("root-group".into());
        repos.groups().save(&mut parent).unwrap();
        let mut child = Group::new("child".into());
        child.parent_id = Some(parent.id);
        repos.groups().save(&mut child).unwrap();
        let mut host = termru_core::Host::new("h1".into(), "h1.example.com".into(), "u".into());
        host.group_id = Some(child.id);
        repos.hosts().save(&mut host).unwrap();

        repos.groups().soft_delete(child.id).unwrap();
        let listed = repos.hosts().list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].group_id, Some(parent.id));
        assert!(listed[0].dirty);
        assert!(
            repos
                .groups()
                .list()
                .unwrap()
                .iter()
                .all(|g| g.id != child.id)
        );
    }

    #[test]
    fn deleting_root_group_moves_hosts_to_root() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        let mut g = Group::new("only".into());
        repos.groups().save(&mut g).unwrap();
        let mut host = termru_core::Host::new("h1".into(), "h1.example.com".into(), "u".into());
        host.group_id = Some(g.id);
        repos.hosts().save(&mut host).unwrap();
        repos.groups().soft_delete(g.id).unwrap();
        assert_eq!(repos.hosts().list().unwrap()[0].group_id, None);
    }

    #[test]
    fn group_save_is_queued_for_sync() {
        let store = Store::open_in_memory().unwrap();
        let mut g = Group::new("ops".into());
        store.repos().groups().save(&mut g).unwrap();
        let pending = store.repos().sync().pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, RecordKind::Group);
    }
}
