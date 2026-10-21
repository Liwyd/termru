//! Embeds the SQL migration files into the binary.
//!
//! The migration set is versioned with the server binary, so a container image
//! never needs a separate migration directory at runtime. The generated table
//! is `MIGRATIONS: &[(version, name, sql)]`, ordered by version.

// A build script has no way to report an error other than panicking, so the
// `expect_used` lint does not apply here.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::process::Command;

/// Emits `MIGRATIONS` and re-runs the build script when a migration changes.
fn main() {
    let migrations_dir = Path::new("migrations");
    println!("cargo:rerun-if-changed=migrations");

    let mut entries: Vec<(i64, String, String)> = Vec::new();
    if migrations_dir.is_dir() {
        for entry in fs::read_dir(migrations_dir).expect("read migrations dir") {
            let path = entry.expect("read migration entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("sql") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("migration file name")
                .to_string();
            let version: i64 = name
                .split('_')
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("migration file name must start with a version: {name}"));
            let sql = fs::read_to_string(&path).expect("read migration file");
            entries.push((version, name, sql));
        }
    }
    entries.sort_by_key(|(version, _, _)| *version);

    let mut generated =
        String::from("/// (version, file name, SQL) of every embedded migration.\n");
    generated.push_str("pub const MIGRATIONS: &[(i64, &str, &str)] = &[\n");
    for (version, name, sql) in &entries {
        generated.push_str(&format!("    ({version}, {name:?}, {sql:?}),\n"));
    }
    generated.push_str("];\n");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out_dir);
    fs::write(out.join("migrations.rs"), generated).expect("write generated migrations");

    // Keep the build script from being considered stale by cargo's default
    // "rerun if any file in the package changed" heuristic.
    let _ = Command::new("true").status();
}
