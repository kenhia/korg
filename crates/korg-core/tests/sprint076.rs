//! Sprint 076 (#1644/#1809/#1879, korg:1944) — three independent slices.
//!
//! Nothing couples them, which is why one file with three sections beats three
//! files: the sprint is a fill-a-sprint bundle, and the record of what it
//! asserted belongs in one place.
//!
//! * `#1879` — comments carry a self-reported `origin` (the D-17 convention
//!   `relate` already carries), so an overseer's clearance reads differently
//!   from a leg's note in the audit trail.
//! * `#1644` — `get_board` carries in-flight materialized schedule work, which
//!   fell into no panel at all the moment a schedule stopped being due.

use korg_core::repo::{
    add_comment, board_rollup, create_proposal, create_schedule, create_work_item,
    get_work_item_detail, list_comments, materialize_schedule, update_comment, update_work_item,
    WorkItemPatch,
};
use korg_test_support::{fresh_korg, new, test_project};
use time::{Duration, OffsetDateTime};

/// A schedule anchored far enough back that every cadence in the vocabulary is
/// already due. The in-flight block is about what happens *after* firing, so
/// each test here needs a schedule it can fire immediately.
fn long_overdue() -> OffsetDateTime {
    OffsetDateTime::now_utc() - Duration::days(500)
}

// --- #1879: comment origin -------------------------------------------------

/// The whole point: two comments on one node, written by different things, are
/// distinguishable afterwards. This is the karc `ship` case — it reads a
/// proposal's thread for the overseer's clearance and could previously prove
/// only that *a* comment existed.
#[tokio::test]
async fn comment_origin_is_stamped_and_read_back() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let p = create_proposal(&pool, new::proposal("origin-bearing thread"))
        .await
        .unwrap()
        .row
        .node_id;

    add_comment(&pool, p, "cleared to ship", Some("overseer"))
        .await
        .unwrap();
    add_comment(&pool, p, "gate is green", Some("overseen-sprint"))
        .await
        .unwrap();

    let thread = list_comments(&pool, p).await.unwrap();
    let origins: Vec<_> = thread.iter().map(|c| c.origin.as_deref()).collect();
    assert_eq!(origins, vec![Some("overseer"), Some("overseen-sprint")]);
}

/// A writer that sends nothing is stored NULL, not stamped with a guess.
///
/// This is the honesty half of D-17 and worth its own test: the tempting
/// implementation derives an origin from the transport ("it came in over MCP,
/// call it `mcp`"), which manufactures provenance the writer never claimed.
/// NULL means unknown; it must stay reachable.
#[tokio::test]
async fn absent_origin_stores_null() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("unstamped"))
        .await
        .unwrap()
        .node_id;

    let c = add_comment(&pool, wi, "no origin sent", None)
        .await
        .unwrap();
    assert_eq!(c.origin, None);
    assert_eq!(list_comments(&pool, wi).await.unwrap()[0].origin, None);
}

/// An edit that sends no origin preserves the existing stamp rather than
/// clearing it — the same shape relate's ON CONFLICT no-op has, and the reason
/// the SQL is `COALESCE($3, origin)` instead of a plain assignment.
///
/// Clearing would be the worse bug of the two available: a comment that *had*
/// provenance silently losing it during an unrelated body fix is exactly the
/// audit-trail hole #1879 exists to close.
#[tokio::test]
async fn update_preserves_origin_unless_a_new_one_is_sent() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("edited"))
        .await
        .unwrap()
        .node_id;

    let c = add_comment(&pool, wi, "first draft", Some("overseer"))
        .await
        .unwrap();

    let untouched = update_comment(&pool, c.id, "second draft", None)
        .await
        .unwrap();
    assert_eq!(untouched.origin.as_deref(), Some("overseer"));

    // An editor that DOES identify itself takes the stamp: it wrote the body
    // that is now there, so claiming the earlier writer's provenance would be
    // the lie in the other direction.
    let restamped = update_comment(&pool, c.id, "third draft", Some("web"))
        .await
        .unwrap();
    assert_eq!(restamped.origin.as_deref(), Some("web"));
}

