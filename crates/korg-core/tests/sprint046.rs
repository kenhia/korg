//! Sprint 046 (korg:1004, #1003) — the board's curated layer.
//!
//! kfdc's Phase-2 curator writes typed things INTO korg — `depends_on` /
//! `collides-with` edges between proposals, one ⟦curator⟧-marked synopsis
//! comment per proposal — and kfdc's Deconfliction + Sensor Net panels render
//! only what this rollup exposes. What this suite pins:
//!
//! 1. **An edge rides the board only when both ends are live board rows.** An
//!    edge to a done/declined/archived proposal is not a Deconfliction card,
//!    and a slice-only row fetched for Operations does not smuggle its edges
//!    into the panel.
//! 2. **Edge provenance is readable** — the first read surface for D-17's
//!    write-side `origin`/`created`, with `directed` answered by the registry
//!    so no consumer hardcodes label semantics.
//! 3. **The synopsis is the newest ⟦curator⟧-marked comment.** Unmarked
//!    comments never surface on the board; a row without a marked comment
//!    carries `None` rather than a guess.

use korg_core::repo::{
    add_comment, board_rollup, create_proposal, relate, update_proposal, ProposalPatch,
    CURATOR_MARKER,
};
use korg_test_support::{fresh_korg, new, test_project};
use sqlx::PgPool;

/// A proposal in the test project with the given status, returning its node_id.
async fn proposal(pool: &PgPool, title: &str, status: &str) -> i64 {
    let id = create_proposal(
        pool,
        new::proposal_in(korg_test_support::TEST_PROJECT, title),
    )
    .await
    .unwrap()
    .row
    .node_id;
    if status != "proposed" {
        update_proposal(
            pool,
            id,
            ProposalPatch {
                status: Some(status.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    id
}

#[tokio::test]
async fn proposal_edges_ride_the_board_only_between_live_rows_with_provenance() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = proposal(&pool, "a", "active").await;
    let b = proposal(&pool, "b", "proposed").await;
    let done = proposal(&pool, "gone", "done").await;

    relate(&pool, a, b, "depends_on", Some("kfdc-curator"), None)
        .await
        .unwrap();
    relate(&pool, b, a, "collides-with", Some("kfdc-curator"), None)
        .await
        .unwrap();
    // Both of these touch a non-live proposal: neither may surface.
    relate(&pool, a, done, "depends_on", None, None)
        .await
        .unwrap();
    relate(&pool, done, b, "collides-with", None, None)
        .await
        .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(
        board.proposal_edges.len(),
        2,
        "exactly the two edges between live rows: {:?}",
        board.proposal_edges
    );

    let dep = board
        .proposal_edges
        .iter()
        .find(|e| e.label == "depends_on")
        .expect("depends_on edge rides the board");
    assert_eq!((dep.left, dep.right), (a, b), "orientation preserved");
    assert!(dep.directed, "depends_on is registry-directed");
    assert_eq!(
        dep.origin.as_deref(),
        Some("kfdc-curator"),
        "origin readable at last (D-17's first read surface)"
    );

    let col = board
        .proposal_edges
        .iter()
        .find(|e| e.label == "collides-with")
        .expect("collides-with edge rides the board");
    assert!(!col.directed, "collides-with is registry-undirected");
    assert!(
        col.created <= board.generated,
        "created from the same clock"
    );
}

#[tokio::test]
async fn synopsis_is_the_newest_curator_marked_comment_and_unmarked_never_surface() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = proposal(&pool, "curated", "active").await;
    let b = proposal(&pool, "uncurated", "proposed").await;

    add_comment(&pool, a, "human discussion, not a synopsis")
        .await
        .unwrap();
    let stale = format!("{CURATOR_MARKER}\nstale synopsis");
    add_comment(&pool, a, &stale).await.unwrap();
    let fresh = format!("{CURATOR_MARKER}\nharness landed; mining pass in design");
    add_comment(&pool, a, &fresh).await.unwrap();
    add_comment(&pool, b, "unmarked comment on the other row")
        .await
        .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let row_a = board.active.iter().find(|p| p.node_id == a).unwrap();
    let syn = row_a.synopsis.as_ref().expect("marked comment surfaces");
    assert_eq!(syn.body, fresh, "the newest marked comment wins");
    assert!(syn.updated <= board.generated, "stamp from the same clock");

    let row_b = board.queue.iter().find(|p| p.node_id == b).unwrap();
    assert!(
        row_b.synopsis.is_none(),
        "unmarked comments never surface as a synopsis"
    );
}
