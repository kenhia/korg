//! Sprint 042 — proposal membership is invisible outside a proposal (#824,
//! #813, #823).
//!
//! You can ask a proposal what it covers; nothing could ask a work item
//! whether anything covers it. The 2026-07-31 backlog review answered "which
//! of the 135 open items are already in a live proposal?" with 17
//! `get_proposal` calls and a script, because `covered` only reads one
//! direction. These tests pin the other direction: the membership markers on
//! the row contract (#824/#813) and the same aggregate grouped by project
//! (#823).
//!
//! Deliberately written against the *behaviour*, not the SQL shape. The
//! implementation changed from correlated subqueries to pre-aggregated joins
//! mid-sprint on the strength of `sprint042_measure.rs`, and every test here
//! passed against both — which is the point of pinning answers rather than
//! plans.
//!
//! Two orientation facts the drafts in #813/#824 got wrong, so they are
//! asserted rather than assumed: the `relationship` table's columns are
//! `left_id`/`right_id`/`relationship` (not `left_node_id`/`label`), and the
//! two labels point opposite ways — `covers` is proposal -> work item (the
//! item is the *right* end), `has_handoff` is work item -> handoff (the item
//! is the *left* end).

use korg_core::repo::{
    archived_default, create_handoff, create_project, create_proposal, create_work_item,
    get_work_item, list_work_items, list_work_items_lean, node_id_for_wi, planning_rollup,
    update_proposal, NewHandoff, NewProposal, NewWorkItem, ProposalPatch, WorkItemQuery,
};
use korg_test_support::{fresh_korg, new};
use rust_decimal::Decimal;
use sqlx::PgPool;

fn wi(title: &str) -> NewWorkItem {
    NewWorkItem {
        content: "x".into(),
        ..new::work_item(title)
    }
}

fn proposal(title: &str, covers: Vec<i64>) -> NewProposal {
    NewProposal {
        project_id: None,
        project: None,
        category: None,
        tags: vec![],
        title: title.into(),
        summary: "because reasons".into(),
        notes: None,
        rank: Decimal::new(1, 0),
        pinned: false,
        covers,
    }
}

async fn row(pool: &PgPool, n: i64) -> korg_core::repo::WorkItemRow {
    get_work_item(pool, n).await.unwrap().expect("work item")
}

/// Both new SQL fragments spell their liveness rule as a literal
/// `IN ('proposed', 'active')`, because it sits inside a `concat!`ed const that
/// cannot interpolate a Rust array. This is the fence: add a fifth proposal
/// status on the live side and this fails, rather than the rows quietly
/// under-reporting what is spoken for.
#[test]
fn the_membership_predicate_matches_the_proposal_vocabulary() {
    assert_eq!(
        korg_core::vocab::PROPOSAL_LIVE_STATUSES,
        ["proposed", "active"],
        "membership_joins! and planning_rollup hardcode this pair — update \
         both SQL fragments in repo.rs if the vocabulary moves"
    );
    assert_eq!(
        korg_core::vocab::WI_TERMINAL_STATUSES,
        ["closed"],
        "planning_rollup's denominator excludes exactly the terminal statuses"
    );
}

/// #824's whole question, at row granularity: does this item's row say whether
/// a proposal covers it, and *which* one?
#[tokio::test]
async fn work_item_row_carries_the_proposal_that_covers_it() {
    let (_c, pool) = fresh_korg().await;
    let covered = create_work_item(&pool, wi("covered")).await.unwrap();
    let loose = create_work_item(&pool, wi("loose")).await.unwrap();

    let p = create_proposal(&pool, proposal("a sprint", vec![covered.wi_number]))
        .await
        .unwrap();

    assert_eq!(
        row(&pool, covered.wi_number).await.proposal_node_id,
        Some(p.row.node_id),
        "the covered item names the proposal, not just a boolean — #824 wants \
         to render <id>/<proposal_id>"
    );
    assert_eq!(
        row(&pool, loose.wi_number).await.proposal_node_id,
        None,
        "an item nothing covers is not spoken for"
    );
}

/// The orientation trap. `covers` reads proposal -> work item, so the item is
/// the edge's RIGHT end. A predicate written against `left_id` (as #813's
/// draft SQL was) matches nothing here and would silently report every item as
/// unspoken-for.
#[tokio::test]
async fn covers_matches_on_the_right_end_not_the_left() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("a")).await.unwrap();
    let p = create_proposal(&pool, proposal("sprint", vec![a.wi_number]))
        .await
        .unwrap();

    let a_node = node_id_for_wi(&pool, a.wi_number).await.unwrap().unwrap();
    let (left, right): (i64, i64) =
        sqlx::query_as("SELECT left_id, right_id FROM relationship WHERE relationship = 'covers'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(left, p.row.node_id, "the proposal is the edge's left");
    assert_eq!(right, a_node, "the work item is the edge's right");
}

