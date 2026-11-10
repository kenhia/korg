//! Sprint 098 (#4366, korg:4367) — `cut`, a second terminal work-item status.
//!
//! `cut` is `closed`'s twin: mechanically identical in every derived rule and
//! different only in *why* the item ended — `closed` is finished and accepted,
//! `cut` is decided not to do. Each test below holds one derived rule that
//! `closed` already obeys and that `cut` must obey too, so a reader that grows a
//! fresh `= 'closed'` literal fails here rather than on the board.

use korg_core::repo::{
    board_rollup, create_proposal, create_work_item, list_awaiting, list_work_items_lean,
    node_id_for_wi, relate, search, set_awaiting, update_work_item, work_item_flow, NewProposal,
    NewWorkItem, SearchQuery, WorkItemPatch,
};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use rust_decimal::Decimal;
use sqlx::PgPool;

async fn wi(pool: &PgPool, title: &str, status: &str) -> i64 {
    create_work_item(
        pool,
        NewWorkItem {
            project: Some(TEST_PROJECT.into()),
            wi_status: status.into(),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number
}

async fn set_status(pool: &PgPool, wi_number: i64, status: &str) {
    update_work_item(
        pool,
        wi_number,
        WorkItemPatch {
            wi_status: Some(status.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn node_of(pool: &PgPool, wi_number: i64) -> i64 {
    node_id_for_wi(pool, wi_number).await.unwrap().unwrap()
}

/// Hidden by default, counted under its OWN `omitted` key, and reachable by
/// asking for it by name or with `"all"`.
#[tokio::test]
async fn cut_is_hidden_by_default_and_counted_apart_from_closed() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    wi(&pool, "live", "open").await;
    wi(&pool, "finished", "closed").await;
    let c = wi(&pool, "not doing it", "open").await;
    set_status(&pool, c, "cut").await;

    let default = list_work_items_lean(&pool, None, None, Some(false), 200, 0)
        .await
        .unwrap();
    assert_eq!(default.total, 1, "only the open row is live");
    assert_eq!(default.omitted.closed, 1, "closed counts as closed");
    assert_eq!(
        default.omitted.cut, 1,
        "cut has its own key — folding it into `closed` erases the distinction"
    );

    let only_cut = list_work_items_lean(&pool, None, Some("cut"), Some(false), 200, 0)
        .await
        .unwrap();
    assert_eq!(only_cut.total, 1);
    assert_eq!(only_cut.items[0].wi_number, c);
    assert_eq!(only_cut.omitted.cut, 0, "a status asked for is not omitted");
    assert_eq!(only_cut.omitted.closed, 1);

    let all = list_work_items_lean(&pool, None, Some("all"), Some(false), 200, 0)
        .await
        .unwrap();
    assert_eq!(all.total, 3);
    assert_eq!((all.omitted.closed, all.omitted.cut), (0, 0));
}

/// Search's live scope hides a cut item the way it hides a closed one.
#[tokio::test]
async fn search_live_scope_hides_cut() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let c = wi(&pool, "zanzibar abandoned idea", "open").await;
    set_status(&pool, c, "cut").await;

    let live = search(
        &pool,
        SearchQuery {
            q: "zanzibar".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(live.items.is_empty(), "a cut item is terminal for search");
    assert_eq!(live.omitted.terminal, 1);

    let all = search(
        &pool,
        SearchQuery {
            q: "zanzibar".into(),
            scope: Some("all".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(all.items.len(), 1);
}

/// D-7: `cut` is Ken-only like `closed`, so reaching it answers the ask.
#[tokio::test]
async fn cutting_clears_the_awaiting_marker() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let c = wi(&pool, "should we?", "open").await;
    set_awaiting(
        &pool,
        node_of(&pool, c).await,
        true,
        Some("do we want this"),
    )
    .await
    .unwrap();
    assert_eq!(list_awaiting(&pool).await.unwrap().len(), 1);

    set_status(&pool, c, "cut").await;
    assert!(
        list_awaiting(&pool).await.unwrap().is_empty(),
        "cut is Ken's answer — the ask is moot"
    );
}

/// A cut blocker frees its dependents — the archived-blocker reasoning: a row
/// held blocked forever by work nobody will do is worse than one that frees up.
#[tokio::test]
async fn a_cut_dependency_does_not_block() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let upstream = wi(&pool, "never happening", "open").await;
    let downstream = wi(&pool, "the dependent", "open").await;
    relate(
        &pool,
        node_of(&pool, downstream).await,
        node_of(&pool, upstream).await,
        "depends_on",
        None,
        None,
    )
    .await
    .unwrap();
    create_proposal(
        &pool,
        NewProposal {
            summary: "bundle".into(),
            rank: Decimal::ONE,
            covers: vec![downstream],
            ..new::proposal_in(TEST_PROJECT, "bundle")
        },
    )
    .await
    .unwrap();
    assert_eq!(board_rollup(&pool).await.unwrap().blocked.len(), 1);

    set_status(&pool, upstream, "cut").await;
    assert!(board_rollup(&pool).await.unwrap().blocked.is_empty());
}

/// The rollups gain a `cut` bucket and still sum to `covered_count`.
#[tokio::test]
async fn proposal_rollup_counts_cut_and_still_sums() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = wi(&pool, "done one", "closed").await;
    let b = wi(&pool, "cut one", "open").await;
    set_status(&pool, b, "cut").await;
    let c = wi(&pool, "live one", "open").await;
    create_proposal(
        &pool,
        NewProposal {
            summary: "bundle".into(),
            rank: Decimal::ONE,
            covers: vec![a, b, c],
            ..new::proposal_in(TEST_PROJECT, "bundle")
        },
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let p = &board.queue[0];
    assert_eq!((p.closed, p.cut, p.open), (1, 1, 1));
    assert_eq!(
        p.open + p.resolved + p.done + p.closed + p.parked + p.cut,
        p.covered_count
    );
}

/// Flow's `closed` means left the backlog: a cut counts, and re-labelling a
/// closed item as cut does not count a second exit.
#[tokio::test]
async fn flow_counts_a_cut_as_leaving_the_backlog_once() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = wi(&pool, "cut today", "open").await;
    let b = wi(&pool, "closed then relabelled", "open").await;
    wi(&pool, "still open", "open").await;
    set_status(&pool, a, "cut").await;
    set_status(&pool, b, "closed").await;
    set_status(&pool, b, "cut").await;

    let series = work_item_flow(&pool, Some(1), "UTC").await.unwrap();
    let today = series.days.last().unwrap();
    assert_eq!(
        today.closed, 2,
        "one exit each, the relabel is not a second"
    );
    assert_eq!(
        today.backlog,
        list_work_items_lean(&pool, None, None, Some(false), 200, 0)
            .await
            .unwrap()
            .total,
        "backlog still equals list_work_items' total"
    );
}
