//! Sprint 072 (#1534 + #1535, korg:1546) — `parked` for proposals and programs.
//!
//! Slice 1 of program korg:1549. korg:1478 sat `active` for a month because
//! kai's 5090 was out for RMA, carrying a comment asking people not to pick it
//! up from Planning, and korg:1480 sat `holding` — true about the motion and
//! wrong about the reason. A comment doing a status's job is the tell that a
//! status is missing, and `declined` was the only alternative the vocabulary
//! offered: a decision *not* to do the work, which says the opposite thing.
//!
//! #810 settled the semantics for work items and this sprint does not reopen
//! them. What it pins here is that the same three properties survived the trip
//! to two more node kinds, plus the two decisions the work items demanded be
//! answered identically:
//!
//! * **Live, not terminal.** A parked row stays in the default read. Hiding it
//!   would make `parked` a slower spelling of `declined`.
//! * **Below the line.** Every queue read sorts parked last, *outside* `pinned`
//!   — the divider is absolute and pinning orders within a half.
//! * **Unfinished.** Deconfliction still reports a parked blocker, and an
//!   awaiting marker on a parked row persists: parking is not answering.
//! * **`get_board`'s shape**, the cross-project seam (GP-19): parked proposals
//!   ride at the end of `queue` and parked programs at the end of `programs`,
//!   never in buckets of their own.
//! * **Declared, not derived.** korg never sets or clears `parked` — most
//!   sharply, nothing promotes a parked program when a slice starts under it.
//!
//! Deliberately **not** here: any notion of what un-parking should restore.
//! That is the point — the status a row was parked out of is not recoverable
//! from the row, so korg refuses to guess and the caller names the target.

use korg_core::repo::{
    board_rollup, create_program, create_proposal, create_work_item, get_program, get_work_item,
    list_awaiting, list_programs, list_proposals_lean, relate, search, set_awaiting,
    update_program, update_proposal, update_work_item, ArchivedFilter, NewProgram, NewProposal,
    PageQuery, ProgramPatch, ProposalPatch, SearchQuery, WorkItemPatch,
};
use korg_core::vocab::{
    PARKED_STATUS, PROGRAM_LIVE_STATUSES, PROGRAM_TERMINAL_STATUSES, PROPOSAL_LIVE_STATUSES,
    PROPOSAL_STARTED_STATUSES, PROPOSAL_TERMINAL_STATUSES,
};
use korg_test_support::{fresh_korg, new, test_project};
use rust_decimal::Decimal;
use sqlx::PgPool;

// --- scaffolding ------------------------------------------------------------

async fn proposal(pool: &PgPool, title: &str) -> i64 {
    create_proposal(pool, new::proposal(title))
        .await
        .unwrap()
        .row
        .node_id
}

async fn ranked_proposal(pool: &PgPool, title: &str, rank: i64, pinned: bool) -> i64 {
    create_proposal(
        pool,
        NewProposal {
            rank: Decimal::new(rank, 0),
            pinned,
            ..new::proposal(title)
        },
    )
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

async fn set_program_status(pool: &PgPool, node_id: i64, status: &str) {
    update_program(
        pool,
        node_id,
        ProgramPatch {
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

async fn program_status(pool: &PgPool, node_id: i64) -> String {
    get_program(pool, node_id).await.unwrap().unwrap().status
}

// --- the vocabulary landed on the right side --------------------------------

/// `vocab.rs`'s partition fences already refuse an *unclassified* status; these
/// pin which side each one landed on, because that single choice is the whole
/// feature. A `parked` filed terminal would partition just as cleanly and be
/// invisible, which is `declined` with extra steps.
#[tokio::test]
async fn parked_is_live_on_both_kinds() {
    assert!(PROPOSAL_LIVE_STATUSES.contains(&PARKED_STATUS));
    assert!(!PROPOSAL_TERMINAL_STATUSES.contains(&PARKED_STATUS));
    assert!(PROGRAM_LIVE_STATUSES.contains(&PARKED_STATUS));
    assert!(!PROGRAM_TERMINAL_STATUSES.contains(&PARKED_STATUS));
    assert!(
        !PROPOSAL_STARTED_STATUSES.contains(&PARKED_STATUS),
        "parked says deferred, never begun — see \
         parked_slices_do_not_promote_a_queued_program"
    );
}

/// The DB half. 0031 converted `sprint_proposal.status` from a PG enum to
/// TEXT + CHECK, so the value the vocabulary admits must be one the column
/// accepts — and the enum must be gone, or the conversion left a second
/// authority behind that a later widen would have to remember.
#[tokio::test]
async fn the_proposal_status_column_is_text_and_admits_parked() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let p = proposal(&pool, "blocked on hardware").await;

    set_proposal_status(&pool, p, PARKED_STATUS).await;

    let kind: String = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns \
          WHERE table_name = 'sprint_proposal' AND column_name = 'status'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kind, "text", "0031 §1: enum -> TEXT + CHECK");

    let enum_survives: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'sprint_proposal_status')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !enum_survives,
        "the enum must be dropped: a leftover type is a second authority"
    );
}