/// "Already spoken for" is a question about *live* proposals. A declined
/// proposal does not speak for anything, and painting its items in the Ops
/// colour would be a false positive on exactly the question being asked.
#[tokio::test]
async fn only_live_proposals_mark_an_item_as_spoken_for() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("a")).await.unwrap();
    let p = create_proposal(&pool, proposal("sprint", vec![a.wi_number]))
        .await
        .unwrap();

    // proposed -> covered.
    assert!(row(&pool, a.wi_number).await.proposal_node_id.is_some());

    // active is still live — an interrupted sprint still owns its items.
    update_proposal(
        &pool,
        p.row.node_id,
        ProposalPatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        row(&pool, a.wi_number).await.proposal_node_id,
        Some(p.row.node_id),
        "an active proposal owns its items"
    );

    for terminal in ["done", "declined"] {
        update_proposal(
            &pool,
            p.row.node_id,
            ProposalPatch {
                status: Some(terminal.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            row(&pool, a.wi_number).await.proposal_node_id,
            None,
            "a {terminal} proposal no longer speaks for its items"
        );
    }
}

/// #813's handoff ticker. `has_handoff` reads work item -> handoff, so here
/// the item is the LEFT end — the mirror of `covers`, which is why the WI
/// warned against assuming they share a shape.
#[tokio::test]
async fn work_item_row_carries_handoff_presence() {
    let (_c, pool) = fresh_korg().await;
    let with = create_work_item(&pool, wi("has context")).await.unwrap();
    let without = create_work_item(&pool, wi("bare")).await.unwrap();
    let with_node = node_id_for_wi(&pool, with.wi_number)
        .await
        .unwrap()
        .unwrap();

    create_handoff(
        &pool,
        NewHandoff {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            title: "picking this up".into(),
            summary: "state".into(),
            body: "# context".into(),
            related_node_ids: vec![with_node],
            allow_standalone: false,
        },
    )
    .await
    .unwrap();

    assert!(
        row(&pool, with.wi_number).await.has_handoff,
        "an item with durable context waiting says so on its row"
    );
    assert!(!row(&pool, without.wi_number).await.has_handoff);
}

/// The lean/MCP tier (#861's contract) gets the same two markers plus
/// `has_details` — the one the Review page could not show, because
/// `WorkItemSummary` deliberately carries no bodies and the page chose a
/// missing 📝 over a false negative.
#[tokio::test]
async fn the_lean_summary_carries_all_three_markers() {
    let (_c, pool) = fresh_korg().await;
    let bare = create_work_item(&pool, wi("bare")).await.unwrap();
    let rich = create_work_item(
        &pool,
        NewWorkItem {
            details: Some("the long form".into()),
            ..wi("rich")
        },
    )
    .await
    .unwrap();
    let rich_node = node_id_for_wi(&pool, rich.wi_number)
        .await
        .unwrap()
        .unwrap();

    let p = create_proposal(&pool, proposal("sprint", vec![rich.wi_number]))
        .await
        .unwrap();
    create_handoff(
        &pool,
        NewHandoff {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            title: "h".into(),
            summary: "s".into(),
            body: "b".into(),
            related_node_ids: vec![rich_node],
            allow_standalone: false,
        },
    )
    .await
    .unwrap();

    let lean = list_work_items_lean(&pool, None, None, archived_default(), 50, 0)
        .await
        .unwrap();
    let find = |n: i64| lean.items.iter().find(|i| i.wi_number == n).unwrap();

    let r = find(rich.wi_number);
    assert!(
        r.has_details,
        "details is a boolean the projection can afford"
    );
    assert!(r.has_handoff);
    assert_eq!(r.proposal_node_id, Some(p.row.node_id));

    let b = find(bare.wi_number);
    assert!(!b.has_details);
    assert!(!b.has_handoff);
    assert_eq!(b.proposal_node_id, None);

    // An empty-string details is as absent as NULL — the Review page must not
    // offer a 📝 that opens onto nothing.
    sqlx::query("UPDATE workitem SET details = '' WHERE wi_number = $1")
        .bind(rich.wi_number)
        .execute(&pool)
        .await
        .unwrap();
    let lean = list_work_items_lean(&pool, None, None, archived_default(), 50, 0)
        .await
        .unwrap();
    assert!(
        !lean
            .items
            .iter()
            .find(|i| i.wi_number == rich.wi_number)
            .unwrap()
            .has_details
    );
}

/// The REST tier the Work Items page walks carries the markers too — same
/// `WORKITEM_SELECT`, so this is the regression guard on the two staying in
/// step rather than a second mechanism.
#[tokio::test]
async fn the_full_list_read_carries_the_markers() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("a")).await.unwrap();
    let p = create_proposal(&pool, proposal("sprint", vec![a.wi_number]))
        .await
        .unwrap();

    let page = list_work_items(&pool, WorkItemQuery::default())
        .await
        .unwrap();
    let r = page
        .items
        .iter()
        .find(|i| i.wi_number == a.wi_number)
        .unwrap();
    assert_eq!(r.proposal_node_id, Some(p.row.node_id));
    assert!(!r.has_handoff);
}

