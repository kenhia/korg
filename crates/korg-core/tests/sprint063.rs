//! Sprint 063 — due schedules where eyes are (korg:1393).
//!
//! Two work items, one theme: the schedules feature computes due-ness
//! correctly and then puts it nowhere anybody looks (#1385), using a cadence
//! set that could not express the second real schedule anybody filed (#1113).
//!
//! Schedule 1112 sat due for nine days in production, invisible. Everything
//! here is the fence for the surfaces that make that impossible to repeat.

use korg_core::repo::{
    board_rollup, create_schedule, list_schedules, materialize_schedule, update_schedule,
    update_work_item, ArchivedFilter, SchedulePatch, WorkItemPatch,
};
use korg_core::vocab;
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

fn days_ago(days: i64) -> OffsetDateTime {
    OffsetDateTime::now_utc() - Duration::days(days)
}

// --- #1113: the missing member of the week family ---------------------------

/// `fortnightly` is fourteen days — not "about a month", and not `weekly`
/// approximated.
///
/// The gap #1113 found on the feature's first live use: only `weekly`
/// preserves a weekday, because month-based intervals drift off it (2026-08-08
/// is a Saturday, 2026-09-08 is a Tuesday). So "every two weeks on Saturday"
/// had no honest spelling at all, and schedule 1112 was filed `weekly` with a
/// note saying so.
#[tokio::test]
async fn fortnightly_is_fourteen_days() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let anchor = days_ago(30);
    let s = create_schedule(
        &pool,
        new::schedule("infographic", "fortnightly", Some(anchor)),
    )
    .await
    .expect("fortnightly is a cadence");
    assert_eq!(
        s.due_at - s.anchor_at,
        Duration::days(14),
        "fortnightly is two whole weeks, so it keeps the weekday `weekly` keeps"
    );
}

/// The boundary in both directions, through the real predicate rather than the
/// arithmetic: thirteen days is not yet, fifteen days is.
#[tokio::test]
async fn a_fortnightly_schedule_comes_due_on_day_fourteen() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let early = create_schedule(
        &pool,
        new::schedule("not yet", "fortnightly", Some(days_ago(13))),
    )
    .await
    .unwrap();
    assert!(!early.due, "13 days into a fortnight is not due");

    let late = create_schedule(
        &pool,
        new::schedule("overdue", "fortnightly", Some(days_ago(15))),
    )
    .await
    .unwrap();
    assert!(late.due, "15 days into a fortnight is due");
}

/// The whole point of adding the value rather than reaching for `monthly`:
/// a fortnight is strictly between the two neighbours it sits between.
#[tokio::test]
async fn the_week_family_orders_weekly_fortnightly_monthly() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let anchor = days_ago(1);
    let mut due_ats = Vec::new();
    for cadence in ["weekly", "fortnightly", "monthly"] {
        let s = create_schedule(&pool, new::schedule(cadence, cadence, Some(anchor)))
            .await
            .unwrap();
        due_ats.push((cadence, s.due_at));
    }
    assert!(
        due_ats[0].1 < due_ats[1].1 && due_ats[1].1 < due_ats[2].1,
        "weekly < fortnightly < monthly, from one anchor: {due_ats:?}"
    );
}

// --- #1385: the board carries what is due -----------------------------------

/// The finding, inverted into a fence: a due schedule rides the board.
///
/// `get_board` is the read that promises "the whole state of the work", and
/// until this sprint it carried nothing schedule-derived at all — so the two
/// daily-traffic surfaces (kfdc, korg-dash) that render it could not have
/// shown a due schedule even if they wanted to.
#[tokio::test]
async fn the_board_carries_due_schedules() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    create_schedule(
        &pool,
        new::schedule("overdue", "weekly", Some(days_ago(60))),
    )
    .await
    .unwrap();
    create_schedule(&pool, new::schedule("not yet", "yearly", Some(days_ago(1))))
        .await
        .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let titles: Vec<&str> = board
        .due_schedules
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(
        titles,
        vec!["overdue"],
        "only what is due now rides the board — a schedule that is not due yet \
         is not a signal, it is furniture"
    );
}

/// Soonest-due first, the same ordering `list_schedules` promises — so the
/// first row of the block is the thing that has been waiting longest.
#[tokio::test]
async fn board_due_schedules_are_ordered_like_the_list() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    for (title, days) in [("newer", 8), ("oldest", 90), ("middle", 30)] {
        create_schedule(&pool, new::schedule(title, "weekly", Some(days_ago(days))))
            .await
            .unwrap();
    }

    let board = board_rollup(&pool).await.unwrap();
    let titles: Vec<&str> = board
        .due_schedules
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(titles, vec!["oldest", "middle", "newer"]);
}

