//! Sprint 062 — the 044–059 re-review's mechanical findings (korg:1392).
//!
//! Every test here is a fence or a regression for one finding of
//! `sprints/review/2026-08-17-044-059-surface-re-review.md`. They share a
//! theme: a vocabulary grew and a hand-written copy of it did not.

use korg_core::repo::{
    board_rollup, create_card, create_program, create_proposal, create_schedule, create_work_item,
    get_program_detail, list_awaiting, materialize_schedule, node_id_for_wi, relate, set_awaiting,
    update_card, BoardProposal, CardPatch, NewProgram, NewWorkItem, ProgramSlice,
};
use korg_core::vocab::{CARD_STATUSES, CARD_TERMINAL_STATUSES, WI_STATUSES};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

// --- F-1 (#1386): every covered-work rollup buckets every status ------------

/// The fence F-1 asked for: a rollup's bucket set **is** [`WI_STATUSES`].
///
/// `wi_statuses_partition_cleanly` fences the vocabulary's own partitions, but
/// nothing asserted that the consumers bucketing *by* status cover it — so
/// sprint 054's fifth status reached both partitions and neither rollup, and a
/// parked covered item counted toward `covered_count` and toward no bucket for
/// three sprints.
///
/// Read off the serialized shape rather than the struct: the field names are
/// the wire contract kfdc and korg's own `/programs` page consume, and the SQL
/// aliases them from the same list. A sixth status now fails here — at
/// `cargo test`, not on the board.
#[test]
fn rollup_buckets_cover_wi_statuses() {
    let slice = serde_json::to_value(ProgramSlice {
        node_id: 1,
        title: "t".into(),
        status: "proposed".into(),
        project: None,
        rank: None,
        covered_count: 0,
        open: 0,
        resolved: 0,
        done: 0,
        closed: 0,
        parked: 0,
    })
    .unwrap();
    let proposal = serde_json::to_value(BoardProposal {
        node_id: 1,
        title: "t".into(),
        summary: "s".into(),
        status: "proposed".into(),
        project: None,
        rank: rust_decimal::Decimal::ZERO,
        pinned: false,
        comment_count: 0,
        covered_count: 0,
        open: 0,
        resolved: 0,
        done: 0,
        closed: 0,
        parked: 0,
        updated: time::OffsetDateTime::UNIX_EPOCH,
        synopsis: None,
    })
    .unwrap();

    for (name, row) in [("ProgramSlice", &slice), ("BoardProposal", &proposal)] {
        let obj = row.as_object().unwrap();
        for status in WI_STATUSES {
            assert!(
                obj.contains_key(status),
                "{name} has no `{status}` bucket — a covered work item in that \
                 status would land in `covered_count` and in no bucket, and the \
                 counts would stop summing to the total"
            );
        }
    }
}

/// The regression itself, on both rollups at once: a proposal covering one item
/// of every status reports one per bucket, and the buckets sum to
/// `covered_count`. The sum is the property F-1 broke and the doc comment
/// claimed.
#[tokio::test]
async fn covered_rollups_bucket_a_parked_item_and_still_sum() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let proposal = create_proposal(&pool, new::proposal_in(TEST_PROJECT, "every status"))
        .await
        .unwrap()
        .row
        .node_id;
    for status in WI_STATUSES {
        let wi = wi_node(&pool, &format!("a {status} item"), status).await;
        relate(&pool, proposal, wi, "covers", None, None)
            .await
            .unwrap();
    }
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![proposal],
            ..new::program("rollup program")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    let n = WI_STATUSES.len() as i64;

    let slice = get_program_detail(&pool, program)
        .await
        .unwrap()
        .unwrap()
        .slices
        .remove(0);
    assert_eq!(slice.covered_count, n);
    assert_eq!(slice.parked, 1, "get_program's slice rollup buckets parked");
    assert_eq!(
        slice.open + slice.resolved + slice.done + slice.closed + slice.parked,
        slice.covered_count,
        "a slice's buckets must sum to its covered_count"
    );

    let board = board_rollup(&pool).await.unwrap();
    let row = board
        .queue
        .iter()
        .find(|p| p.node_id == proposal)
        .expect("the proposal is in the board's queue");
    assert_eq!(row.covered_count, n);
    assert_eq!(row.parked, 1, "the board's cov CTE buckets parked");
    assert_eq!(
        row.open + row.resolved + row.done + row.closed + row.parked,
        row.covered_count,
        "BoardProposal's counts must sum to covered_count — the claim its doc \
         comment makes"
    );
    // And the program's slices on the board come from the same pass, so they
    // cannot disagree with the focused read.
    let board_slice = &board.programs[0].slices[0];
    assert_eq!(board_slice.parked, slice.parked);
}

// --- F-6 (#1389): the awaiting lane clears for cards ------------------------

