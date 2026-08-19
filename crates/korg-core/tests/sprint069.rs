//! Sprint 069 (#1424, korg:1445) — `queued`, the program state before `active`.
//!
//! Slice 1 of program korg:1447. The board reported every freshly created
//! program as ACTIVE, because `active` was the only thing a new program could
//! be: 0023 gave `program.status` the default `'active'` and `create_program`
//! never wrote the column. kfdc's Operations panel showed a callout saying work
//! was underway on programs nobody had started, which is precisely the failure
//! `holding` was introduced to prevent one step later in the lifecycle.
//!
//! So the fix is `holding`'s argument moved earlier, and these tests pin the
//! three things that argument needs to be true:
//!
//! * **A new program is `queued`.** Both in core (which now writes the column
//!   explicitly, per #526 — the vocabulary is the authority) and in the DB
//!   default (the backstop, which must not disagree).
//! * **`queued` is live.** It rides `list_programs`' default set and therefore
//!   the board's Operations panel, which reads through the same function. Slice
//!   2 (kfdc #1444) has nothing to colour otherwise, and korg+ GP-13 says the
//!   consumer must not reconstruct the state from the slices itself.
//! * **Nothing strands a program in it.** A program is `queued` only while no
//!   slice has started, so every path that can start one promotes it:
//!   `update_proposal` (the slice starts under an existing program),
//!   `create_program` (the program is built over a slice already running) and
//!   `relate` (a running slice is added to a queued program afterwards).
//!
//! Deliberately **not** here: promoting `holding` → `active` when its next
//! slice starts. `holding` is a statement somebody made on purpose, and this
//! sprint's scope is the state before `active`, not a general program state
//! machine. Filed as a follow-up on #1424 instead of smuggled in.

use korg_core::repo::{
    board_rollup, create_program, create_proposal, get_program, list_programs, relate,
    update_program, update_proposal, ArchivedFilter, NewProgram, ProgramPatch, ProposalPatch,
};
use korg_core::vocab::{PROGRAM_INITIAL_STATUS, PROGRAM_LIVE_STATUSES, PROGRAM_STATUSES};
use korg_test_support::{fresh_korg, new, test_project};
use sqlx::{PgPool, Row};

// --- scaffolding ------------------------------------------------------------

async fn proposal(pool: &PgPool, title: &str) -> i64 {
    create_proposal(pool, new::proposal(title))
        .await
        .unwrap()
        .row
        .node_id
}

async fn set_proposal_status(pool: &PgPool, node_id: i64, status: &str) {
    update_proposal(
        pool,
        node_id,
        ProposalPatch {
            status: Some(status.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn program_over(pool: &PgPool, title: &str, slices: Vec<i64>) -> i64 {
    create_program(
        pool,
        NewProgram {
            slices,
            ..new::program(title)
        },
    )
    .await
    .unwrap()
    .row
    .node_id
}

async fn status_of(pool: &PgPool, node_id: i64) -> String {
    get_program(pool, node_id).await.unwrap().unwrap().status
}

async fn transitions(pool: &PgPool, node_id: i64) -> Vec<(String, String)> {
    sqlx::query("SELECT from_status, to_status FROM transition WHERE node_id = $1 ORDER BY id")
        .bind(node_id)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.get("from_status"), r.get("to_status")))
        .collect()
}

// --- the vocabulary ---------------------------------------------------------

/// `queued` is live, not terminal. The partition test in `vocab.rs` already
/// refuses an unclassified fourth status; this pins which side it landed on,
/// because that single choice is what puts a queued program on the board.
#[tokio::test]
async fn queued_is_a_live_program_status() {
    assert!(PROGRAM_STATUSES.contains(&"queued"));
    assert!(
        PROGRAM_LIVE_STATUSES.contains(&"queued"),
        "a program nobody has started is still going to happen — filing it \
         terminal would hide it from the panel that exists to show it"
    );
    assert_eq!(
        PROGRAM_INITIAL_STATUS, "queued",
        "the state a program is born in"
    );
}

// --- a new program is queued ------------------------------------------------

#[tokio::test]
async fn a_new_program_is_queued_not_active() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let slice = proposal(&pool, "slice one").await;

    let program = program_over(&pool, "two-repo change", vec![slice]).await;

    assert_eq!(
        status_of(&pool, program).await,
        "queued",
        "#1424: a program whose first slice is not started must not report ACTIVE"
    );
}

#[tokio::test]
async fn a_program_with_no_slices_yet_is_queued() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let program = program_over(&pool, "nothing planned yet", vec![]).await;

    assert_eq!(status_of(&pool, program).await, "queued");
}

