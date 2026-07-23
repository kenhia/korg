//! Sprint 014 — relationship semantics: semantic orientation for
//! `covers`/`finding`, the 0014 backfill, self-edge rejection, and the
//! `neighbors` filters/limit.

use korg_core::relationships;
use korg_core::repo::{
    self, create_proposal, create_work_item, neighbors, relate, upsert_report, NeighborQuery,
    NewProposal, NewReport, NewWorkItem, RepoError,
};
use korg_test_support::{fresh_korg, new};
use rust_decimal::Decimal;
use time::macros::date;

fn wi(title: &str) -> NewWorkItem {
    NewWorkItem {
        wi_tshirt: "S".into(),
        content: "c".into(),
        ..new::work_item(title)
    }
}

/// Every `covers` edge must read proposal → work item, whichever end you ask
/// from. Before WI #531 the orientation was `(least(id), greatest(id))`, so it
/// recorded node-id ordering and `direction` was noise.
#[tokio::test]
async fn covers_edges_are_written_proposal_to_work_item() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("first")).await.unwrap();
    let b = create_work_item(&pool, wi("second")).await.unwrap();

    let p = create_proposal(
        &pool,
        NewProposal {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            title: "bundle".into(),
            summary: "s".into(),
            rank: Decimal::ZERO,
            pinned: false,
            covers: vec![a.wi_number, b.wi_number],
        },
    )
    .await
    .unwrap();

    // The proposal's node id is higher than both work items', so an
    // id-canonicalizing writer would have put the proposal on the right.
    assert!(p.row.node_id > a.node_id && p.row.node_id > b.node_id);

    let from_proposal = neighbors(&pool, p.row.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(from_proposal.total, 2);
    assert!(
        from_proposal.items.iter().all(|n| n.direction == "out"),
        "covers reads proposal -> work item"
    );

    // …and the same edge seen from the work item reads "in".
    let from_item = neighbors(&pool, a.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(from_item.items.len(), 1);
    assert_eq!(from_item.items[0].direction, "in");
    assert_eq!(from_item.items[0].node_id, p.row.node_id);
    assert!(from_item.items[0].directed);
}

/// Same contract for `finding`: report → work item.
#[tokio::test]
async fn finding_edges_are_written_report_to_work_item() {
    let (_c, pool) = fresh_korg().await;
    let w = create_work_item(&pool, wi("disk full")).await.unwrap();
    let r = upsert_report(
        &pool,
        NewReport {
            source: "kmon".into(),
            report_date: date!(2026 - 07 - 22),
            status: "problem".into(),
            summary: "s".into(),
            body: "b".into(),
            model: None,
            escalated: false,
            findings: vec![w.wi_number],
        },
    )
    .await
    .unwrap();

    let from_report = neighbors(&pool, r.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(from_report.items.len(), 1);
    assert_eq!(from_report.items[0].direction, "out");
    assert_eq!(from_report.items[0].label, "finding");

    let from_item = neighbors(&pool, w.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(from_item.items[0].direction, "in");
}

/// The 0014 backfill, exercised against edges written the old way. Inserting
/// id-canonicalized edges directly reproduces the pre-sprint state, including
/// the legacy work-item bundle (`covers` between two work items, from before
/// the `sprint_proposal` kind existed).
#[tokio::test]
async fn backfill_orients_legacy_edges_and_leaves_no_proposal_on_the_right() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("covered one")).await.unwrap();
    let bundle = create_work_item(&pool, wi("Sprint: legacy bundle"))
        .await
        .unwrap();
    let p = create_proposal(
        &pool,
        NewProposal {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            title: "modern".into(),
            summary: "s".into(),
            rank: Decimal::ZERO,
            pinned: false,
            covers: vec![],
        },
    )
    .await
    .unwrap();

    // Rewrite history: store both edges the way the old writers did.
    sqlx::query("DELETE FROM relationship")
        .execute(&pool)
        .await
        .unwrap();
    for (lo, hi, label) in [
        (a.node_id, p.row.node_id, "covers"),  // work item -> proposal
        (a.node_id, bundle.node_id, "covers"), // work item -> legacy bundle
    ] {
        sqlx::query("INSERT INTO relationship (left_id, right_id, relationship) VALUES ($1,$2,$3)")
            .bind(lo)
            .bind(hi)
            .bind(label)
            .execute(&pool)
            .await
            .unwrap();
    }

    // Re-run the migration's logic by applying it to this dirtied state: the
    // migrator has already run, so drive the same statements the file does.
    sqlx::query(
        "UPDATE relationship r SET left_id = r.right_id, right_id = r.left_id \
         FROM node l, node rt WHERE l.id = r.left_id AND rt.id = r.right_id \
           AND r.relationship = 'covers' \
           AND rt.kind = 'sprint_proposal' AND l.kind <> 'sprint_proposal'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE relationship r SET left_id = r.right_id, right_id = r.left_id \
         FROM workitem lw, workitem rw \
         WHERE lw.node_id = r.left_id AND rw.node_id = r.right_id \
           AND r.relationship = 'covers' \
           AND rw.title LIKE 'Sprint:%' AND lw.title NOT LIKE 'Sprint:%'",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Postcondition the migration asserts.
    let bad: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM relationship r JOIN node rt ON rt.id = r.right_id \
         WHERE r.relationship = 'covers' AND rt.kind = 'sprint_proposal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bad, 0, "no covers edge may point at a proposal");

    // The proposal now covers outward…
    let from_proposal = neighbors(&pool, p.row.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(from_proposal.items[0].direction, "out");
    // …and so does the legacy work-item bundle.
    let from_bundle = neighbors(&pool, bundle.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(from_bundle.items[0].direction, "out");
    assert_eq!(from_bundle.items[0].node_id, a.node_id);
}

/// WI #532 — a node related to itself is meaningless under every registry
/// label and would make a `depends_on` node block itself forever.
#[tokio::test]
async fn self_edges_are_rejected_by_the_app_and_the_schema() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("lonely")).await.unwrap();

    let err = relate(&pool, a.node_id, a.node_id, "depends_on")
        .await
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<RepoError>(),
            Some(RepoError::InvalidInput(_))
        ),
        "self-edge is invalid input, got {err:?}"
    );

    // The CHECK constraint backs it up even if something bypasses the repo.
    let direct = sqlx::query(
        "INSERT INTO relationship (left_id, right_id, relationship) VALUES ($1,$1,'x')",
    )
    .bind(a.node_id)
    .execute(&pool)
    .await;
    assert!(direct.is_err(), "relationship_no_self_edge must reject it");
}

