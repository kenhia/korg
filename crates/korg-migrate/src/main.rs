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
//!
//! Flags:
//!   --reset             TRUNCATE korg work items/cards/projects/areas first.

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

#[tokio::main]
async fn main() -> Result<()> {
    let korg_url = std::env::var("KORG_DATABASE_URL").context("KORG_DATABASE_URL is required")?;
    let admin_url =
        std::env::var("KORG_ADMIN_URL").unwrap_or_else(|_| swap_db(&korg_url, "postgres"));
    let snap_dir = PathBuf::from(std::env::var("KORG_SNAPSHOTS").unwrap_or_else(|_| "snapshots".into()));
    let reset = std::env::args().any(|a| a == "--reset");

    eprintln!(">> restoring snapshots from {}", snap_dir.display());
    let admin = connect(&admin_url).await.context("connect admin db")?;
    recreate_db(&admin, "korg_kwi_src").await?;
    recreate_db(&admin, "korg_kcard_src").await?;
    let kwi_src = swap_db(&admin_url, "korg_kwi_src");
    let kcard_src = swap_db(&admin_url, "korg_kcard_src");
    restore(&kwi_src, &snap_dir.join("kwi.dump"))?;
    restore(&kcard_src, &snap_dir.join("kcard.dump"))?;

    eprintln!(">> reading sources");
    let kwi = read_kwi(&connect(&kwi_src).await?).await.context("read kwi")?;
    let kcard = read_kcard(&connect(&kcard_src).await?).await.context("read kcard")?;

    eprintln!(">> migrating + importing into korg");
    let korg = korg_core::connect(&korg_url).await.context("connect korg")?;
    if reset {
        eprintln!(">> --reset: clearing existing korg work items / cards / projects / areas");
        sqlx::query("TRUNCATE node, project, area RESTART IDENTITY CASCADE")
            .execute(&korg)
            .await?;
        sqlx::query("ALTER SEQUENCE workitem_wi_number_seq RESTART WITH 1")
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