/// #823's rail count: `<proposals> | <wi_in_proposal> / <wi_total>`, per
/// project, in one query rather than the per-row aggregate grouped by hand.
#[tokio::test]
async fn planning_rollup_counts_proposals_and_coverage_per_project() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "alpha").await.unwrap();
    create_project(&pool, "beta").await.unwrap();
    // A project with nothing in it still has to appear — a rail entry that
    // vanishes when its counts are zero is a rail you cannot click.
    create_project(&pool, "empty").await.unwrap();

    let mut alpha = vec![];
    for t in ["a1", "a2", "a3"] {
        alpha.push(
            create_work_item(
                &pool,
                NewWorkItem {
                    project: Some("alpha".into()),
                    ..wi(t)
                },
            )
            .await
            .unwrap()
            .wi_number,
        );
    }
    let b1 = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("beta".into()),
            ..wi("b1")
        },
    )
    .await
    .unwrap()
    .wi_number;

    // alpha: two proposals, covering 2 of its 3 items.
    create_proposal(
        &pool,
        NewProposal {
            project: Some("alpha".into()),
            ..proposal("alpha one", vec![alpha[0]])
        },
    )
    .await
    .unwrap();
    create_proposal(
        &pool,
        NewProposal {
            project: Some("alpha".into()),
            ..proposal("alpha two", vec![alpha[1]])
        },
    )
    .await
    .unwrap();
    // beta: one proposal covering its single item.
    create_proposal(
        &pool,
        NewProposal {
            project: Some("beta".into()),
            ..proposal("beta one", vec![b1])
        },
    )
    .await
    .unwrap();

    let rollup = planning_rollup(&pool).await.unwrap();
    let get = |name: &str| {
        rollup
            .iter()
            .find(|r| r.project == name)
            .unwrap_or_else(|| panic!("{name} missing from the rollup"))
            .clone()
    };

    let a = get("alpha");
    assert_eq!(a.proposals, 2);
    assert_eq!(a.wi_in_proposal, 2);
    assert_eq!(a.wi_total, 3);

    let b = get("beta");
    assert_eq!(b.proposals, 1);
    assert_eq!(b.wi_in_proposal, 1);
    assert_eq!(b.wi_total, 1);

    let e = get("empty");
    assert_eq!((e.proposals, e.wi_in_proposal, e.wi_total), (0, 0, 0));
}

/// The rollup counts the same *live* proposals the row marker does, or the
/// rail and the rows would disagree about what "spoken for" means while
/// sitting on the same screen.
#[tokio::test]
async fn planning_rollup_ignores_terminal_proposals() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "alpha").await.unwrap();
    let a1 = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("alpha".into()),
            ..wi("a1")
        },
    )
    .await
    .unwrap()
    .wi_number;
    let p = create_proposal(
        &pool,
        NewProposal {
            project: Some("alpha".into()),
            ..proposal("alpha one", vec![a1])
        },
    )
    .await
    .unwrap();

    update_proposal(
        &pool,
        p.row.node_id,
        ProposalPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let rollup = planning_rollup(&pool).await.unwrap();
    let a = rollup.iter().find(|r| r.project == "alpha").unwrap();
    assert_eq!(a.proposals, 0, "a done proposal is off the queue");
    assert_eq!(a.wi_in_proposal, 0, "and stops speaking for its items");
    assert_eq!(a.wi_total, 1, "but the item is still there to be planned");
}
