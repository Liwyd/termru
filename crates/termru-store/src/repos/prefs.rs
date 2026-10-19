//! Preferences repository: portable (synced) document + device-local kv.

use rusqlite::{Connection, OptionalExtension};

use termru_core::Preferences;
use termru_core::sync::{PREFERENCES_RECORD_ID, RecordKind};

use crate::Store;
use crate::error::StoreError;
use crate::repos::{enqueue_mutation, from_json, to_json};
use crate::util::{bool_to_int, now_ms};

pub struct Prefs<'a> {
    pub(crate) store: &'a Store,
}

const PORTABLE_KEY: &str = "portable";
const DEVICE_KEY: &str = "device";

fn read_row(
    conn: &Connection,
    key: &str,
) -> Result<Option<(String, Option<i64>, i64)>, StoreError> {
    conn.query_row(
        "SELECT value, server_rev, dirty FROM preferences WHERE key = ?1",
        [key],
        |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)?)),
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

fn write_row(
    conn: &Connection,
    key: &str,
    value: &str,
    server_rev: Option<i64>,
    dirty: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO preferences (key, value, updated_at, server_rev, dirty) VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, \
         server_rev = excluded.server_rev, dirty = excluded.dirty",
        rusqlite::params![key, value, now_ms(), server_rev, bool_to_int(dirty)],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

impl Prefs<'_> {
    /// Portable preferences (theme, terminal, shortcut overrides). Synced.
    pub fn portable(&self) -> Result<Preferences, StoreError> {
        self.store
            .with_conn(|conn| match read_row(conn, PORTABLE_KEY)? {
                Some((value, _, _)) => from_json(&value, "portable preferences"),
                None => Ok(Preferences::default()),
            })
    }

    pub fn set_portable(&self, prefs: &mut Preferences) -> Result<(), StoreError> {
        prefs.validate()?;
        self.store.with_tx(move |conn| {
            let value = to_json(&prefs, "portable preferences")?;
            write_row(conn, PORTABLE_KEY, &value, None, true)?;
            enqueue_mutation(conn, PREFERENCES_RECORD_ID, RecordKind::Preferences)?;
            Ok(())
        })
    }

    pub fn device_prefs(&self) -> Result<DevicePrefs, StoreError> {
        self.store
            .with_conn(|conn| match read_row(conn, DEVICE_KEY)? {
                Some((value, _, _)) => from_json(&value, "device preferences"),
                None => Ok(DevicePrefs::default()),
            })
    }

    pub fn set_device_prefs(&self, prefs: &DevicePrefs) -> Result<(), StoreError> {
        let value = to_json(prefs, "device preferences")?;
        self.store.with_conn(move |conn| {
            // Device preferences are never synced: written with dirty = 0 and
            // no queue entry.
            write_row(conn, DEVICE_KEY, &value, None, false)
        })
    }

    // ----- sync internals -----

    pub(crate) fn portable_snapshot(
        conn: &Connection,
    ) -> Result<Option<(Preferences, Option<i64>, bool)>, StoreError> {
        match read_row(conn, PORTABLE_KEY)? {
            Some((value, rev, dirty)) => Ok(Some((
                from_json(&value, "portable preferences")?,
                rev,
                dirty != 0,
            ))),
            None => Ok(None),
        }
    }

    pub(crate) fn apply_remote_portable(
        conn: &Connection,
        prefs: &Preferences,
        rev: i64,
    ) -> Result<(), StoreError> {
        let value = to_json(prefs, "portable preferences")?;
        write_row(conn, PORTABLE_KEY, &value, Some(rev), false)
    }

    pub(crate) fn set_server_rev(
        conn: &Connection,
        rev: Option<i64>,
        dirty: bool,
    ) -> Result<(), StoreError> {
        conn.execute(
            "UPDATE preferences SET server_rev = ?2, dirty = ?3 WHERE key = ?1",
            rusqlite::params![PORTABLE_KEY, rev, bool_to_int(dirty)],
        )
        .map_err(StoreError::Sqlite)?;
        Ok(())
    }
}

use crate::repos::DevicePrefs;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn portable_prefs_roundtrip_and_queue() {
        let store = Store::open_in_memory().unwrap();
        let mut prefs = Preferences::default();
        prefs.terminal.font_size = 16;
        store.repos().prefs().set_portable(&mut prefs).unwrap();
        let loaded = store.repos().prefs().portable().unwrap();
        assert_eq!(loaded.terminal.font_size, 16);
        let pending = store.repos().sync().pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].record_id, PREFERENCES_RECORD_ID);
    }

    #[test]
    fn device_prefs_never_queued() {
        let store = Store::open_in_memory().unwrap();
        let device = DevicePrefs {
            sync_url: "https://sync.example.com".into(),
            ..DevicePrefs::default()
        };
        store.repos().prefs().set_device_prefs(&device).unwrap();
        assert_eq!(
            store.repos().prefs().device_prefs().unwrap().sync_url,
            device.sync_url
        );
        assert!(store.repos().sync().pending().unwrap().is_empty());
    }

    #[test]
    fn invalid_portable_prefs_rejected() {
        let store = Store::open_in_memory().unwrap();
        let mut prefs = Preferences::default();
        prefs.terminal.font_size = 1;
        assert!(store.repos().prefs().set_portable(&mut prefs).is_err());
    }
}
