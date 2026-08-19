//! Sprint 067 (#1432, #1433, korg:1435) — the flow window's baseline.
//!
//! The bug this suite exists to hold: **a window delta needs a day the window
//! does not contain.** Every row's `backlog` is the count at that day's *end*,
//! so `days[0]` already has its own arrivals and closures applied; differencing
//! the first and last rows measures one day less than the `added`/`closed` sums
//! do. kfdc's Rate of Fire panel labelled both `/6d` and printed `-8` where the
//! flow said `-17` — off by exactly the first day's own net.
//!
//! The fix is korg's, not the consumer's: the response now carries
//! `backlog_before`, the count at the end of the day before `days[0]`, so the
//! delta is answerable from one read and horizon reasoning stays server-side
//! (#1432). `null` at the horizon, never `0` — a zero is a real backlog level.
//!
//! #1433 rides along: the default window widens 6 → 10 now that the log covers
//! eleven days, which is also why the two shipped together — widening while the
//! delta was still off would have stretched the error, not shrunk it.

use korg_core::repo::{
    create_work_item, node_id_for_wi, update_work_item, work_item_flow, NewWorkItem, WorkItemPatch,
    FLOW_DAYS_DEFAULT, FLOW_TRANSITION_HORIZON,
};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use sqlx::PgPool;
use time::Date;

// --- scaffolding ------------------------------------------------------------