/// Origin rides the *inlined* comments too, not just `list_comments`. The
/// focused reads are what an agent actually calls, and a field that surfaced on
/// only one of the two paths would be a field agents learn not to trust.
#[tokio::test]
async fn origin_rides_inlined_comments() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("inlined"))
        .await
        .unwrap()
        .node_id;
    add_comment(&pool, wi, "stamped", Some("refill-queue"))
        .await
        .unwrap();

    let item = get_work_item_detail(&pool, wi).await.unwrap().unwrap();
    assert_eq!(item.comments[0].origin.as_deref(), Some("refill-queue"));
}

// --- #1644: in-flight schedules on the board -------------------------------

/// The failure #1644 was filed for, reproduced: a schedule materializes, stops
/// being due, and its open work item lands in no board panel at all.
///
/// Asserting both halves in one test is deliberate — `due_schedules` losing the
/// row is not a regression, it is the correct behaviour that made the work
/// invisible, and the two facts only mean something together.
#[tokio::test]
async fn materialized_schedule_work_stays_on_the_board() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let s = create_schedule(
        &pool,
        new::schedule("check the heater probe", "monthly", Some(long_overdue())),
    )
    .await
    .unwrap()
    .node_id;

    let before = board_rollup(&pool).await.unwrap();
    assert!(
        before.due_schedules.iter().any(|r| r.node_id == s),
        "a schedule anchored in the past is due before it fires"
    );
    assert!(
        before.in_flight_schedules.is_empty(),
        "nothing has materialized yet"
    );

    let wi = materialize_schedule(&pool, s, false)
        .await
        .unwrap()
        .work_item;

    let after = board_rollup(&pool).await.unwrap();
    assert!(
        !after.due_schedules.iter().any(|r| r.node_id == s),
        "materializing ends due-ness — the outstanding-item clause"
    );
    let flight = after
        .in_flight_schedules
        .iter()
        .find(|r| r.node_id == s)
        .expect("the open work item is now the board's surface for this schedule");
    assert_eq!(flight.wi_number, wi.wi_number);
    assert_eq!(flight.wi_title, wi.title);
    assert_eq!(flight.wi_status, "open");
    assert_eq!(flight.project.as_deref(), Some("korg"));
}

/// Finishing the work takes the row off the board. The block tracks *unfinished*
/// scheduled work, so a `done` item leaving is the point, not an omission — and
/// the schedule becomes due again on its own cadence, which is the existing
/// `due_schedules` behaviour this must not disturb.
#[tokio::test]
async fn finished_schedule_work_leaves_the_in_flight_block() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let s = create_schedule(
        &pool,
        new::schedule("rotate the backup drive", "monthly", Some(long_overdue())),
    )
    .await
    .unwrap()
    .node_id;
    let wi = materialize_schedule(&pool, s, false)
        .await
        .unwrap()
        .work_item;

    assert!(board_rollup(&pool)
        .await
        .unwrap()
        .in_flight_schedules
        .iter()
        .any(|r| r.node_id == s));

    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            wi_status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(
        !board_rollup(&pool)
            .await
            .unwrap()
            .in_flight_schedules
            .iter()
            .any(|r| r.node_id == s),
        "a finished item is not in flight"
    );
}

/// `parked` counts as unfinished (#810), so parked scheduled work stays visible.
///
/// This mirrors the `outstanding` clause `due_schedules` already uses — both
/// blocks read unfinished-ness from the same vocabulary, so a status moving
/// between the sets can never make a schedule appear in both or neither.
#[tokio::test]
async fn parked_schedule_work_is_still_in_flight() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let s = create_schedule(
        &pool,
        new::schedule(
            "re-test the flapping sensor",
            "monthly",
            Some(long_overdue()),
        ),
    )
    .await
    .unwrap()
    .node_id;
    let wi = materialize_schedule(&pool, s, false)
        .await
        .unwrap()
        .work_item;

    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            wi_status: Some("parked".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let flight = board
        .in_flight_schedules
        .iter()
        .find(|r| r.node_id == s)
        .expect("parked is deferred, not finished — it must not fall off the board");
    assert_eq!(flight.wi_status, "parked");
    assert!(
        !board.due_schedules.iter().any(|r| r.node_id == s),
        "and it must not be double-counted as due"
    );
}
