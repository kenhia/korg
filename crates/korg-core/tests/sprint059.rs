//! Sprint 059 (#1318, korg:1320) — the work-item flow series, korg's one
//! time-series read.
//!
//! The principle this suite exists to hold: **a day the data cannot answer is
//! absent, never zero.** The whole sprint came out of a backlog review whose
//! instrument (the raw open count) looked flat while the backlog tripled; a
//! flow endpoint that zero-filled days past the transition horizon would
//! rebuild that lie one endpoint over. Hence:
//!
//! 1. `added` comes from `node.created` on every path — creating a work item
//!    writes no transition row, so a log-derived `added` is silently zero.
//! 2. `closed` and the backlog reconstruction come from the transition log
//!    alone, and the window is clamped to the log's horizon.
//! 3. Today's `backlog` must equal what `list_work_items` reports as `total`
//!    — two reads that disagree about "how many are open" is a bug, not a
//!    nuance.

use korg_core::repo::{
    archived_default, create_work_item, list_work_items_lean, node_id_for_wi, update_work_item,
    work_item_flow, NewWorkItem, WorkItemFlowSeries, WorkItemPatch, FLOW_DAYS_DEFAULT,
    FLOW_DURABLE_AFTER_DAYS, FLOW_TRANSITION_HORIZON,
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

async fn set_wi_status(pool: &PgPool, wi_number: i64, status: &str) {
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

/// Backdate a work item's creation by whole days. The flow endpoint reads
/// `node.created` directly, so this is the seam for "an item that arrived
/// N days ago".
async fn backdate_created(pool: &PgPool, wi_number: i64, days_ago: i64) {
    let node = node_id_for_wi(pool, wi_number).await.unwrap().unwrap();
    sqlx::query("UPDATE node SET created = now() - make_interval(days => $1::int) WHERE id = $2")
        .bind(days_ago)
        .bind(node)
        .execute(pool)
        .await
        .unwrap();
}

/// Backdate the newest transition into `to_status` on a work item. Status
/// changes go through the real update path (so the log rows are the ones
/// production writes) and are then moved in time.
async fn backdate_transition(pool: &PgPool, wi_number: i64, to_status: &str, days_ago: i64) {
    let node = node_id_for_wi(pool, wi_number).await.unwrap().unwrap();
    let moved = sqlx::query(
        "UPDATE transition SET at = now() - make_interval(days => $1::int) \
          WHERE id = (SELECT max(id) FROM transition \
                       WHERE node_id = $2 AND to_status = $3)",
    )
    .bind(days_ago)
    .bind(node)
    .bind(to_status)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(moved.rows_affected(), 1, "no transition to backdate");
}

/// Today's date in `tz`, from the same clock the endpoint reads.
async fn local_today(pool: &PgPool, tz: &str) -> Date {
    sqlx::query_scalar("SELECT (now() AT TIME ZONE $1::text)::date")
        .bind(tz)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The row for `day`, or a panic naming the series — a wrong date must fail
/// as "day X is missing", not as an off-by-one index.
fn on(flow: &WorkItemFlowSeries, day: Date) -> &korg_core::repo::WorkItemFlowDay {
    flow.days
        .iter()
        .find(|d| d.day == day)
        .unwrap_or_else(|| panic!("no row for {day}; series has {:?}", flow.days))
}

// ===========================================================================
// Shape: window, ordering, clamp, self-description
// ===========================================================================

/// The default window: six rows, ascending, ending today, every count zero on
/// an empty corpus — and the response names its own contract (horizon,
/// timezone, durable threshold) so a consumer never hardcodes them.
#[tokio::test]
async fn an_empty_corpus_yields_the_default_window_of_zeros() {
    let (_c, pool) = fresh_korg().await;

    let flow = work_item_flow(&pool, None, "UTC").await.unwrap();
    assert_eq!(flow.days.len(), FLOW_DAYS_DEFAULT as usize);
    assert_eq!(flow.horizon, FLOW_TRANSITION_HORIZON);
    assert_eq!(flow.timezone, "UTC");
    assert_eq!(flow.durable_after_days, FLOW_DURABLE_AFTER_DAYS);

    let today = local_today(&pool, "UTC").await;
    assert_eq!(flow.days.last().unwrap().day, today, "series ends today");
    assert!(
        flow.days.windows(2).all(|w| w[0].day < w[1].day),
        "ascending, no duplicates"
    );
    for d in &flow.days {
        assert_eq!(
            (
                d.added,
                d.closed,
                d.backlog,
                d.added_durable,
                d.closed_durable
            ),
            (0, 0, 0, 0, 0),
            "empty corpus, day {}",
            d.day
        );
    }
}

/// A window reaching past the horizon is clamped to it — the series starts at
/// the horizon and is simply shorter than asked, never zero-filled back to
/// the requested depth.
#[tokio::test]
async fn a_window_past_the_horizon_is_clamped_to_it() {
    let (_c, pool) = fresh_korg().await;

    let flow = work_item_flow(&pool, Some(100_000), "UTC").await.unwrap();
    let today = local_today(&pool, "UTC").await;
    let expected = (today - FLOW_TRANSITION_HORIZON).whole_days() + 1;
    assert_eq!(flow.days.len(), expected as usize);
    assert_eq!(flow.days.first().unwrap().day, FLOW_TRANSITION_HORIZON);

    // The floor: a nonsense window still serves today.
    let floor = work_item_flow(&pool, Some(0), "UTC").await.unwrap();
    assert_eq!(floor.days.len(), 1);
    assert_eq!(floor.days[0].day, today);
}

// ===========================================================================
// added — from node.created, never the log
// ===========================================================================

/// Arrivals land on the local day they were created, from `node.created` —
/// which exists for every item, transition log or no transition log.
#[tokio::test]
async fn added_comes_from_created_and_lands_on_the_local_day() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    wi(&pool, "today's arrival").await;
    let b = wi(&pool, "yesterday's first").await;
    let c = wi(&pool, "yesterday's second").await;
    backdate_created(&pool, b, 1).await;
    backdate_created(&pool, c, 1).await;

    let flow = work_item_flow(&pool, None, "UTC").await.unwrap();
    let today = local_today(&pool, "UTC").await;
    let yesterday = today.previous_day().unwrap();

    assert_eq!(on(&flow, today).added, 1);
    assert_eq!(on(&flow, yesterday).added, 2);
    assert_eq!(
        flow.days.iter().map(|d| d.added).sum::<i64>(),
        3,
        "nothing lands anywhere else"
    );
    // None of them has lived past the durable threshold.
    assert!(flow.days.iter().all(|d| d.added_durable == 0));
    // Backlog accumulates: two open at yesterday's end, three at today's.
    assert_eq!(on(&flow, yesterday).backlog, 2);
    assert_eq!(on(&flow, today).backlog, 3);
}

// ===========================================================================
// closed + backlog — the transition log, walked honestly
// ===========================================================================

/// The durable split on the closed side: an item that lived a month and an
/// item that lived under a day close on the same day, and the series tells
/// them apart — that distinction is the entire point of the endpoint.
#[tokio::test]
async fn closed_splits_durable_from_same_day_churn() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    // X: filed a month ago, closed yesterday — durable drawdown.
    let x = wi(&pool, "long-lived").await;
    backdate_created(&pool, x, 30).await;
    set_wi_status(&pool, x, "closed").await;
    backdate_transition(&pool, x, "closed", 1).await;

    // Z: filed and closed yesterday — same-day churn.
    let z = wi(&pool, "churn").await;
    backdate_created(&pool, z, 1).await;
    set_wi_status(&pool, z, "closed").await;
    backdate_transition(&pool, z, "closed", 1).await;

    let flow = work_item_flow(&pool, None, "UTC").await.unwrap();
    let today = local_today(&pool, "UTC").await;
    let yesterday = today.previous_day().unwrap();

    let y = on(&flow, yesterday);
    assert_eq!(y.closed, 2, "both closed yesterday");
    assert_eq!(y.closed_durable, 1, "only the month-old one is durable");
    assert_eq!(
        y.added, 1,
        "Z arrived yesterday; X arrived outside the window"
    );
    assert_eq!(on(&flow, today).closed, 0);

    // Backlog: X was open through the earlier days, gone from yesterday on.
    for d in &flow.days {
        let expected = if d.day < yesterday { 1 } else { 0 };
        assert_eq!(d.backlog, expected, "backlog on {}", d.day);
    }
}

/// Status at a day's end is reconstructed by un-walking the log: a close
/// counts once per day it happened, a reopened item rejoins the backlog, and
/// the days in between report it closed.
#[tokio::test]
async fn a_reopened_item_counts_its_close_and_rejoins_the_backlog() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, "reopened").await;
    backdate_created(&pool, n, 20).await;
    set_wi_status(&pool, n, "closed").await;
    backdate_transition(&pool, n, "closed", 3).await;
    set_wi_status(&pool, n, "open").await;
    backdate_transition(&pool, n, "open", 1).await;

    let flow = work_item_flow(&pool, None, "UTC").await.unwrap();
    let today = local_today(&pool, "UTC").await;
    let day = |ago: i64| today - time::Duration::days(ago);

    assert_eq!(on(&flow, day(3)).closed, 1);
    assert_eq!(on(&flow, day(3)).closed_durable, 1, "it had lived 17 days");
    assert_eq!(flow.days.iter().map(|d| d.closed).sum::<i64>(), 1);

    // Open, open, closed, closed, open again, open.
    let expected = [(5, 1), (4, 1), (3, 0), (2, 0), (1, 1), (0, 1)];
    for (ago, backlog) in expected {
        assert_eq!(on(&flow, day(ago)).backlog, backlog, "{ago} days ago");
    }
}

