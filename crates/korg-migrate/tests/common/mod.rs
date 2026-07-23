//! Restore the frozen kwi/kcard snapshots into an ephemeral Postgres via
//! `pg_restore`. Used by S4/S5/S6.
//!
//! The container itself comes from `korg-test-support`; these suites need a
//! *bare* server (they restore legacy schemas into named databases), which is
//! why they take `start_pg` rather than `fresh_korg`.

#![allow(dead_code)]

use sqlx::PgPool;
use std::path::PathBuf;
use std::process::Command;

// The container primitive is shared workspace-wide (WI #550); what stays here
// is the snapshot-restore half, which is this crate's alone.
pub use korg_test_support::{connect, start_pg, Pg};

pub fn snapshots_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../snapshots")
}

/// Decide whether a snapshot-backed suite should run, and say so if not.
///
/// These three suites (S4/S5/S6) restore `snapshots/*.dump` — frozen pg_dumps
/// of the legacy kwi/kcard databases. The dumps are gitignored and
/// machine-local, so on a clean checkout `cargo test --workspace` used to fail
/// on a missing file rather than on anything about korg (F-13).
///
/// `KORG_SNAPSHOT_TESTS` decides:
///
/// - **`1`** — required. Missing dumps fail loudly, which is what a run that
///   deliberately asked for these suites wants.
/// - **`0`** — skipped even if the dumps are present.
/// - **unset** — run iff the dumps are present. A clean checkout is green; a
///   machine that has run `just snapshot` keeps the coverage without anyone
///   having to remember a flag.
///
/// Returns `true` when the caller should return early. Rust has no native skip,
/// so the message on stdout (visible with `--nocapture`, and always in CI's
/// summary of a passing run) is how a skip is distinguishable from a pass.
pub fn skip_snapshot_suite(suite: &str) -> bool {
    let dumps_present =
        snapshots_dir().join("kwi.dump").exists() && snapshots_dir().join("kcard.dump").exists();
    match std::env::var("KORG_SNAPSHOT_TESTS").as_deref() {
        Ok("1") => false,
        Ok("0") => {
            println!("skipping {suite}: KORG_SNAPSHOT_TESTS=0");
            true
        }
        _ if dumps_present => false,
        _ => {
            println!(
                "skipping {suite}: no snapshots in {} — run `just snapshot`, \
                 or set KORG_SNAPSHOT_TESTS=1 to require them",
                snapshots_dir().display()
            );
            true
        }
    }
}

async fn create_db(admin: &PgPool, name: &str) {
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin)
        .await
        .expect("create database");
}

fn restore(db_url: &str, dump: &std::path::Path) {
    assert!(
        dump.exists(),
        "snapshot {dump:?} is missing — run `just snapshot` first",
    );
    let output = Command::new("pg_restore")
        .args(["--no-owner", "--no-privileges", "--dbname", db_url])
        .arg(dump)
        .output()
        .expect("spawn pg_restore");
    assert!(
        output.status.success(),
        "pg_restore failed for {dump:?}: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Start Postgres, restore both snapshots into `kwi_src` / `kcard_src`,
/// and hand back pools for each. The returned `Pg` must be kept alive.
pub async fn staged_sources() -> (Pg, PgPool, PgPool) {
    let pg = start_pg().await;
    let admin = connect(&pg.url("postgres")).await;
    create_db(&admin, "kwi_src").await;
    create_db(&admin, "kcard_src").await;

    restore(&pg.url("kwi_src"), &snapshots_dir().join("kwi.dump"));
    restore(&pg.url("kcard_src"), &snapshots_dir().join("kcard.dump"));

    let kwi = connect(&pg.url("kwi_src")).await;
    let kcard = connect(&pg.url("kcard_src")).await;
    (pg, kwi, kcard)
}

/// A fresh, migrated korg database on the same server.
pub async fn migrate_korg(pg: &Pg) -> PgPool {
    let admin = connect(&pg.url("postgres")).await;
    create_db(&admin, "korg").await;
    let korg = connect(&pg.url("korg")).await;
    korg_core::migrator()
        .run(&korg)
        .await
        .expect("apply korg migrations");
    korg
}