/// D-7's rule is "a state only Ken sets clears the marker". `set_awaiting`
/// invites marking a card; the clearing machinery knew three kinds and not
/// cards, so a card marked awaiting and then moved to `Done` — or `Cut`, where
/// the ask is moot the way archived is — sat in the Commander's Call lane until
/// someone cleared it by hand.
#[tokio::test]
async fn a_card_reaching_done_or_cut_clears_its_awaiting_marker() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    for terminal in ["Done", "Cut"] {
        let card = create_card(&pool, new::card(&format!("ask about {terminal}")))
            .await
            .unwrap()
            .node_id;
        set_awaiting(&pool, card, true, Some("needs Ken"))
            .await
            .unwrap();
        assert!(
            list_awaiting(&pool)
                .await
                .unwrap()
                .iter()
                .any(|r| r.node_id == card),
            "the card is in the lane before it settles"
        );

        update_card(&pool, card, card_status(terminal))
            .await
            .unwrap();

        assert!(
            !list_awaiting(&pool)
                .await
                .unwrap()
                .iter()
                .any(|r| r.node_id == card),
            "a `{terminal}` card must leave the lane — the kanban is Ken's \
             board, so reaching a terminal column is him answering"
        );
    }
}

/// The other half of D-7, and the reason the clearing rule is not "any status
/// change": a card still moving through the board is still waiting on Ken.
#[tokio::test]
async fn moving_a_card_short_of_done_keeps_its_marker() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let card = create_card(&pool, new::card("still waiting"))
        .await
        .unwrap()
        .node_id;
    set_awaiting(&pool, card, true, None).await.unwrap();
    for status in CARD_STATUSES
        .iter()
        .filter(|s| !CARD_TERMINAL_STATUSES.contains(s))
    {
        update_card(&pool, card, card_status(status)).await.unwrap();
        assert!(
            list_awaiting(&pool)
                .await
                .unwrap()
                .iter()
                .any(|r| r.node_id == card),
            "`{status}` is not Ken answering — the ask stands"
        );
    }
}

// --- F-7 (#1390): the materialize gates re-run under the lock ---------------

/// The pre-transaction gates are a fast path, not the guarantee. Until #1390
/// they were the *only* check: two callers — realistically one agent retrying a
/// call that timed out — could both read "nothing outstanding", queue on the
/// `FOR UPDATE`, and both insert, producing the duplicate drill 051 D-2 exists
/// to prevent.
///
/// The window is reproduced rather than approximated, because a test that
/// cannot tell the pre-check from the in-transaction one would pass with the
/// fix reverted. This test holds the schedule row, waits until the call under
/// test is *demonstrably* blocked on that lock — every pre-check behind it —
/// then commits what the winning transaction would have committed and asserts
/// the refusal comes anyway.
#[tokio::test(flavor = "multi_thread")]
async fn a_racing_materialization_is_refused_after_the_lock_not_only_before() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let schedule = create_schedule(
        &pool,
        new::schedule(
            "restore drill",
            "quarterly",
            Some(OffsetDateTime::now_utc() - Duration::days(200)),
        ),
    )
    .await
    .unwrap();
    // The item the other caller's transaction produced.
    let racer = wi_node(&pool, "the copy the winner made", "open").await;

    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM schedule WHERE node_id = $1 FOR UPDATE")
        .bind(schedule.node_id)
        .fetch_one(&mut *holder)
        .await
        .unwrap();

    let node_id = schedule.node_id;
    let racing_pool = pool.clone();
    let loser =
        tokio::spawn(async move { materialize_schedule(&racing_pool, node_id, false).await });
    await_lock_wait(&pool).await;

    sqlx::query("UPDATE schedule SET last_wi_id = $2 WHERE node_id = $1")
        .bind(node_id)
        .bind(racer)
        .execute(&mut *holder)
        .await
        .unwrap();
    holder.commit().await.unwrap();

    let err = loser
        .await
        .unwrap()
        .expect_err("the second materialization must refuse")
        .to_string();
    assert!(
        err.contains("already materialized"),
        "the in-transaction re-check refuses with the outstanding-item message, \
         the one `force` never lifts — got: {err}"
    );
    let produced: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM relationship WHERE left_id = $1 AND relationship = 'materializes'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(produced, 0, "the refused call must write nothing at all");
}

// --- helpers ----------------------------------------------------------------

/// Block until some session is waiting on a lock — i.e. the call under test has
/// cleared its pre-transaction gates and reached the `FOR UPDATE`. Polling
/// beats a sleep: a sleep that is too short turns this into a test of the
/// pre-check, silently.
async fn await_lock_wait(pool: &PgPool) {
    for _ in 0..600 {
        let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_locks WHERE NOT granted")
            .fetch_one(pool)
            .await
            .unwrap();
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("nothing ever blocked on the schedule row lock");
}

/// A card move, which is all these tests ever patch.
fn card_status(status: &str) -> CardPatch {
    CardPatch {
        status: Some(status.into()),
        ..Default::default()
    }
}

/// A work item in the test project with a given status, returning its node id.
async fn wi_node(pool: &PgPool, title: &str, status: &str) -> i64 {
    let wi = create_work_item(
        pool,
        NewWorkItem {
            project: Some(TEST_PROJECT.into()),
            wi_status: status.into(),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number;
    node_id_for_wi(pool, wi).await.unwrap().unwrap()
}