/// **One definition of due-ness, on every surface.** The board block is
/// `list_schedules(due_only)`'s rows, not a second predicate that agrees with
/// it today — which is the property that makes the three clauses of
/// `schedule_due_sql` (active, no outstanding item, interval elapsed) apply to
/// the board for free.
///
/// The three cases below are exactly the ones a re-derived board query would
/// have got wrong.
#[tokio::test]
async fn the_board_and_the_list_cannot_disagree_about_due() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    // Due.
    create_schedule(&pool, new::schedule("due", "weekly", Some(days_ago(30))))
        .await
        .unwrap();
    // Paused: never comes due, however long the interval has run.
    let paused = create_schedule(&pool, new::schedule("paused", "weekly", Some(days_ago(30))))
        .await
        .unwrap();
    update_schedule(
        &pool,
        paused.node_id,
        SchedulePatch {
            status: Some("paused".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Outstanding: it already fired and the work item it produced is still
    // open, so the item IS the surface and the schedule does not compete.
    let fired = create_schedule(&pool, new::schedule("fired", "weekly", Some(days_ago(30))))
        .await
        .unwrap();
    materialize_schedule(&pool, fired.node_id, false)
        .await
        .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let listed = list_schedules(&pool, None, None, true, ArchivedFilter::default())
        .await
        .unwrap();

    let board_ids: Vec<i64> = board.due_schedules.iter().map(|s| s.node_id).collect();
    let listed_ids: Vec<i64> = listed.items.iter().map(|s| s.node_id).collect();
    assert_eq!(
        board_ids, listed_ids,
        "the board's due block is the due list — same rows, same order"
    );
    let titles: Vec<&str> = board
        .due_schedules
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(titles, vec!["due"], "paused and outstanding are not due");
}

/// A schedule stops riding the board the moment its work item exists, and
/// comes back when that item is finished — the round trip the surface has to
/// get right for it to be trustworthy rather than nagging.
#[tokio::test]
async fn a_materialized_schedule_leaves_the_board_and_returns_when_done() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(&pool, new::schedule("drill", "weekly", Some(days_ago(30))))
        .await
        .unwrap();
    assert_eq!(due_titles(&pool).await, vec!["drill"]);

    let fired = materialize_schedule(&pool, s.node_id, false).await.unwrap();
    assert!(
        due_titles(&pool).await.is_empty(),
        "while its item is open the schedule is not the surface"
    );

    finish(&pool, fired.work_item.wi_number).await;
    // `anchor_mode` is `completed`, so finishing the item advances the anchor
    // to now — the schedule is genuinely not due again for another week.
    assert!(
        due_titles(&pool).await.is_empty(),
        "a completed drill starts its next interval, it does not re-fire"
    );

    update_schedule(
        &pool,
        s.node_id,
        SchedulePatch {
            anchor_at: Some(days_ago(30)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        due_titles(&pool).await,
        vec!["drill"],
        "a finished item leaves the schedule free to come due again"
    );
}

async fn due_titles(pool: &PgPool) -> Vec<String> {
    board_rollup(pool)
        .await
        .unwrap()
        .due_schedules
        .into_iter()
        .map(|s| s.title)
        .collect()
}

async fn finish(pool: &PgPool, wi_number: i64) {
    update_work_item(
        pool,
        wi_number,
        WorkItemPatch {
            wi_status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

/// The board block is **uncapped**, for the reason `sources` is: a cap could
/// push the longest-overdue schedule off the board behind fresher ones, which
/// inverts the failure the block exists to catch. Ten is well past any cap a
/// panel would have been given.
#[tokio::test]
async fn the_due_block_is_uncapped() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    for i in 0..10 {
        create_schedule(
            &pool,
            new::schedule(&format!("due {i}"), "weekly", Some(days_ago(30 + i))),
        )
        .await
        .unwrap();
    }

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.due_schedules.len(), 10);
}

/// A due row renders without a follow-up read: it names its project and what
/// materialising it would create *right now*, substitutions applied. That is
/// the reason the block carries `ScheduleRow` rather than ids — a board panel
/// that has to call back per row is a board panel nobody wires up.
#[tokio::test]
async fn a_due_row_renders_without_a_follow_up_read() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule("drill — {QUARTER} {YEAR}", "weekly", Some(days_ago(30))),
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let [row] = &board.due_schedules[..] else {
        panic!("exactly one due schedule");
    };
    assert_eq!(row.node_id, s.node_id);
    assert_eq!(
        row.project.as_deref(),
        Some(TEST_PROJECT),
        "a due row names its project — maintenance is maintenance *of* something"
    );
    assert_eq!(row.cadence, "weekly");
    assert!(row.due, "every row in the block is due, by construction");
    assert_eq!(
        row.preview_title, s.preview_title,
        "the substituted title the board renders is the one /schedules renders"
    );
    assert!(
        !row.preview_title.contains('{'),
        "substitutions are applied: {}",
        row.preview_title
    );
}

/// Every cadence korg knows has a CHECK constraint that accepts it. The SQL
/// `CASE` is fenced by sprint051's `every_cadence_has_an_interval`; this is the
/// other half — the constraint migration 0028 restates.
#[tokio::test]
async fn the_cadence_check_accepts_every_vocabulary_value() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    for cadence in vocab::SCHEDULE_CADENCES {
        create_schedule(&pool, new::schedule(cadence, cadence, Some(days_ago(1))))
            .await
            .unwrap_or_else(|e| panic!("the schedule_cadence_check rejects {cadence}: {e}"));
    }
}
