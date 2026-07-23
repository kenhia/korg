//! Schema migration test (build step S2).
//!
//! Spins up an ephemeral Postgres via testcontainers, applies korg's
//! embedded migrations, and asserts the typed-node + generalized-edges
//! schema landed: every table, the `card_status` enum, and the
//! `wi_number` sequence exist.

use korg_test_support::raw_postgres;
use sqlx::Row;

#[tokio::test]
async fn schema_applies_cleanly() {
    // `raw_postgres` deliberately, not `fresh_korg`: this test's subject *is*
    // the migrator, so it must run it itself and say so when it fails.
    let (_pg, pool) = raw_postgres().await;

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
        "topic",
        "daily_plan_item",
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

    for removed in ["slot_template", "slot"] {
        let exists: bool = sqlx::query(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = $1)",
        )
        .bind(removed)
        .fetch_one(&pool)
        .await
        .expect("query removed table existence")
        .get(0);
        assert!(!exists, "obsolete table `{removed}` must be removed");
    }

    // The replacement kinds are accepted and the obsolete slot kind is not.
    for kind in [
        "workitem",
        "card",
        "link",
        "sprint_proposal",
        "report",
        "topic",
        "daily_plan_item",
    ] {
        sqlx::query("INSERT INTO node (kind) VALUES ($1)")
            .bind(kind)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("kind `{kind}` should be accepted: {e}"));
    }
    assert!(
        sqlx::query("INSERT INTO node (kind) VALUES ('slot')")
            .execute(&pool)
            .await
            .is_err(),
        "slot must no longer be an accepted node kind"
    );
}
