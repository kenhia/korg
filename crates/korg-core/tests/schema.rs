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

    // 0003's timebox slots went in 0012; 0012's own daily planning went in
    // 0024 (WI #965). All four tables must stay gone.
    for removed in ["slot_template", "slot", "topic", "daily_plan_item"] {
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
    // Every insert carries a project because 0022 requires one on
    // `sprint_proposal`; the other kinds are indifferent to it, and this test
    // is about the kind vocabulary, not about routing.
    let project: i64 =
        sqlx::query_scalar("INSERT INTO project (name) VALUES ('kinds') RETURNING id")
            .fetch_one(&pool)
            .await
            .expect("seed a project");
    for kind in [
        "workitem",
        "card",
        "link",
        "sprint_proposal",
        "report",
        "handoff",
    ] {
        sqlx::query("INSERT INTO node (kind, project_id) VALUES ($1, $2)")
            .bind(kind)
            .bind(project)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("kind `{kind}` should be accepted: {e}"));
    }
    // A program is the one kind that must NOT carry a project (0023 §3).
    sqlx::query("INSERT INTO node (kind) VALUES ('program')")
        .execute(&pool)
        .await
        .expect("kind `program` should be accepted without a project");
    // …and the new constraint is real: a proposal node with no project is
    // refused at the database, not only by core (0022 §2).
    assert!(
        sqlx::query("INSERT INTO node (kind) VALUES ('sprint_proposal')")
            .execute(&pool)
            .await
            .is_err(),
        "a sprint_proposal node must carry a project (#967)"
    );
    for retired in ["slot", "topic", "daily_plan_item"] {
        assert!(
            sqlx::query("INSERT INTO node (kind) VALUES ($1)")
                .bind(retired)
                .execute(&pool)
                .await
                .is_err(),
            "{retired} must no longer be an accepted node kind"
        );
    }
}