/// The backstop must agree with the authority. 0023 defaulted the column to
/// `'active'`; if 0030 had widened the CHECK without moving the default, a
/// direct INSERT — a migration, a fixture, a future writer — would still mint
/// active programs, and the disagreement would surface as the original bug.
#[tokio::test]
async fn the_database_default_matches_the_vocabulary() {
    let (_c, pool) = fresh_korg().await;

    let node_id: i64 = sqlx::query_scalar(
        "INSERT INTO node (kind, project_id) VALUES ('program', NULL) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO program (node_id, title, aim) VALUES ($1, 'raw', 'raw aim')")
        .bind(node_id)
        .execute(&pool)
        .await
        .unwrap();

    let status: String = sqlx::query_scalar("SELECT status FROM program WHERE node_id = $1")
        .bind(node_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, PROGRAM_INITIAL_STATUS);
}

// --- nothing strands a program in `queued` ----------------------------------

/// The main transition: the slice starts, so the program is under way.
#[tokio::test]
async fn starting_a_slice_promotes_the_queued_program() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let first = proposal(&pool, "slice one").await;
    let second = proposal(&pool, "slice two").await;
    let program = program_over(&pool, "two-repo change", vec![first, second]).await;
    assert_eq!(status_of(&pool, program).await, "queued");

    set_proposal_status(&pool, first, "active").await;

    assert_eq!(
        status_of(&pool, program).await,
        "active",
        "start-sprint marking slice 1 active is what makes the program running"
    );
    assert_eq!(
        transitions(&pool, program).await,
        vec![("queued".to_string(), "active".to_string())],
        "the promotion is a status change like any other and belongs in the log"
    );
}

/// Any slice, not only the first — a program whose second slice is picked up
/// first is unusual but not illegal, and it is running either way.
#[tokio::test]
async fn starting_any_slice_promotes_the_program() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let first = proposal(&pool, "slice one").await;
    let second = proposal(&pool, "slice two").await;
    let program = program_over(&pool, "two-repo change", vec![first, second]).await;

    set_proposal_status(&pool, second, "active").await;

    assert_eq!(status_of(&pool, program).await, "active");
}

#[tokio::test]
async fn every_queued_program_over_the_started_slice_is_promoted() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let shared = proposal(&pool, "shared slice").await;
    let a = program_over(&pool, "program a", vec![shared]).await;
    let b = program_over(&pool, "program b", vec![shared]).await;

    set_proposal_status(&pool, shared, "active").await;

    assert_eq!(status_of(&pool, a).await, "active");
    assert_eq!(
        status_of(&pool, b).await,
        "active",
        "a proposal may be a slice of more than one program (D-2); both started"
    );
}

/// The promotion moves programs *out of* `queued` and touches nothing else. A
/// `holding` program is a deliberate statement — "resting between slices" — and
/// a `done` one is finished; neither is korg's to reopen off a slice edit.
#[tokio::test]
async fn promotion_leaves_every_other_program_status_alone() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    for status in ["holding", "done"] {
        let slice = proposal(&pool, &format!("slice under {status}")).await;
        let program = program_over(&pool, &format!("{status} program"), vec![slice]).await;
        update_program(
            &pool,
            program,
            ProgramPatch {
                status: Some(status.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        set_proposal_status(&pool, slice, "active").await;

        assert_eq!(
            status_of(&pool, program).await,
            status,
            "{status} is somebody's decision; starting a slice does not overturn it"
        );
    }
}

/// The two halves of [`PROPOSAL_STARTED_STATUSES`], which is where "started"
/// is defined and why it is not simply "not `proposed`".
///
/// `declined` is a decision *not* to do the work, so a program whose slice was
/// dropped was never started by it.
#[tokio::test]
async fn a_declined_slice_does_not_promote() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let slice = proposal(&pool, "slice one").await;
    let program = program_over(&pool, "declined before it began", vec![slice]).await;

    set_proposal_status(&pool, slice, "declined").await;

    assert_eq!(
        status_of(&pool, program).await,
        "queued",
        "a slice nobody ran and then dropped never started the program"
    );
    assert!(transitions(&pool, program).await.is_empty());
}

/// …and `done` does, even though the slice skipped `active`. Work that shipped
/// was plainly begun, and a program cannot still be waiting to start on a slice
/// that has already finished. This is the case a "not `proposed`" rule and an
/// "is `active`" rule would each get wrong, in opposite directions.
#[tokio::test]
async fn a_slice_marked_done_without_ever_going_active_promotes() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let slice = proposal(&pool, "shipped in one go").await;
    let program = program_over(&pool, "retroactively recorded", vec![slice]).await;

    set_proposal_status(&pool, slice, "done").await;

    assert_eq!(status_of(&pool, program).await, "active");
}