/// WI #533 — filters, bound, truncation flag, and stable ordering.
#[tokio::test]
async fn neighbors_filters_bounds_and_orders_stably() {
    let (_c, pool) = fresh_korg().await;
    let hub = create_work_item(&pool, wi("hub")).await.unwrap();
    let dep = create_work_item(&pool, wi("dependency")).await.unwrap();
    let p = create_proposal(
        &pool,
        NewProposal {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            title: "bundle".into(),
            summary: "s".into(),
            rank: Decimal::ZERO,
            pinned: false,
            covers: vec![hub.wi_number],
        },
    )
    .await
    .unwrap();
    relate(&pool, hub.node_id, dep.node_id, "depends_on")
        .await
        .unwrap();
    relate(&pool, hub.node_id, dep.node_id, "related-to")
        .await
        .unwrap();

    let all = neighbors(&pool, hub.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(all.total, 3);
    assert!(!all.truncated);
    assert_eq!(all.limit, repo::NEIGHBOR_LIMIT_DEFAULT);

    // Label filter — what the Planning page and start-sprint actually want.
    let covers = neighbors(
        &pool,
        p.row.node_id,
        NeighborQuery {
            label: Some("covers".into()),
            kind: Some("workitem".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(covers.items.len(), 1);
    assert_eq!(covers.items[0].node_id, hub.node_id);

    // A kind filter that matches nothing is empty, not everything.
    let none = neighbors(
        &pool,
        hub.node_id,
        NeighborQuery {
            kind: Some("card".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(none.total, 0);
    assert!(none.items.is_empty());

    // The bound is exact: total counts every match, truncated says so.
    let clipped = neighbors(
        &pool,
        hub.node_id,
        NeighborQuery {
            limit: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(clipped.items.len(), 1);
    assert_eq!(clipped.total, 3);
    assert!(clipped.truncated);

    // Ordering is stable across calls even with two edges to the same node.
    let first: Vec<i64> = all.items.iter().map(|n| n.rel_id).collect();
    let again = neighbors(&pool, hub.node_id, NeighborQuery::default())
        .await
        .unwrap();
    assert_eq!(
        first,
        again.items.iter().map(|n| n.rel_id).collect::<Vec<_>>()
    );

    // `directed` comes from the registry, so a reader knows when to ignore
    // `direction` (D-1).
    let related = all.items.iter().find(|n| n.label == "related-to").unwrap();
    assert!(!related.directed, "related-to is undirected");
    let depends = all.items.iter().find(|n| n.label == "depends_on").unwrap();
    assert!(depends.directed);
    assert!(relationships::direction_is_meaningful("has_handoff"));
}
