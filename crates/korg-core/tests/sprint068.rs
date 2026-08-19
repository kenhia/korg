//! Sprint 068 (#1414, #1442, #1412, korg:1443) — the project rollup grows a
//! category, and the attachment miss learns to say `img-<hex>`.
//!
//! **#1414 — why `category` had to ride the rollup row.** `planning_rollup` is
//! the board's `depth` and the Planning rail's counts, and it returns *every*
//! project by construction. Sprint 065 (#466) then made `eval` a permanently
//! **active** project — #884 makes an archived project refuse writes, so the
//! harness-residue bucket has to stay live — which means eval residue is inside
//! every project-spanning read and `status` cannot say so: `active` is exactly
//! what it is. A consumer that wanted the real corpus had to call
//! `list_projects` too, against `get_board`'s one-call contract.
//!
//! What ships is the raw category and nothing else. The korg+ plan's GP-10
//! (project tiers as data) treats EVAL as the tier below every tier, so an
//! `is_eval` flag would be a parallel switch that design would have to
//! deprecate. These tests therefore assert the *datum*, and deliberately do not
//! assert that korg filters anything: korg does not.
//!
//! **#1442 — the Work items rail's open count.** Rendered in the web app, but
//! its definition is here, because the number comes from this rollup's
//! `wi_total`. That is the point of the test at the bottom: the count beside a
//! project must be the count of rows you get when you click it, and the Work
//! items list defaults to unarchived and not-`closed`. Two definitions of "open
//! work" on one screen is the bug the Planning rail's own doc comment warns
//! about; this pins them together.

use korg_core::repo::{
    create_project, create_work_item, node_id_for_wi, planning_rollup, update_project,
    update_work_item, NewWorkItem, ProjectPatch, WorkItemPatch,
};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use sqlx::PgPool;

// --- scaffolding ------------------------------------------------------------

