//! korg-migrate CLI — one-shot import of kwi + kcard into korg.
//!
//! Runs the same path the fidelity tests verify: restore the read-only
//! `pg_dump` snapshots into scratch source databases, read them, and import
//! into korg. The source databases are never touched.
//!
//! Env:
//!   KORG_DATABASE_URL   (required) destination korg database.
//!   KORG_ADMIN_URL      admin/superuser conn used to create scratch source
//!                       DBs and to host them (default: KORG_DATABASE_URL with
//!                       the database swapped to `postgres`).
//!   KORG_SNAPSHOTS      directory holding kwi.dump / kcard.dump (default: ./snapshots).
//!   KORG_RESET_CONFIRM  must be `yes` to allow --reset (see below).
//!
//! Flags:
//!   --reset             TRUNCATE **every node kind** plus projects and areas
//!                       first — work items, cards, links, topics, daily plan
//!                       items, sprint proposals and reports all go. The import
//!                       is one-shot and long done, so in practice --reset is
//!                       only ever reached by mistake; it refuses to run unless
//!                       KORG_RESET_CONFIRM=yes.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use korg_migrate::import::import;
use korg_migrate::source::{read_kcard, read_kwi};

fn swap_db(url: &str, db: &str) -> String {
    match url.rsplit_once('/') {
        Some((head, _)) => format!("{head}/{db}"),
        None => url.to_string(),
    }
}

async fn connect(url: &str) -> Result<PgPool> {
    Ok(PgPoolOptions::new().max_connections(4).connect(url).await?)
}

async fn recreate_db(admin: &PgPool, name: &str) -> Result<()> {
    sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .execute(admin)
        .await
        .with_context(|| format!("drop {name}"))?;
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin)
        .await
        .with_context(|| format!("create {name}"))?;
    Ok(())
}

fn restore(db_url: &str, dump: &std::path::Path) -> Result<()> {
    if !dump.exists() {
        bail!("snapshot {dump:?} missing — run `just snapshot` first");
    }
    let out = Command::new("pg_restore")
        .args(["--no-owner", "--no-privileges", "--dbname", db_url])
        .arg(dump)
        .output()
        .context("spawn pg_restore")?;
    if !out.status.success() {
        bail!(
            "pg_restore failed for {dump:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Refuse `--reset` unless the operator has explicitly confirmed, and show
/// exactly what would be destroyed (WI #528). The truncate is
/// `TRUNCATE node, project, area … CASCADE`: it takes **every** node kind, not
/// the "work items / cards / projects / areas" the flag used to advertise.
/// The import is one-shot and finished, so a re-run against a live database is
/// almost by definition an accident.
async fn guard_reset(korg: &PgPool) -> Result<()> {
    let counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT kind, count(*) FROM node GROUP BY kind ORDER BY kind")
            .fetch_all(korg)
            .await?;
    let inventory = if counts.is_empty() {
        "no nodes".to_string()
    } else {
        counts
            .iter()
            .map(|(kind, n)| format!("{n} {kind}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if std::env::var("KORG_RESET_CONFIRM").as_deref() != Ok("yes") {
        bail!(
            "--reset would TRUNCATE every node kind plus projects and areas \
             (currently: {inventory}). This destroys topics, daily plans, sprint \
             proposals, reports and reading-list links as well as work items and \
             cards. Set KORG_RESET_CONFIRM=yes to proceed."
        );
    }
    eprintln!(">> --reset confirmed; destroying: {inventory}");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let korg_url = std::env::var("KORG_DATABASE_URL").context("KORG_DATABASE_URL is required")?;
    let admin_url =
        std::env::var("KORG_ADMIN_URL").unwrap_or_else(|_| swap_db(&korg_url, "postgres"));
    let snap_dir =
        PathBuf::from(std::env::var("KORG_SNAPSHOTS").unwrap_or_else(|_| "snapshots".into()));
    let reset = std::env::args().any(|a| a == "--reset");

    // Connect (and migrate) korg first so `--reset` is refused before any
    // snapshot work — a mistaken reset should cost nothing.
    let korg = korg_core::connect(&korg_url)
        .await
        .context("connect korg")?;
    if reset {
        guard_reset(&korg).await?;
    }

    eprintln!(">> restoring snapshots from {}", snap_dir.display());
    let admin = connect(&admin_url).await.context("connect admin db")?;
    recreate_db(&admin, "korg_kwi_src").await?;
    recreate_db(&admin, "korg_kcard_src").await?;
    let kwi_src = swap_db(&admin_url, "korg_kwi_src");
    let kcard_src = swap_db(&admin_url, "korg_kcard_src");
    restore(&kwi_src, &snap_dir.join("kwi.dump"))?;
    restore(&kcard_src, &snap_dir.join("kcard.dump"))?;

    eprintln!(">> reading sources");
    let kwi = read_kwi(&connect(&kwi_src).await?)
        .await
        .context("read kwi")?;
    let kcard = read_kcard(&connect(&kcard_src).await?)
        .await
        .context("read kcard")?;

    eprintln!(">> migrating + importing into korg");
    if reset {
        eprintln!(">> --reset: truncating ALL nodes (every kind) plus projects and areas");
        sqlx::query("TRUNCATE node, project, area RESTART IDENTITY CASCADE")
            .execute(&korg)
            .await?;
    }

    let report = import(&kwi, &kcard, &korg).await.context("import")?;
    println!(
        "imported: {} projects, {} areas, {} work items (wi_number -> {}), {} cards, {} comments, {} relationships",
        report.projects,
        report.areas,
        report.workitems,
        report.max_wi_number,
        report.cards,
        report.comments,
        report.relationships,
    );
    Ok(())
}
