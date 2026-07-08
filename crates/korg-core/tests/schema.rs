//! Schema migration test (build step S2).
//!
//! Spins up an ephemeral Postgres via testcontainers, applies korg's
//! embedded migrations, and asserts the typed-node + generalized-edges
//! schema landed: every table, the `card_status` enum, and the
//! `wi_number` sequence exist.

use sqlx::postgres::PgPoolOptions;
use sqlx::Row;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

#[tokio::test]
async fn schema_applies_cleanly() {
    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres container");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("get mapped port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to postgres");

    korg_core::migrator()
        .run(&pool)
        .await
        .expect("migrations apply cleanly");

    // Every expected table is present.
    for table in [
        "project",
        "area",
        "node",
        "workitem",
        "card",
        "comment",
        "relationship",
        "link",
        "slot_template",
        "slot",
    ] {
        let exists: bool = sqlx::query(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = $1)",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .expect("query table existence")
        .get(0);
        assert!(exists, "table `{table}` should exist after migration");
    }

    // The card_status enum exists.
    let enum_exists: bool =
        sqlx::query("SELECT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'card_status')")
            .fetch_one(&pool)
            .await
            .expect("query enum existence")
            .get(0);
    assert!(enum_exists, "card_status enum should exist");

    // Since 0009_identity the wi_number sequence is GONE — wi_number is assigned
    // from node.id at insert, so the two can never diverge.
    let seq_exists: bool = sqlx::query(
        "SELECT EXISTS (SELECT 1 FROM information_schema.sequences \
         WHERE sequence_schema = 'public' AND sequence_name = 'workitem_wi_number_seq')",
    )
    .fetch_one(&pool)
    .await
    .expect("query sequence existence")
    .get(0);
    assert!(
        !seq_exists,
        "workitem_wi_number_seq should be dropped by 0009_identity"
    );

    // The `link` node kind is accepted (and others still are).
    for kind in ["workitem", "card", "link", "slot"] {
        sqlx::query("INSERT INTO node (kind) VALUES ($1)")
            .bind(kind)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("kind `{kind}` should be accepted: {e}"));
    }
}