/// The backstop must refuse what the vocabulary refuses. korg-core validates
/// first (#526), so this reaches past it to the constraint itself — the guard
/// against a writer that is not `update_proposal`.
#[tokio::test]
async fn the_check_constraints_refuse_an_unknown_status() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let p = proposal(&pool, "a sprint").await;
    let g = program_over(&pool, "a program", vec![]).await;

    for (table, node_id) in [("sprint_proposal", p), ("program", g)] {
        let refused = sqlx::query(&format!(
            "UPDATE {table} SET status = 'dormant' WHERE node_id = $1"
        ))
        .bind(node_id)
        .execute(&pool)
        .await;
        assert!(
            refused.is_err(),
            "{table}.status must reject a value outside the vocabulary"
        );
    }
}

// --- live, and below the line -----------------------------------------------

/// The two halves of "visible but de-prioritised", in one read. A parked
/// proposal is *in* the default queue (hiding it defeats the status) and *last*
/// in it (it must not sit among the rows you are choosing from).
#[tokio::test]
async fn a_parked_proposal_stays_in_the_queue_and_sorts_last() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let first = ranked_proposal(&pool, "rank 1", 1, false).await;
    let second = ranked_proposal(&pool, "rank 2", 2, false).await;
    let parked = ranked_proposal(&pool, "rank 0, parked", 0, false).await;

    set_proposal_status(&pool, parked, PARKED_STATUS).await;

    let listed = list_proposals_lean(&pool, None, None, ArchivedFilter::default())
        .await
        .unwrap();
    let order: Vec<i64> = listed.items.iter().map(|p| p.node_id).collect();
    assert_eq!(
        order,
        vec![first, second, parked],
        "parked is live so it stays, and sorts last despite the lowest rank"
    );
    assert_eq!(
        listed.omitted.done, 0,
        "nothing was hidden, so nothing is reported hidden"
    );
    assert_eq!(listed.omitted.declined, 0);
}

/// The divider is **absolute**: `pinned` orders within a half, never across it.
/// Pinning a parked proposal is "this one first, when it comes back" — reading
/// it as "this one now" would put an unstartable row at the top of the read
/// whose entire question is what to start next.
#[tokio::test]
async fn pinning_a_parked_proposal_does_not_lift_it_above_the_divider() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let ordinary = ranked_proposal(&pool, "unpinned and live", 9, false).await;
    let parked = ranked_proposal(&pool, "pinned but parked", 1, true).await;

    set_proposal_status(&pool, parked, PARKED_STATUS).await;

    let listed = list_proposals_lean(&pool, None, None, ArchivedFilter::default())
        .await
        .unwrap();
    let order: Vec<i64> = listed.items.iter().map(|p| p.node_id).collect();
    assert_eq!(order, vec![ordinary, parked]);
}

#[tokio::test]
async fn a_parked_program_stays_in_the_list_and_sorts_last() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let running = program_over(&pool, "still going", vec![]).await;
    let parked = program_over(&pool, "dormant", vec![]).await;

    set_program_status(&pool, parked, PARKED_STATUS).await;

    let listed = list_programs(&pool, None, ArchivedFilter::default())
        .await
        .unwrap();
    let order: Vec<i64> = listed.items.iter().map(|p| p.node_id).collect();
    assert_eq!(order, vec![running, parked]);
    assert_eq!(
        listed.omitted.done, 0,
        "a parked program is dormant, not finished — counting it in \
         `omitted.done` would claim it had shipped"
    );
}

// --- the board shape (GP-19, the cross-project seam) -------------------------

/// The decision both work items had to answer identically, and the one kfdc
/// filters on. Parked rides the collection it would otherwise vanish from,
/// last, with `status` telling a consumer which rows they are — rather than a
/// third bucket that would make every consumer merge two lists to render the
/// ordinary case.
#[tokio::test]
async fn the_board_carries_parked_at_the_end_of_the_queue() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let queued = ranked_proposal(&pool, "on deck", 5, false).await;
    let running = ranked_proposal(&pool, "in flight", 6, false).await;
    let from_proposed = ranked_proposal(&pool, "parked before starting", 1, false).await;
    let from_active = ranked_proposal(&pool, "parked mid-flight", 2, false).await;

    set_proposal_status(&pool, running, "active").await;
    set_proposal_status(&pool, from_proposed, PARKED_STATUS).await;
    set_proposal_status(&pool, from_active, "active").await;
    set_proposal_status(&pool, from_active, PARKED_STATUS).await;

    let board = board_rollup(&pool).await.unwrap();

    assert_eq!(
        board.active.iter().map(|p| p.node_id).collect::<Vec<_>>(),
        vec![running],
        "a parked row must not sit in Fire Missions — it cannot move, and the \
         panel claiming otherwise is the bug this status exists to fix"
    );
    assert_eq!(
        board.queue.iter().map(|p| p.node_id).collect::<Vec<_>>(),
        vec![queued, from_proposed, from_active],
        "parked rides `queue`, last, whichever half it was parked out of"
    );
    assert!(
        board
            .queue
            .iter()
            .filter(|p| p.node_id != queued)
            .all(|p| p.status == PARKED_STATUS),
        "`status` is how a consumer tells them apart — GP-19 lets it filter, \
         never invent its own notion of dormant"
    );
}

