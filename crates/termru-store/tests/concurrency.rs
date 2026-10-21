//! Concurrent access to the local database.
//!
//! The app itself is single-process and serializes its writes behind one
//! connection, but the database file is also reachable from other processes
//! (a backup tool, a second instance). These tests prove that two
//! independent connections can work on the same file without corrupting it
//! and without surfacing a `SQLITE_BUSY` error.

use std::sync::Arc;
use std::thread;

use termru_core::Host;
use termru_store::Store;

fn host(name: &str) -> Host {
    Host::new(
        name.to_string(),
        format!("{name}.example.com"),
        "deploy".to_string(),
    )
}

#[test]
fn two_connections_to_the_same_file_both_work() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("termru.sqlite");

    let first = Store::open(&path).expect("open the first store");
    let second = Store::open(&path).expect("open the second store");

    let mut host = host("web-1");
    first.repos().hosts().save(&mut host).expect("write");

    // The second connection sees the write (WAL mode: the reader is not
    // blocked by the writer).
    let listed = second.repos().hosts().list().expect("read");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "web-1");
}

#[test]
fn interleaved_writes_from_two_connections_keep_every_row() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("termru.sqlite");

    let first = Store::open(&path).expect("open the first store");
    let second = Store::open(&path).expect("open the second store");

    // Both connections write at the same time.
    let handles: Vec<_> = [Arc::new(first), Arc::new(second)]
        .into_iter()
        .enumerate()
        .map(|(index, store)| {
            thread::spawn(move || {
                for round in 0..20 {
                    let mut entry = host(&format!("host-{index}-{round}"));
                    store.repos().hosts().save(&mut entry).expect("write");
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("the writer thread must not panic");
    }

    // Every write is there, and the file is intact.
    let reader = Store::open(&path).expect("open the reader");
    let listed = reader.repos().hosts().list().expect("read");
    assert_eq!(listed.len(), 40, "no write may be lost");
    reader
        .integrity_check()
        .expect("the database must be intact");
}

#[test]
fn a_second_store_sees_the_migrations_of_the_first() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("termru.sqlite");

    let first = Store::open(&path).expect("open the first store");
    let mut host = host("web-1");
    first.repos().hosts().save(&mut host).expect("write");

    // The second store brings the file to the current schema version; it
    // must not fail and must not lose the row.
    let second = Store::open(&path).expect("open the second store");
    assert_eq!(second.repos().hosts().list().expect("read").len(), 1);
    second
        .integrity_check()
        .expect("the database must be intact");
}
