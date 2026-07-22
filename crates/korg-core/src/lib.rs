//! korg-core — domain model, schema, and SQL access for korg.
//!
//! korg unifies kwi (work items) and kcard (kanban cards) onto a single
//! typed-node + generalized-edges model. This crate owns the schema
//! migrations and the repository layer.

/// Embedded SQL migrations applied via `sqlx::migrate!`.
pub fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}

/// Connect a pool to `url` and ensure the schema is migrated. Used by the
/// MCP/CLI/web surfaces.
pub async fn connect(url: &str) -> anyhow::Result<sqlx::PgPool> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(url)
        .await?;
    migrator().run(&pool).await?;
    Ok(pool)
}

pub mod config;
pub mod daily_plan;
pub mod error;
pub mod ops;
pub mod relationships;
pub mod repo;
pub mod topics;
pub mod vocab;