#[tokio::test]
async fn the_board_carries_parked_programs_last() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let running = program_over(&pool, "in flight", vec![]).await;
    let dormant = program_over(&pool, "waiting on hardware", vec![]).await;

    set_program_status(&pool, dormant, PARKED_STATUS).await;

    let board = board_rollup(&pool).await.unwrap();
    let order: Vec<i64> = board
        .programs
        .iter()
        .map(|p| p.program.node_id)
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        vec![running, dormant],
        "Operations and Planning draw one divider, not two"
    );
}

// --- declared, not derived --------------------------------------------------

/// The sharpest form of "korg never overturns a decision somebody made", and
/// #1535's main technical content. `queued` is maintained across three write
/// paths; `parked` is a declaration, and a slice starting underneath must not
/// lift it. No clause was added for this — gating the promotion on `queued`
/// already excludes every other status — so the test asserts the behaviour
/// against the database rather than trusting the comment that says so.
#[tokio::test]
async fn starting_a_slice_never_unparks_its_program() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let slice = proposal(&pool, "slice one").await;
    let later = proposal(&pool, "slice two").await;
    let program = program_over(&pool, "dormant line of work", vec![slice]).await;

    set_program_status(&pool, program, PARKED_STATUS).await;

    // Every path that promotes a `queued` program, in turn.
    set_proposal_status(&pool, slice, "active").await;
    assert_eq!(
        program_status(&pool, program).await,
        PARKED_STATUS,
        "update_proposal must not promote a parked program"
    );

    relate(
        &pool,
        program,
        later,
        "includes",
        None,
        Some(Decimal::new(2, 0)),
    )
    .await
    .unwrap();
    set_proposal_status(&pool, later, "active").await;
    assert_eq!(
        program_status(&pool, program).await,
        PARKED_STATUS,
        "relate + a running slice must not promote a parked program either"
    );
}

/// The other half of the same rule, and the reason `parked` is not in
/// `PROPOSAL_STARTED_STATUSES`: a proposal can be parked before it ever runs,
/// so a program built over one must not be born `active`. That is #1424 — an
/// ACTIVE callout on work nobody has touched — arriving by a new route.
#[tokio::test]
async fn a_program_over_a_never_started_parked_slice_is_still_queued() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let slice = proposal(&pool, "parked before it began").await;
    set_proposal_status(&pool, slice, PARKED_STATUS).await;

    let program = program_over(&pool, "planned, not begun", vec![slice]).await;

    assert_eq!(program_status(&pool, program).await, "queued");
}

/// Un-parking restores what the caller names, and korg contributes nothing to
/// the choice. Both directions are legitimate from the same starting row, which
/// is exactly why korg cannot pick one.
#[tokio::test]
async fn un_parking_goes_where_the_caller_says() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let back_to_queue = proposal(&pool, "resume later").await;
    let back_to_work = proposal(&pool, "resume now").await;
    for p in [back_to_queue, back_to_work] {
        set_proposal_status(&pool, p, "active").await;
        set_proposal_status(&pool, p, PARKED_STATUS).await;
    }

    set_proposal_status(&pool, back_to_queue, "proposed").await;
    set_proposal_status(&pool, back_to_work, "active").await;

    let listed = list_proposals_lean(&pool, Some("proposed"), None, ArchivedFilter::default())
        .await
        .unwrap();
    assert_eq!(
        listed.items.iter().map(|p| p.node_id).collect::<Vec<_>>(),
        vec![back_to_queue]
    );
}

// --- unfinished: parked still blocks, and still waits ------------------------

