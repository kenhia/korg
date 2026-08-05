//! Sprint 043 (#967, korg:971) — proposals are single-project, enforced.
//!
//! Until now "one proposal, one project" was a convention the `refill-queue`
//! and `start-sprint` skills happened to follow. Nothing stopped a proposal
//! being created with no project at all, and nothing stopped a `covers` edge
//! reaching a work item in a different project — which is how Ken kept finding
//! other projects' work inside a sprint he thought was single-repo.
//!
//! Two write rules, both in core (the single path both transports share, the
//! same place D-11/D-12 put the label and endpoint-kind checks):
//!
//! 1. `create_proposal` refuses without a project.
//! 2. A `covers` edge is refused when the two ends are in *different*
//!    projects — through `relate()` and through `create_proposal`'s bundled
//!    insert, which does not go via `relate()`.
//!
//! A work item with **no** project is deliberately not refused: it is unfiled,
//! not filed elsewhere, and the corpus holds none of them (measured on the
//! deployed instance 2026-08-05). See the sprint record.

use korg_core::repo::{
    create_project, create_proposal, create_work_item, node_id_for_wi, relate, NewProposal,
    NewWorkItem,
};
use korg_test_support::{fresh_korg, new, TEST_PROJECT};
use sqlx::PgPool;

fn wi_in(project: &str, title: &str) -> NewWorkItem {
    NewWorkItem {
        project: Some(project.into()),
        content: "x".into(),
        ..new::work_item(title)
    }
}

async fn wi_node(pool: &PgPool, project: &str, title: &str) -> i64 {
    let n = create_work_item(pool, wi_in(project, title))
        .await
        .unwrap()
        .wi_number;
    node_id_for_wi(pool, n).await.unwrap().unwrap()
}

// --- 1. a proposal without a project is not a proposal ----------------------

#[tokio::test]
async fn create_proposal_refuses_without_a_project() {
    let (_c, pool) = fresh_korg().await;

    let err = create_proposal(
        &pool,
        NewProposal {
            project: None,
            project_id: None,
            ..new::proposal("no home")
        },
    )
    .await
    .expect_err("a project-less proposal must be refused");

    let msg = err.to_string();
    assert!(
        msg.contains("project"),
        "the refusal must name the missing field: {msg}"
    );
    // The routing hint, not a bare rejection — the caller is an agent that can
    // fix this in one retry if told how.
    assert!(
        msg.contains("project_id"),
        "the refusal must name both selectors: {msg}"
    );
}

#[tokio::test]
async fn create_proposal_accepts_either_selector() {
    let (_c, pool) = fresh_korg().await;
    let id = create_project(&pool, "alpha").await.unwrap();

    let by_name = create_proposal(&pool, new::proposal_in("alpha", "by name"))
        .await
        .unwrap();
    assert_eq!(by_name.row.project.as_deref(), Some("alpha"));

    let by_id = create_proposal(
        &pool,
        NewProposal {
            project: None,
            project_id: Some(id),
            ..new::proposal("by id")
        },
    )
    .await
    .unwrap();
    assert_eq!(by_id.row.project.as_deref(), Some("alpha"));
}

// --- 2. covers never spans two projects -------------------------------------

#[tokio::test]
async fn propose_sprint_refuses_a_work_item_from_another_project() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "alpha").await.unwrap();
    create_project(&pool, "beta").await.unwrap();

    let mine = create_work_item(&pool, wi_in("alpha", "mine"))
        .await
        .unwrap()
        .wi_number;
    let theirs = create_work_item(&pool, wi_in("beta", "theirs"))
        .await
        .unwrap()
        .wi_number;

    let err = create_proposal(
        &pool,
        NewProposal {
            covers: vec![mine, theirs],
            ..new::proposal_in("alpha", "cross-project bundle")
        },
    )
    .await
    .expect_err("a covers edge into another project must be refused");

    let msg = err.to_string();
    assert!(msg.contains("alpha"), "names the proposal's project: {msg}");
    assert!(msg.contains("beta"), "names the work item's project: {msg}");
    assert!(
        msg.contains(&format!("#{theirs}")),
        "names the offending work item: {msg}"
    );
    // The refusal teaches the sanctioned cross-project path rather than just
    // blocking (proposal korg:971's notes).
    assert!(
        msg.contains("program"),
        "points at the program layer: {msg}"
    );

    // No partial insert: the whole create is refused, so `mine` is not left
    // covered by a proposal that never came back to the caller.
    let orphans: i64 =
        sqlx::query_scalar("SELECT count(*) FROM relationship WHERE relationship = 'covers'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(orphans, 0, "a refused create leaves no edges behind");
    let proposals: i64 = sqlx::query_scalar("SELECT count(*) FROM sprint_proposal")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(proposals, 0, "a refused create leaves no proposal behind");
}

#[tokio::test]
async fn relate_refuses_a_cross_project_covers_edge() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "alpha").await.unwrap();
    create_project(&pool, "beta").await.unwrap();

    let p = create_proposal(&pool, new::proposal_in("alpha", "alpha work"))
        .await
        .unwrap();
    let theirs = wi_node(&pool, "beta", "theirs").await;

    let err = relate(&pool, p.row.node_id, theirs, "covers", None)
        .await
        .expect_err("relate must apply the same rule as propose_sprint");
    let msg = err.to_string();
    assert!(msg.contains("alpha") && msg.contains("beta"), "{msg}");
    assert!(msg.contains("program"), "{msg}");
}

#[tokio::test]
async fn same_project_covers_is_unaffected() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "alpha").await.unwrap();

    let a = create_work_item(&pool, wi_in("alpha", "a"))
        .await
        .unwrap()
        .wi_number;
    let p = create_proposal(
        &pool,
        NewProposal {
            covers: vec![a],
            ..new::proposal_in("alpha", "alpha sprint")
        },
    )
    .await
    .unwrap();
    assert_eq!(p.covered.len(), 1);

    // And the later relate() path, the one a skill uses to add a WI to a
    // proposal that already exists.
    let b = wi_node(&pool, "alpha", "b").await;
    relate(&pool, p.row.node_id, b, "covers", Some("test"))
        .await
        .expect("same-project covers still relates");
}

#[tokio::test]
async fn an_unfiled_work_item_is_not_a_cross_project_edge() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, TEST_PROJECT).await.unwrap();

    // `create_work_item` still allows no project, and an unfiled item is not
    // filed *elsewhere* — refusing it would be a different (stricter) rule than
    // #967 asked for. Recorded as a test so the choice is deliberate rather
    // than an accident of the SQL.
    let unfiled = create_work_item(&pool, new::work_item("unfiled"))
        .await
        .unwrap()
        .wi_number;

    let p = create_proposal(
        &pool,
        NewProposal {
            covers: vec![unfiled],
            ..new::proposal(TEST_PROJECT)
        },
    )
    .await
    .expect("an unfiled work item may still be covered");
    assert_eq!(p.covered.len(), 1);
}

// --- 3. the other labels are untouched --------------------------------------

#[tokio::test]
async fn only_covers_carries_the_project_rule() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "alpha").await.unwrap();
    create_project(&pool, "beta").await.unwrap();

    let a = wi_node(&pool, "alpha", "a").await;
    let b = wi_node(&pool, "beta", "b").await;

    // depends_on across projects is the normal case — the homelab-ai plan is
    // built out of them — and related-to is deliberately unconstrained.
    relate(&pool, a, b, "depends_on", None)
        .await
        .expect("cross-project depends_on stays legal");
    relate(&pool, a, b, "related-to", None)
        .await
        .expect("cross-project related-to stays legal");
}