async fn wi(pool: &PgPool, title: &str) -> i64 {
    create_work_item(
        pool,
        NewWorkItem {
            project: Some(TEST_PROJECT.into()),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number
}

/// Create a work item that arrived `days_ago` days ago. The flow endpoint reads
/// `node.created` directly, so backdating that column is the seam.
async fn arrived(pool: &PgPool, title: &str, days_ago: i64) -> i64 {
    let n = wi(pool, title).await;
    let node = node_id_for_wi(pool, n).await.unwrap().unwrap();
    sqlx::query("UPDATE node SET created = now() - make_interval(days => $1::int) WHERE id = $2")
        .bind(days_ago)
        .bind(node)
        .execute(pool)
        .await
        .unwrap();
    n
}

/// Close a work item `days_ago` days ago. The status change goes through the
/// real update path — so the transition row is the one production writes — and
/// is then moved in time.
async fn closed_on(pool: &PgPool, wi_number: i64, days_ago: i64) {
    update_work_item(
        pool,
        wi_number,
        WorkItemPatch {
            wi_status: Some("closed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let node = node_id_for_wi(pool, wi_number).await.unwrap().unwrap();
    let moved = sqlx::query(
        "UPDATE transition SET at = now() - make_interval(days => $1::int) \
          WHERE id = (SELECT max(id) FROM transition \
                       WHERE node_id = $2 AND to_status = 'closed')",
    )
    .bind(days_ago)
    .bind(node)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(moved.rows_affected(), 1, "no close transition to backdate");
}

/// Today's date in `tz`, from the same clock the endpoint reads.
async fn local_today(pool: &PgPool, tz: &str) -> Date {
    sqlx::query_scalar("SELECT (now() AT TIME ZONE $1::text)::date")
        .bind(tz)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ===========================================================================
// #1432 — the baseline
// ===========================================================================

/// `backlog_before` is exactly the row a one-day-wider window would have led
/// with. Stated that way the test does not re-derive the arithmetic it is
/// checking: it asks the endpoint the same question two ways and pins the
/// answers together.
///
/// The corpus is built so the two spellings of the delta disagree — three
/// items arrive five days ago, one closes yesterday — because a fixture where
/// `days[0].backlog` happens to equal the baseline would pass with the bug
/// still in place.
#[tokio::test]
async fn the_baseline_is_the_row_a_wider_window_would_have_led_with() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let a = arrived(&pool, "three arrive together", 5).await;
    arrived(&pool, "and stay open", 5).await;
    arrived(&pool, "as does this one", 5).await;
    closed_on(&pool, a, 1).await;

    let narrow = work_item_flow(&pool, Some(2), "UTC").await.unwrap();
    let wider = work_item_flow(&pool, Some(3), "UTC").await.unwrap();

    assert_eq!(narrow.days.len(), 2);
    assert_eq!(wider.days.len(), 3);
    assert_eq!(
        narrow.backlog_before,
        Some(wider.days[0].backlog),
        "the baseline must be the day the wider window starts on"
    );

    // And it is the number the window's own rows cannot supply: differencing
    // the series ends says nothing moved, which is the bug (#1432).
    let baseline = narrow.backlog_before.expect("inside the horizon");
    assert_eq!(baseline, 3, "three open at the end of the day before");
    assert_eq!(narrow.days.last().unwrap().backlog, 2);
    assert_eq!(
        narrow.days.last().unwrap().backlog - baseline,
        -1,
        "the true window delta"
    );
    assert_eq!(
        narrow.days.last().unwrap().backlog - narrow.days[0].backlog,
        0,
        "differencing the series ends loses the first day's own net"
    );
}

/// The invariant that makes a header and its bars agree:
/// `backlog_before + Σ(added − closed) == days.last().backlog`. It is what
/// would have caught this on day one, and it is cheap.
#[tokio::test]
async fn the_baseline_reconciles_with_the_flow_it_precedes() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    // Arrivals and closes on both sides of the window edge, so the sums are
    // not trivially zero and the baseline is not trivially the first row.
    let old_one = arrived(&pool, "arrived long before", 8).await;
    arrived(&pool, "also before", 8).await;
    let mid = arrived(&pool, "arrived mid-window", 2).await;
    arrived(&pool, "arrived today", 0).await;
    closed_on(&pool, old_one, 3).await;
    closed_on(&pool, mid, 1).await;

    for window in [2, 4, 6] {
        let flow = work_item_flow(&pool, Some(window), "UTC").await.unwrap();
        let baseline = flow
            .backlog_before
            .expect("every one of these windows sits inside the horizon");
        let net: i64 = flow.days.iter().map(|d| d.added - d.closed).sum();
        assert_eq!(
            baseline + net,
            flow.days.last().unwrap().backlog,
            "backlog_before + Σ(added − closed) must land on the last row, window {window}"
        );
    }
}

/// At the horizon the baseline is `null`, and emphatically not `0`: the log
/// begins there, so no prior day can be answered honestly. A zero would render
/// as a real backlog level — "the backlog was empty" — which is the same
/// silent-wrong-answer failure the clamp exists to prevent.
///
/// The corpus carries an item that arrived before the horizon, so a `0` here
/// would also be a *wrong* count rather than a coincidentally right one.
#[tokio::test]
async fn a_window_starting_at_the_horizon_has_no_baseline() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    arrived(&pool, "predates the transition log", 30).await;

    let flow = work_item_flow(&pool, Some(100_000), "UTC").await.unwrap();
    assert_eq!(flow.days.first().unwrap().day, FLOW_TRANSITION_HORIZON);
    assert_eq!(
        flow.backlog_before, None,
        "no prior day the log can answer — null, never 0"
    );
}

// ===========================================================================
// #1433 — the widened default
// ===========================================================================

/// The default window is ten days, and the log still covers the day before it.
///
/// Both halves matter. Ten is the widening #1433 made once the horizon had
/// passed far enough (the launch value of six was a coverage decision, not a
/// preference), and the baseline being present at the default is what makes
/// the widening safe to consume: a default window that reached the horizon
/// would hand every caller a `null` delta.
#[tokio::test]
async fn the_default_window_is_ten_days_with_its_baseline_still_in_the_log() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    assert_eq!(FLOW_DAYS_DEFAULT, 10, "#1433 widened the default 6 → 10");

    let flow = work_item_flow(&pool, None, "UTC").await.unwrap();
    assert_eq!(flow.days.len(), FLOW_DAYS_DEFAULT as usize);
    assert_eq!(
        flow.days.last().unwrap().day,
        local_today(&pool, "UTC").await
    );
    assert!(
        flow.backlog_before.is_some(),
        "the default window must leave a day for its own baseline"
    );
    assert!(
        flow.days.first().unwrap().day > FLOW_TRANSITION_HORIZON,
        "the default window starts after the horizon, leaving the horizon day \
         itself to serve as the baseline"
    );
}