/// #978's Deconfliction derives *unfinished* from the vocabulary per kind, so
/// this falls out of `parked` being non-terminal rather than from new code —
/// which is precisely why it is worth a test. "Deferred indefinitely" is the
/// most blocking thing a dependency can be, and a reader told otherwise would
/// be told a thing waiting on an RMA had been dealt with.
#[tokio::test]
async fn a_parked_proposal_still_blocks_what_depends_on_it() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let blocker = proposal(&pool, "blocked on the 5090").await;
    let waiting = proposal(&pool, "needs that first").await;
    relate(&pool, waiting, blocker, "depends_on", None, None)
        .await
        .unwrap();

    set_proposal_status(&pool, blocker, PARKED_STATUS).await;

    let board = board_rollup(&pool).await.unwrap();
    let blocked: Vec<i64> = board.blocked.iter().map(|b| b.blocker).collect();
    assert_eq!(
        blocked,
        vec![blocker],
        "deferred is not done — a dependency on a parked proposal is unmet"
    );
    assert_eq!(board.blocked[0].blocker_status, PARKED_STATUS);
}

/// D-7's ghost-free rule clears an awaiting marker when the node reaches a
/// state **only Ken sets**. Parking is not that: it defers the work, it does
/// not answer the question somebody asked. The exclusion lists were hand-written
/// pairs until this sprint made them read `vocab`'s terminal sets — which is
/// what this pins, since a hand-written list is exactly how `parked` got missed
/// one vocabulary over.
#[tokio::test]
async fn parking_does_not_answer_an_awaiting_ask() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let p = proposal(&pool, "needs a call from Ken").await;
    let g = program_over(&pool, "also needs a call", vec![]).await;
    for node_id in [p, g] {
        set_awaiting(&pool, node_id, true, Some("does this still matter?"))
            .await
            .unwrap();
    }

    set_proposal_status(&pool, p, PARKED_STATUS).await;
    set_program_status(&pool, g, PARKED_STATUS).await;

    let lane: Vec<i64> = list_awaiting(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.node_id)
        .collect();
    assert!(
        lane.contains(&p) && lane.contains(&g),
        "a question that outlived the work going dormant is exactly the row \
         the lane exists to show"
    );
}

/// The claim marker (#824) reads the live set, so `parked` joining it changed
/// what "spoken for" means — a decision, not a widen. A parked bundle is a
/// deferred plan, not an abandoned one, so it still claims: reading it as
/// unclaimed would invite a curator to bundle the item a second time.
#[tokio::test]
async fn a_parked_proposal_still_claims_the_work_it_covers() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let item = create_work_item(&pool, new::work_item("covered work"))
        .await
        .unwrap();
    let p = create_proposal(
        &pool,
        NewProposal {
            covers: vec![item.wi_number],
            ..new::proposal("a bundle that went dormant")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    set_proposal_status(&pool, p, PARKED_STATUS).await;

    let row = get_work_item(&pool, item.wi_number).await.unwrap().unwrap();
    assert_eq!(
        row.proposal_node_id,
        Some(p),
        "a deferred plan is still a plan — dropping the marker invites a \
         second proposal over the same item"
    );
}

/// The work-item side is untouched by this sprint and must stay that way: #810's
/// `parked` keeps its own meaning, and widening two more vocabularies must not
/// have moved it. Cheap, and it is the kind of thing a shared literal makes
/// possible to break by accident.
#[tokio::test]
async fn the_work_item_meaning_of_parked_is_unchanged() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let item = create_work_item(&pool, new::work_item("deferred task"))
        .await
        .unwrap();

    update_work_item(
        &pool,
        item.wi_number,
        WorkItemPatch {
            wi_status: Some(PARKED_STATUS.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let row = get_work_item(&pool, item.wi_number).await.unwrap().unwrap();
    assert_eq!(row.wi_status, PARKED_STATUS);
}

/// Search's default scope hides each kind's *terminal* rows so it agrees with
/// the list reads rather than inventing a fourth rule. `parked` is not terminal,
/// so a parked proposal stays findable — which is the behaviour, not an
/// accident of the predicate being written as a complement. Pinned here because
/// `search.rs` spells its per-kind terminal test as literals inside a `const`
/// raw string: it is correct today, and this is what would notice if a later
/// sprint "tidied" it into hiding dormant work.
#[tokio::test]
async fn a_parked_proposal_is_still_searchable_by_default() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let p = create_proposal(
        &pool,
        NewProposal {
            summary: "waiting on the graphics card to come back".into(),
            ..new::proposal("hyperspectral throughput rehearsal")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    set_proposal_status(&pool, p, PARKED_STATUS).await;

    let hits = search(
        &pool,
        SearchQuery {
            q: "hyperspectral throughput rehearsal".into(),
            kind: None,
            project: None,
            scope: None,
            archived: ArchivedFilter::default(),
            page: PageQuery::default(),
        },
    )
    .await
    .unwrap();
    assert!(
        hits.items.iter().any(|h| h.node_id == p),
        "parked is not terminal — dropping it from the default scope would make \
         deferred work unfindable exactly when somebody goes looking for it"
    );
}
