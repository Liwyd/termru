//! Workspace layout repository (device-local documents).

use rusqlite::{OptionalExtension, Row};

use termru_core::ids::WorkspaceId;
use termru_core::workspace::WorkspaceDoc;

use crate::Store;
use crate::error::StoreError;
use crate::repos::to_json;
use crate::util::{datetime_from_ms, datetime_to_ms, now_ms};

pub struct Workspaces<'a> {
    pub(crate) store: &'a Store,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub name: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn row_summary(row: &Row<'_>) -> rusqlite::Result<WorkspaceSummary> {
    let id: String = row.get(0)?;
    Ok(WorkspaceSummary {
        id: id.parse().unwrap_or_default(),
        name: row.get(1)?,
        updated_at: datetime_from_ms(row.get(2)?),
    })
}

impl Workspaces<'_> {
    pub fn list(&self) -> Result<Vec<WorkspaceSummary>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, updated_at FROM workspaces \
                     ORDER BY updated_at DESC, id",
                )
                .map_err(StoreError::Sqlite)?;
            let rows = stmt
                .query_map([], row_summary)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::Sqlite)?;
            Ok(rows)
        })
    }

    pub fn get(&self, id: WorkspaceId) -> Result<Option<WorkspaceDoc>, StoreError> {
        self.store.with_conn(|conn| {
            let doc: Option<String> = conn
                .query_row(
                    "SELECT doc FROM workspaces WHERE id = ?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            match doc {
                Some(json) => {
                    let doc: WorkspaceDoc = crate::repos::from_json(&json, "workspace document")?;
                    doc.validate()?;
                    Ok(Some(doc))
                }
                None => Ok(None),
            }
        })
    }

    /// Saves a workspace document. Validation runs before any write so a
    /// corrupt or hostile document never lands in the database.
    pub fn save(&self, id: WorkspaceId, name: &str, doc: &WorkspaceDoc) -> Result<(), StoreError> {
        termru_core::validate::validate_display_name(
            "workspace name",
            name,
            termru_core::validate::MAX_NAME_LEN,
        )?;
        doc.validate()?;
        let doc_json = to_json(doc, "workspace document")?;
        let key = id.to_string();
        let name = name.to_string();
        self.store.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO workspaces (id, name, doc, updated_at) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, doc = excluded.doc, \
                 updated_at = excluded.updated_at",
                rusqlite::params![key, name, doc_json, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            Ok(())
        })
    }

    pub fn delete(&self, id: WorkspaceId) -> Result<(), StoreError> {
        self.store.with_conn(move |conn| {
            conn.execute("DELETE FROM workspaces WHERE id = ?1", [id.to_string()])
                .map_err(StoreError::Sqlite)?;
            Ok(())
        })
    }

    pub fn updated_at(
        &self,
        id: WorkspaceId,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StoreError> {
        self.store.with_conn(move |conn| {
            let ms: Option<i64> = conn
                .query_row(
                    "SELECT updated_at FROM workspaces WHERE id = ?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            Ok(ms.map(datetime_from_ms))
        })
    }

    #[allow(dead_code)]
    pub(crate) fn doc_json_for_compare(
        &self,
        id: WorkspaceId,
    ) -> Result<Option<String>, StoreError> {
        self.store.with_conn(move |conn| {
            conn.query_row(
                "SELECT doc FROM workspaces WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)
        })
    }

    pub(crate) fn _ts(dt: &chrono::DateTime<chrono::Utc>) -> i64 {
        datetime_to_ms(dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use termru_core::workspace::{PaneNode, TabLayout, WindowLayout};

    #[test]
    fn workspace_roundtrip_and_validation() {
        let store = Store::open_in_memory().unwrap();
        let id = WorkspaceId::new();
        let mut doc = WorkspaceDoc::default();
        doc.windows.push(WindowLayout {
            id: termru_core::ids::WindowId::new(),
            tabs: vec![TabLayout {
                id: termru_core::ids::TabId::new(),
                title: None,
                root: PaneNode::Leaf {
                    pane: termru_core::ids::PaneId::new(),
                    host_id: None,
                },
                active_pane: None,
            }],
            active_tab: None,
            bounds: None,
        });
        store
            .repos()
            .workspaces()
            .save(id, "default", &doc)
            .unwrap();
        let loaded = store.repos().workspaces().get(id).unwrap().unwrap();
        assert_eq!(loaded.windows.len(), 1);
        assert_eq!(store.repos().workspaces().list().unwrap().len(), 1);

        // Invalid doc rejected.
        let bad = WorkspaceDoc {
            version: 99,
            ..WorkspaceDoc::default()
        };
        assert!(
            store
                .repos()
                .workspaces()
                .save(WorkspaceId::new(), "x", &bad)
                .is_err()
        );

        store.repos().workspaces().delete(id).unwrap();
        assert!(store.repos().workspaces().get(id).unwrap().is_none());
    }
}
