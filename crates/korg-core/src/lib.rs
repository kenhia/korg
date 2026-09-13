//! korg-core — domain model, schema, and SQL access for korg.
//!
//! korg unifies kwi (work items) and kcard (kanban cards) onto a single
//! typed-node + generalized-edges model. This crate owns the schema
//! migrations and the repository layer.

/// Embedded SQL migrations applied via `sqlx::migrate!`.
pub fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}

// Connecting lives in `db`, which also owns how the deployed binary gets its
// password (korg #2547). `connect` is re-exported here because it is the name
// every test and the importer already call.
pub use db::connect;

pub mod config;
pub mod db;
pub mod error;
pub mod ops;
pub mod relationships;
pub mod repo;
pub mod vocab;
