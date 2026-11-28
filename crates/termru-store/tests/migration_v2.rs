//! Opening a database written before data model v2.
//!
//! The fixture (`fixtures/schema_v1.sql`) is a frozen copy of the v1 schema:
//! the database is built the way the previous release built it, filled with
//! rows through plain SQL, and then opened with the current code.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "setup failures must panic loudly in tests"
)]

use rusqlite::Connection;
use termru_core::{AuthMethod, Forward, ForwardKind, Host};
use termru_store::Store;

const SCHEMA_V1: &str = include_str!("fixtures/schema_v1.sql");
const HOST_A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const HOST_B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const IDENTITY: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const TAG: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";

/// A v1 database file with two hosts (one with a key, a tag and a pending
/// sync mutation), as the previous release left it.
fn v1_database(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("termru.sqlite");
    let conn = Connection::open(&path).expect("create the v1 file");
    conn.execute_batch(SCHEMA_V1).expect("build schema v1");
    conn.execute_batch(&format!(
        "INSERT INTO identities (id, name, created_at, updated_at) \
           VALUES ('{IDENTITY}', 'work key', 1000, 1000);
         INSERT INTO identity_bindings (identity_id, key_path, updated_at) \
           VALUES ('{IDENTITY}', '/home/u/.ssh/id_work', 1000);
         INSERT INTO tags (id, name, created_at, updated_at) VALUES ('{TAG}', 'prod', 1000, 1000);
         INSERT INTO hosts (id, name, address, port, username, auth, favorite, term, \
           keepalive_seconds, sort_order, created_at, updated_at, server_rev, dirty) \
           VALUES ('{HOST_A}', 'web-1', 'web-1.example.com', 22, 'deploy', \
           '{{\"type\":\"private_key\",\"identity_id\":\"{IDENTITY}\"}}', 1, 'xterm-256color', \
           30, 0, 1000, 2000, 5, 0);
         INSERT INTO hosts (id, name, address, port, username, auth, term, \
           created_at, updated_at, dirty) \
           VALUES ('{HOST_B}', 'db-1', 'fe80::1', 2200, 'postgres', '{{\"type\":\"password\"}}', \
           'xterm', 1000, 3000, 1);
         INSERT INTO host_tags (host_id, tag_id) VALUES ('{HOST_A}', '{TAG}');
         INSERT INTO sync_queue (mutation_id, record_id, kind, created_at) \
           VALUES ('eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee', '{HOST_B}', 'host', 3000);"
    ))
    .expect("fill the v1 database");
    path
}

fn schema_version(path: &std::path::Path) -> u32 {
    Connection::open(path)
        .expect("reopen")
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("version")
}

#[test]
fn opening_a_v1_database_migrates_it_in_place() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = v1_database(dir.path());
    assert_eq!(schema_version(&path), 1);

    let store = Store::open(&path).expect("open migrates");
    assert_eq!(schema_version(&path), 2);

    let hosts = store.repos().hosts().list().expect("list");
    assert_eq!(hosts.len(), 2);
    let web = hosts
        .iter()
        .find(|host| host.name == "web-1")
        .expect("web-1");
    assert_eq!(web.address, "web-1.example.com");
    assert!(web.favorite);
    assert_eq!(web.keepalive_seconds, 30);
    assert_eq!(web.server_rev, Some(5));
    assert!(matches!(web.auth, AuthMethod::PrivateKey { .. }));
    assert_eq!(web.tags.len(), 1, "the tag link survives");
    // None of the new features is on.
    for host in &hosts {
        assert!(host.jump_chain.is_empty() && host.forwards.is_empty() && host.env.is_empty());
        assert!(host.startup_command.is_none() && host.auth_order.is_none());
        assert!(!host.agent_forward);
        host.validate().expect("a migrated host is valid");
    }
    // The IPv6 address of the second host reads back as stored.
    assert!(hosts.iter().any(|host| host.address == "fe80::1"));

    // The key binding and the pending mutation are untouched.
    let identity = web.auth.clone();
    let AuthMethod::PrivateKey { identity_id } = identity else {
        panic!("web-1 uses a key");
    };
    assert_eq!(
        store
            .repos()
            .identities()
            .binding(identity_id)
            .expect("binding")
            .as_deref(),
        Some("/home/u/.ssh/id_work")
    );
    let pending = store.repos().sync().pending().expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].record_id, HOST_B);

    // What a migrated host puts on the wire is what v1 put there.
    let payload = store
        .repos()
        .sync()
        .local_payload(termru_core::RecordKind::Host, HOST_A)
        .expect("payload")
        .expect("exists");
    for field in [
        "jump_chain",
        "forwards",
        "env",
        "startup_command",
        "agent_forward",
        "auth_order",
    ] {
        assert!(payload.get(field).is_none(), "{field} stays off the wire");
    }
}

#[test]
fn a_migrated_host_can_use_the_new_features_and_reopen() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = v1_database(dir.path());
    {
        let store = Store::open(&path).expect("open migrates");
        let hosts = store.repos().hosts().list().expect("list");
        let mut web: Host = hosts
            .into_iter()
            .find(|host| host.name == "web-1")
            .expect("web-1");
        let mut jump = store
            .repos()
            .hosts()
            .list()
            .expect("list")
            .into_iter()
            .find(|host| host.name == "db-1")
            .expect("db-1");
        web.jump_chain = vec![jump.id];
        web.forwards = vec![Forward {
            kind: ForwardKind::Local,
            bind_address: None,
            bind_port: 8080,
            target_host: Some("localhost".into()),
            target_port: Some(80),
            enabled: true,
        }];
        web.agent_forward = true;
        store
            .repos()
            .hosts()
            .save(&mut web)
            .expect("save with v2 fields");
        jump.startup_command = Some("uptime".into());
        store
            .repos()
            .hosts()
            .save(&mut jump)
            .expect("save the jump host");
    }
    // A second open runs no migration and still reads everything.
    let store = Store::open(&path).expect("reopen");
    assert_eq!(schema_version(&path), 2);
    let web = store
        .repos()
        .hosts()
        .list()
        .expect("list")
        .into_iter()
        .find(|host| host.name == "web-1")
        .expect("web-1");
    assert_eq!(web.jump_chain.len(), 1);
    assert_eq!(web.forwards[0].bind_port, 8080);
    assert!(web.agent_forward);
}

/// A database from a newer release must not be opened (and silently
/// downgraded) by this one.
#[test]
fn a_database_from_the_future_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = v1_database(dir.path());
    drop(Store::open(&path).expect("open migrates"));
    Connection::open(&path)
        .expect("reopen")
        .execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (3, 'later', 0)",
            [],
        )
        .expect("pretend a newer release ran");
    assert!(Store::open(&path).is_err());
}
