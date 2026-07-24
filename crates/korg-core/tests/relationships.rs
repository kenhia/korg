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

    let err = relate(&pool, a.node_id, a.node_id, "depends_on", None)
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
    relate(&pool, hub.node_id, dep.node_id, "depends_on", None)
        .await
        .unwrap();
    relate(&pool, hub.node_id, dep.node_id, "related-to", None)
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

// --- LB-2: registry enforcement + provenance write path -------------------

/// D-11: the vocabulary is closed. An unregistered label is invalid_input whose
/// message names the whole registry and, for an obvious near-miss, suggests the
/// real label — the sprint-017 principle that the error is the retry doc.
#[tokio::test]
async fn unknown_label_is_rejected_naming_the_vocabulary_and_near_miss() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("a")).await.unwrap();
    let b = create_work_item(&pool, wi("b")).await.unwrap();

    let err = relate(&pool, a.node_id, b.node_id, "related", None)
        .await
        .unwrap_err();
    let msg = match err.downcast_ref::<RepoError>() {
        Some(RepoError::InvalidInput(m)) => m.clone(),
        other => panic!("expected invalid_input, got {other:?}"),
    };
    assert!(
        msg.contains("covers, finding, depends_on, related-to"),
        "names the whole vocabulary: {msg}"
    );
    assert!(
        msg.contains("did you mean 'related-to'"),
        "suggests the near-miss: {msg}"
    );
}

/// D-12: a kind-constrained label validates both endpoints. `covers` demands a
/// sprint_proposal on the left; a work item there is invalid_input naming the
/// expected kind. (create_proposal writes covers by construction and never
/// reaches this path.)
#[tokio::test]
async fn covers_via_relate_validates_endpoint_kinds() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("a")).await.unwrap();
    let b = create_work_item(&pool, wi("b")).await.unwrap();

    let err = relate(&pool, a.node_id, b.node_id, "covers", None)
        .await
        .unwrap_err();
    let msg = match err.downcast_ref::<RepoError>() {
        Some(RepoError::InvalidInput(m)) => m.clone(),
        other => panic!("expected invalid_input, got {other:?}"),
    };
    assert!(
        msg.contains("sprint_proposal") && msg.contains("left"),
        "names the expected left kind: {msg}"
    );
}

/// D-17: relate stamps `created` + the caller's `origin`, and the ON CONFLICT
/// no-op preserves both on a re-relate (what LB-1's migration comment reserved).
#[tokio::test]
async fn relate_stamps_provenance_and_preserves_it_on_rerelate() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("a")).await.unwrap();
    let b = create_work_item(&pool, wi("b")).await.unwrap();

    let id = relate(&pool, a.node_id, b.node_id, "related-to", Some("web"))
        .await
        .unwrap();
    let created1: String =
        sqlx::query_scalar("SELECT created::text FROM relationship WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let origin1: Option<String> =
        sqlx::query_scalar("SELECT origin FROM relationship WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!created1.is_empty(), "created stamped on insert");
    assert_eq!(origin1.as_deref(), Some("web"), "origin recorded as sent");

    // Re-relate with a different origin: the no-op must keep the originals.
    let id2 = relate(
        &pool,
        a.node_id,
        b.node_id,
        "related-to",
        Some("sprint-ship"),
    )
    .await
    .unwrap();
    assert_eq!(id2, id, "re-relate returns the same edge");
    let created2: String =
        sqlx::query_scalar("SELECT created::text FROM relationship WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let origin2: Option<String> =
        sqlx::query_scalar("SELECT origin FROM relationship WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(created2, created1, "created preserved on re-relate");
    assert_eq!(
        origin2.as_deref(),
        Some("web"),
        "origin preserved on re-relate"
    );
}

/// D-17: the internal edge writers stamp their operation name as origin, so
/// every covers/finding edge is attributed even though no caller passed one.
#[tokio::test]
async fn internal_writers_stamp_their_operation_as_origin() {
    let (_c, pool) = fresh_korg().await;
    let hub = create_work_item(&pool, wi("hub")).await.unwrap();
    create_proposal(
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

    let origin: Option<String> =
        sqlx::query_scalar("SELECT origin FROM relationship WHERE relationship = 'covers' LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(origin.as_deref(), Some("propose_sprint"));
}

/// LB-3 (D-20): a focused read inlines the node's edges, so an agent reading a
/// work item cannot silently miss that it is covered or depended on. Each ref
/// carries the neighbor's title (and wi_number when it is a work item). The
/// proposal read excludes `covers` (already inlined as `covered`) but carries
/// its other edges.
#[tokio::test]
async fn focused_reads_inline_related_context() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("covered item")).await.unwrap();
    let dep = create_work_item(&pool, wi("a dependency")).await.unwrap();

    // `a` depends on `dep`; a proposal covers `a`.
    relate(&pool, a.node_id, dep.node_id, "depends_on", None)
        .await
        .unwrap();
    let p = create_proposal(
        &pool,
        NewProposal {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            title: "the sprint".into(),
            summary: "s".into(),
            rank: Decimal::ZERO,
            pinned: false,
            covers: vec![a.wi_number],
        },
    )
    .await
    .unwrap();

    // get_work_item(a) inlines both edges, no second round-trip.
    let d = repo::get_work_item_detail(&pool, a.wi_number)
        .await
        .unwrap()
        .unwrap();
    assert!(!d.related_truncated);
    let cov = d
        .related
        .iter()
        .find(|r| r.label == "covers")
        .expect("covers edge inlined on the work item");
    assert_eq!(cov.direction, "in", "the proposal covers the item");
    assert_eq!(cov.kind, "sprint_proposal");
    assert_eq!(cov.title, "the sprint", "carries the proposal's title");
    let dp = d
        .related
        .iter()
        .find(|r| r.label == "depends_on")
        .expect("depends_on inlined");
    assert_eq!(dp.direction, "out");
    assert_eq!(
        dp.wi_number,
        Some(dep.wi_number),
        "work-item neighbor carries its wi_number"
    );
    assert_eq!(dp.title, "a dependency");

    // get_proposal(p) excludes covers (already in `covered`) but inlines others.
    relate(&pool, p.row.node_id, dep.node_id, "related-to", None)
        .await
        .unwrap();
    let pd = repo::get_proposal_detail(&pool, p.row.node_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        pd.related.iter().all(|r| r.label != "covers"),
        "covers is excluded from proposal.related (it is in `covered`)"
    );
    assert!(
        pd.related.iter().any(|r| r.label == "related-to"),
        "non-covers edges are inlined on the proposal"
    );
    assert_eq!(pd.covered.len(), 1, "covers still reported via `covered`");
}
