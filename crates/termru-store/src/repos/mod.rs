//! Repository layer. Each module exposes operations that take `&Connection`
//! so they compose inside transactions; `Repos` provides the public grouped
//! API that acquires the store lock (or an open transaction) as needed.

pub mod conflicts;
pub mod groups;
pub mod hosts;
pub mod identities;
pub mod prefs;
pub mod recents;
pub mod sync;
pub mod tags;
pub mod workspaces;

use rusqlite::Connection;

use crate::Store;
use crate::error::StoreError;

/// Entry point: `store.repos().hosts().list()?`
pub struct Repos<'a> {
    pub(crate) store: &'a Store,
}

impl Repos<'_> {
    pub fn hosts(&self) -> hosts::Hosts<'_> {
        hosts::Hosts { store: self.store }
    }
    pub fn groups(&self) -> groups::Groups<'_> {
        groups::Groups { store: self.store }
    }
    pub fn tags(&self) -> tags::Tags<'_> {
        tags::Tags { store: self.store }
    }
    pub fn identities(&self) -> identities::Identities<'_> {
        identities::Identities { store: self.store }
    }
    pub fn prefs(&self) -> prefs::Prefs<'_> {
        prefs::Prefs { store: self.store }
    }
    pub fn workspaces(&self) -> workspaces::Workspaces<'_> {
        workspaces::Workspaces { store: self.store }
    }
    pub fn recents(&self) -> recents::Recents<'_> {
        recents::Recents { store: self.store }
    }
    pub fn sync(&self) -> sync::Sync<'_> {
        sync::Sync { store: self.store }
    }
    pub fn conflicts(&self) -> conflicts::Conflicts<'_> {
        conflicts::Conflicts { store: self.store }
    }
}

/// Device-local key/value settings (sync endpoint, device name, geometry…).
/// Never synced. `T` must be serializable; values are stored as JSON.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct DevicePrefs {
    /// Cloud sync endpoint URL (empty = sync disabled).
    pub sync_url: String,
    /// Display name for this device during enrollment.
    pub device_name: String,
    /// The account email of the sync server (not a secret).
    pub sync_email: String,
    /// The stable id of this installation, used to enroll the device.
    pub sync_device_id: String,
    /// Active workspace id.
    pub active_workspace: Option<termru_core::ids::WorkspaceId>,
    /// Last successful sync cursor (informational mirror of sync_meta).
    pub last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub(crate) fn enqueue_mutation(
    conn: &Connection,
    record_id: &str,
    kind: termru_core::RecordKind,
) -> Result<(), StoreError> {
    // Coalesce: one pending entry per record, payload is materialized at
    // push time from the live row. A fresh mutation_id per enqueue keeps
    // retries of the *current* state idempotent while content-equality
    // acceptance on the server covers lost-acknowledgement races.
    conn.execute("DELETE FROM sync_queue WHERE record_id = ?1", [record_id])
        .map_err(StoreError::Sqlite)?;
    conn.execute(
        "INSERT INTO sync_queue (mutation_id, record_id, kind, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            record_id,
            kind.as_str(),
            crate::util::now_ms(),
        ],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

pub(crate) fn remove_queued(conn: &Connection, record_id: &str) -> Result<(), StoreError> {
    conn.execute("DELETE FROM sync_queue WHERE record_id = ?1", [record_id])
        .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Decodes a stored JSON row field with context for error messages.
pub(crate) fn from_json<T: serde::de::DeserializeOwned>(
    value: &str,
    context: &str,
) -> Result<T, StoreError> {
    serde_json::from_str(value)
        .map_err(|e| StoreError::migration(format!("corrupt {context}: {e}")))
}

pub(crate) fn to_json<T: serde::Serialize>(value: &T, context: &str) -> Result<String, StoreError> {
    serde_json::to_string(value)
        .map_err(|e| StoreError::migration(format!("failed to serialize {context}: {e}")))
}
