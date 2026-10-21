//! Performance measurements of the local store.
//!
//! The numbers are printed with `--nocapture` and recorded in
//! `docs/performance.md`. They are *measurements*, not thresholds: the test
//! only fails when something is obviously broken (a query that returns the
//! wrong number of rows), never because a machine is slower than another.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "harness setup failures must panic loudly in tests"
)]

use std::time::{Duration, Instant};

use termru_core::Host;
use termru_store::Store;

const HOSTS: usize = 10_000;

fn seed(store: &Store, count: usize) -> Duration {
    let start = Instant::now();
    for index in 0..count {
        let mut host = Host::new(
            format!("host-{index:05}"),
            format!("host-{index:05}.example.com"),
            "deploy".to_string(),
        );
        host.description = Some("a seeded host for the measurement".to_string());
        store
            .repos()
            .hosts()
            .save(&mut host)
            .expect("seed the host");
    }
    start.elapsed()
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[test]
fn measures_the_store_with_ten_thousand_hosts() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let store = Store::open(&dir.path().join("perf.sqlite")).expect("open the store");

    // --- writing ---------------------------------------------------------
    let write = seed(&store, HOSTS);
    println!(
        "write: {HOSTS} hosts in {:.0} ms ({:.0} hosts/s)",
        millis(write),
        HOSTS as f64 / write.as_secs_f64()
    );

    // --- listing ---------------------------------------------------------
    let start = Instant::now();
    let listed = store.repos().hosts().list().expect("list the hosts");
    let list = start.elapsed();
    assert_eq!(listed.len(), HOSTS);
    println!("list: {HOSTS} hosts in {:.0} ms", millis(list));

    // --- reading one host ------------------------------------------------
    let id = listed[0].id;
    let start = Instant::now();
    for _ in 0..1000 {
        store.repos().hosts().get(id).expect("get the host");
    }
    let get = start.elapsed() / 1000;
    println!("get: one host in {:.3} ms", millis(get));

    // --- searching -------------------------------------------------------
    let start = Instant::now();
    let found = store.repos().hosts().search("host-07777").expect("search");
    let search = start.elapsed();
    assert!(!found.is_empty());
    println!("search: {HOSTS} hosts in {:.0} ms", millis(search));

    // --- the sync queue -------------------------------------------------
    // Every seeded host is queued for the sync, so the push payload has to
    // be materialized for all of them.
    let pending = store.repos().sync().pending().expect("pending");
    assert_eq!(pending.len(), HOSTS);
    let start = Instant::now();
    let push = store.repos().sync().build_push().expect("build the push");
    let build = start.elapsed();
    assert_eq!(push.len(), HOSTS);
    println!(
        "sync: materialized {HOSTS} mutations in {:.0} ms",
        millis(build)
    );

    // --- backup ---------------------------------------------------------
    let start = Instant::now();
    let backup = dir.path().join("backup.sqlite");
    termru_store::backup_to(&store, &backup).expect("write the backup");
    let backup_time = start.elapsed();
    let size = std::fs::metadata(&backup).expect("the backup").len();
    println!(
        "backup: {HOSTS} hosts in {:.0} ms ({:.1} MiB)",
        millis(backup_time),
        size as f64 / (1024.0 * 1024.0)
    );

    // --- the database file ----------------------------------------------
    let file = std::fs::metadata(dir.path().join("perf.sqlite")).expect("the database");
    println!(
        "database: {:.1} MiB on disk",
        file.len() as f64 / (1024.0 * 1024.0)
    );
}
