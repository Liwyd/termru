//! Recently-used host tracking (device-local).

use rusqlite::{OptionalExtension, Row};

use termru_core::Host;
use termru_core::ids::HostId;

use crate::Store;
use crate::error::StoreError;
use crate::util::now_ms;

pub struct Recents<'a> {
    pub(crate) store: &'a Store,
}

const MAX_RECENTS: usize = 50;

fn row_recent(row: &Row<'_>) -> rusqlite::Result<(Host, i64)> {
    Ok((super::hosts::row_to_host(row)?, row.get(17)?))
}

impl Recents<'_> {
    /// Most recently used hosts (visible only), limited to [`MAX_RECENTS`].
    pub fn list(&self) -> Result<Vec<Host>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {cols}, r.last_used_at FROM recents r \
                     JOIN hosts h ON h.id = r.host_id \
                     WHERE h.deleted = 0 \
                     ORDER BY r.last_used_at DESC, r.host_id LIMIT {MAX_RECENTS}",
                    cols = super::hosts::HOST_COLUMNS,
                ))
                .map_err(StoreError::Sqlite)?;
            let mut hosts = stmt
                .query_map([], row_recent)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<(Host, i64)>, _>>()
                .map_err(StoreError::Sqlite)?
                .into_iter()
                .map(|(h, _)| h)
                .collect::<Vec<_>>();
            super::hosts::fetch_tags(conn, &mut hosts)?;
            Ok(hosts)
        })
    }

    pub(crate) fn touch(&self, host_id: HostId) -> Result<(), StoreError> {
        let key = host_id.to_string();
        self.store.with_conn(move |conn| {
            let exists: i64 = conn
                .query_row("SELECT 1 FROM hosts WHERE id = ?1", [&key], |r| r.get(0))
                .optional()
                .map_err(StoreError::Sqlite)?
                .unwrap_or(0);
            if exists == 0 {
                return Err(StoreError::NotFound("host"));
            }
            conn.execute(
                "INSERT INTO recents (host_id, last_used_at, use_count) VALUES (?1, ?2, 1) \
                 ON CONFLICT(host_id) DO UPDATE SET last_used_at = excluded.last_used_at, \
                 use_count = use_count + 1",
                rusqlite::params![key, now_ms()],
            )
            .map_err(StoreError::Sqlite)?;
            // Keep the table bounded.
            conn.execute(
                &format!(
                    "DELETE FROM recents WHERE host_id NOT IN (\
                     SELECT host_id FROM recents ORDER BY last_used_at DESC, host_id \
                     LIMIT {MAX_RECENTS})"
                ),
                [],
            )
            .map_err(StoreError::Sqlite)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use termru_core::Host;

    #[test]
    fn recents_order_and_bound() {
        let store = Store::open_in_memory().unwrap();
        let repos = store.repos();
        for i in 0..5 {
            let mut h = Host::new(format!("h{i}"), format!("h{i}.example.com"), "u".into());
            repos.hosts().save(&mut h).unwrap();
            repos.recents().touch(h.id).unwrap();
        }
        let recents = repos.recents().list().unwrap();
        assert_eq!(recents.len(), 5);
        assert_eq!(recents[0].name, "h4");
    }
}