/// The same distinction on the `create_program` path, so the two definitions
/// cannot drift apart.
#[tokio::test]
async fn a_program_created_over_a_declined_slice_is_still_queued() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let dropped = proposal(&pool, "dropped").await;
    let planned = proposal(&pool, "still planned").await;
    set_proposal_status(&pool, dropped, "declined").await;

    let program = program_over(&pool, "one slice already dropped", vec![dropped, planned]).await;

    assert_eq!(status_of(&pool, program).await, "queued");
}

/// Strand path 2: the program is built over a slice that is already running.
/// The initial status is derived from the slices rather than assumed, so the
/// program is never born lying about work already in flight.
#[tokio::test]
async fn a_program_created_over_a_running_slice_starts_active() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let running = proposal(&pool, "already started").await;
    let later = proposal(&pool, "not started").await;
    set_proposal_status(&pool, running, "active").await;

    let program = program_over(&pool, "retrofitted program", vec![running, later]).await;

    assert_eq!(
        status_of(&pool, program).await,
        "active",
        "a program wrapped around work in flight is in flight"
    );
}

/// Strand path 3: the slice is attached afterwards. `relate` is the documented
/// way to extend a program (api.md), so it is a start path too — and it is the
/// one a rule living only in `update_proposal` would miss.
#[tokio::test]
async fn relating_a_running_slice_into_a_queued_program_starts_it() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let planned = proposal(&pool, "planned slice").await;
    let running = proposal(&pool, "running slice").await;
    set_proposal_status(&pool, running, "active").await;
    let program = program_over(&pool, "extended program", vec![planned]).await;
    assert_eq!(status_of(&pool, program).await, "queued");

    relate(&pool, program, running, "includes", Some("test"), None)
        .await
        .unwrap();

    assert_eq!(status_of(&pool, program).await, "active");
    assert_eq!(
        transitions(&pool, program).await,
        vec![("queued".to_string(), "active".to_string())],
    );
}

/// …and attaching a *planned* slice leaves it queued. The rule is "a slice is
/// running", not "the slice list changed".
#[tokio::test]
async fn relating_a_planned_slice_leaves_the_program_queued() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let planned = proposal(&pool, "planned slice").await;
    let also_planned = proposal(&pool, "also planned").await;
    let program = program_over(&pool, "still queued", vec![planned]).await;

    relate(&pool, program, also_planned, "includes", Some("test"), None)
        .await
        .unwrap();

    assert_eq!(status_of(&pool, program).await, "queued");
    assert!(transitions(&pool, program).await.is_empty());
}

// --- `queued` rides the reads kfdc consumes ---------------------------------

#[tokio::test]
async fn list_programs_returns_queued_by_default() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let program = program_over(&pool, "not started", vec![]).await;

    let list = list_programs(&pool, None, ArchivedFilter::default())
        .await
        .unwrap();

    assert!(
        list.items.iter().any(|p| p.node_id == program),
        "the default list is the live set, and a queued program is live"
    );
    assert_eq!(
        list.omitted.done, 0,
        "queued is shown, so it is not omitted — the envelope counts what the \
         default hid, and it hid nothing here"
    );
}

#[tokio::test]
async fn list_programs_can_ask_for_queued_alone() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let queued = program_over(&pool, "not started", vec![]).await;
    let started = program_over(&pool, "running", vec![]).await;
    update_program(
        &pool,
        started,
        ProgramPatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let list = list_programs(&pool, Some("queued"), ArchivedFilter::default())
        .await
        .unwrap();

    let ids: Vec<i64> = list.items.iter().map(|p| p.node_id).collect();
    assert_eq!(ids, vec![queued]);
}

/// GP-13, the korg+ decision this slice exists under: the value has to ride the
/// read the consumer already makes. The Operations panel is `board_rollup`'s
/// `programs`, which calls `list_programs` — so this is really a test that the
/// shared function stayed shared.
#[tokio::test]
async fn the_board_carries_a_queued_program_with_its_status() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let slice = proposal(&pool, "slice one").await;
    let program = program_over(&pool, "two-repo change", vec![slice]).await;

    let board = board_rollup(&pool).await.unwrap();

    let found = board
        .programs
        .iter()
        .find(|p| p.program.node_id == program)
        .expect("Operations shows live programs, and queued is live");
    assert_eq!(
        found.program.status, "queued",
        "kfdc switches its colour on this literal — korg emits it rather than \
         leaving the board to infer it from the slices (GP-13)"
    );
    assert_eq!(found.slices.len(), 1, "and the slices still ride along");
}
