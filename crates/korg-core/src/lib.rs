//! korg-core — domain model, schema, and SQL access for korg.
//!
//! korg unifies kwi (work items) and kcard (kanban cards) onto a single
//! typed-node + generalized-edges model. This crate owns the schema
//! migrations and the repository layer.

/// Embedded SQL migrations applied via `sqlx::migrate!`.
pub fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}

pub mod repo;