// ===========================================================================
// The invariant the spec names: today's backlog == list_work_items total
// ===========================================================================

/// Two reads, one answer. `parked` is live (non-`closed`), archived is out of
/// both — if this drifts, one of the two reads is lying about how many items
/// are open.
#[tokio::test]
async fn todays_backlog_equals_the_list_work_items_total() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    wi(&pool, "open").await;
    let parked = wi(&pool, "parked").await;
    set_wi_status(&pool, parked, "parked").await;
    let closed = wi(&pool, "closed").await;
    set_wi_status(&pool, closed, "closed").await;
    let archived = wi(&pool, "archived").await;
    update_work_item(
        &pool,
        archived,
        WorkItemPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let flow = work_item_flow(&pool, None, "UTC").await.unwrap();
    let list = list_work_items_lean(&pool, None, None, archived_default(), 50, 0)
        .await
        .unwrap();
    assert_eq!(
        flow.days.last().unwrap().backlog,
        list.total,
        "flow's today and list_work_items disagree about the open count"
    );
    assert_eq!(flow.days.last().unwrap().backlog, 2, "open + parked");
}

// ===========================================================================
// Timezone — days are bucketed where the board lives
// ===========================================================================

/// One instant, two timezones, two different days. An item created just after
/// UTC midnight belongs to yesterday in Anchorage; a series that bucketed in
/// UTC regardless of the asked timezone would put it on the wrong bar.
#[tokio::test]
async fn days_are_bucketed_in_the_requested_timezone() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, "midnight-adjacent").await;
    let node = node_id_for_wi(&pool, n).await.unwrap().unwrap();
    // 00:30 UTC today — unambiguously today in UTC, unambiguously the
    // previous calendar day in America/Anchorage (UTC-8/-9 year-round range).
    sqlx::query(
        "UPDATE node SET created = \
             date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' \
             + interval '30 minutes' \
          WHERE id = $1",
    )
    .bind(node)
    .execute(&pool)
    .await
    .unwrap();

    let utc_day: Date = local_today(&pool, "UTC").await;
    let anchorage_day: Date = sqlx::query_scalar(
        "SELECT ((date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' \
                  + interval '30 minutes') AT TIME ZONE 'America/Anchorage')::date",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_ne!(utc_day, anchorage_day, "the two calendars must disagree");

    let utc = work_item_flow(&pool, Some(3), "UTC").await.unwrap();
    assert_eq!(on(&utc, utc_day).added, 1);

    let anc = work_item_flow(&pool, Some(3), "America/Anchorage")
        .await
        .unwrap();
    assert_eq!(anc.timezone, "America/Anchorage");
    assert_eq!(on(&anc, anchorage_day).added, 1);
    assert_eq!(
        anc.days.iter().map(|d| d.added).sum::<i64>(),
        1,
        "the one arrival lands on exactly one Anchorage day"
    );
}