async fn project_with_category(pool: &PgPool, name: &str, category: Option<&str>) -> i64 {
    let id = create_project(pool, name).await.unwrap();
    if let Some(c) = category {
        update_project(
            pool,
            id,
            &ProjectPatch {
                category: Some(Some(c.into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    id
}

async fn wi_in(pool: &PgPool, project: &str, title: &str) -> i64 {
    create_work_item(
        pool,
        NewWorkItem {
            project: Some(project.into()),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number
}

async fn set_status(pool: &PgPool, wi_number: i64, status: &str) {
    let node = node_id_for_wi(pool, wi_number).await.unwrap().unwrap();
    update_work_item(
        pool,
        node,
        WorkItemPatch {
            wi_status: Some(status.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

fn row(
    rollup: &[korg_core::repo::PlanningRollupRow],
    name: &str,
) -> korg_core::repo::PlanningRollupRow {
    rollup
        .iter()
        .find(|r| r.project == name)
        .unwrap_or_else(|| panic!("{name} missing from the rollup"))
        .clone()
}

// --- #1414 ------------------------------------------------------------------

/// The datum itself: the rollup row carries the project's category, and `None`
/// for a project the vocabulary has not claimed. `None`, not `""` — an
/// uncategorised project is a state `create_project` leaves you in (it takes
/// only a name), so the absent case is the common one, not the edge.
#[tokio::test]
async fn the_rollup_row_carries_the_projects_category() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    project_with_category(&pool, "alpha", Some("AI")).await;
    project_with_category(&pool, "beta", Some("Ops")).await;
    project_with_category(&pool, "loose", None).await;

    let rollup = planning_rollup(&pool).await.unwrap();
    assert_eq!(row(&rollup, "alpha").category.as_deref(), Some("AI"));
    assert_eq!(row(&rollup, "beta").category.as_deref(), Some("Ops"));
    assert_eq!(row(&rollup, "loose").category, None);
}

/// The case that forced #1414, end to end: an EVAL project is `active` — it has
/// to be, or #884 stops the residue being filed at all — so `status` cannot
/// distinguish it, and `category` is the only thing on the row that can.
///
/// Note what is *not* asserted: that korg drops the row. It does not. `depth`
/// returns every project, and excluding harness residue is the consumer's
/// decision made from this datum (GP-10). A future version of this test that
/// asserts an absence has changed the contract, not fixed the test.
#[tokio::test]
async fn an_eval_project_is_active_and_only_category_says_otherwise() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    project_with_category(&pool, "harness-eval", Some("EVAL")).await;
    project_with_category(&pool, "real-work", Some("AI")).await;
    wi_in(&pool, "harness-eval", "residue").await;
    wi_in(&pool, "real-work", "actual work").await;

    let rollup = planning_rollup(&pool).await.unwrap();
    let eval = row(&rollup, "harness-eval");
    assert_eq!(eval.status, "active", "#884: the residue bucket stays live");
    assert_eq!(eval.category.as_deref(), Some("EVAL"));
    assert_eq!(
        eval.wi_total, 1,
        "its residue is in the corpus, uncounted by nobody"
    );

    // The consumer-side filter #1414 exists to make possible, in one line and
    // against one read.
    let real: Vec<_> = rollup
        .iter()
        .filter(|r| r.category.as_deref() != Some("EVAL"))
        .map(|r| r.project.clone())
        .collect();
    assert!(real.contains(&"real-work".to_string()));
    assert!(!real.contains(&"harness-eval".to_string()));
}

// --- #1442 ------------------------------------------------------------------

/// The Work items rail's open count **is** `wi_total`, and this is what that
/// commits korg to: everything except `closed`, and nothing archived.
///
/// The web page's default filters are the other half of this contract — it
/// hides `closed` (`HIDDEN_BY_DEFAULT`) and hides archived, and shows `open`,
/// `resolved`, `done` and `parked`. So the number beside a project is the
/// number of rows clicking it produces. If this test and that page ever
/// disagree, the rail is lying about the list one click away.
#[tokio::test]
async fn the_rail_count_is_exactly_what_the_default_list_shows() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let mut expected_live = 0;
    for status in ["open", "resolved", "done", "parked"] {
        let n = wi_in(&pool, TEST_PROJECT, status).await;
        set_status(&pool, n, status).await;
        expected_live += 1;
    }
    // Closed is the one status a default list hides — and 78% of the live
    // corpus (#861), which is why it cannot be in a rail count.
    let closed = wi_in(&pool, TEST_PROJECT, "closed").await;
    set_status(&pool, closed, "closed").await;
    // Archived is hidden by default too, whatever its status.
    let archived = wi_in(&pool, TEST_PROJECT, "archived but open").await;
    let node = node_id_for_wi(&pool, archived).await.unwrap().unwrap();
    update_work_item(
        &pool,
        node,
        WorkItemPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let rollup = planning_rollup(&pool).await.unwrap();
    assert_eq!(row(&rollup, TEST_PROJECT).wi_total, expected_live);
}

/// A project with nothing open still has a row, and it reads `0`. The rail
/// renders every project it shows, so a missing row would render a blank cell
/// where a number belongs — and `0` open is a real, useful answer.
#[tokio::test]
async fn an_empty_project_counts_zero_rather_than_going_missing() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    project_with_category(&pool, "quiet", Some("Other")).await;

    let rollup = planning_rollup(&pool).await.unwrap();
    let quiet = row(&rollup, "quiet");
    assert_eq!(quiet.wi_total, 0);
    assert_eq!(quiet.category.as_deref(), Some("Other"));
}

// --- #1412 ------------------------------------------------------------------

/// F-9 nit 5: `get_attachment` resolves `img-<hex>` to a decimal node id before
/// it reads, so a miss used to answer a hex question in decimal. Both spellings
/// now appear, which beats echoing the caller's — the reader gets the
/// translation whichever one they arrived with.
#[test]
fn the_attachment_miss_names_both_spellings_of_the_id() {
    let msg = korg_core::repo::attachment_not_found(3114);
    assert!(msg.contains("3114"), "{msg}");
    assert!(msg.contains("img-c2a"), "{msg}");
}

/// `ImgId` refuses a non-positive node id, and there is no honest `img-`
/// spelling of one — so the message drops the hex half rather than inventing
/// it. The decimal the caller passed is still named, which is the part that
/// makes the error actionable.
#[test]
fn a_non_positive_id_gets_the_decimal_alone() {
    let msg = korg_core::repo::attachment_not_found(0);
    assert!(msg.contains('0'), "{msg}");
    assert!(!msg.contains("img-"), "{msg}");
}
