//! Sprint 053 (#977 + #978, korg:1076) — the board's two deferred panels.
//!
//! Sprint 045 shipped the reports half of the board rollup and named these two
//! as its D-7 and D-8. Both change one contract (`get_board`), which is why they
//! ship together: kfdc absorbs one shape change instead of two.
//!
//! They share a principle, and it is the thing this suite exists to hold:
//! **on a surface whose promise is deterministic rendering (kfdc standing
//! decision #1, no LLM in the render path), a plausible-looking wrong answer is
//! worse than an absent panel.** #977 exists precisely because `node.updated`
//! would date a proposal by its last tag edit and call that a ship date.
//!
//! What each half must not do, therefore:
//!
//! 1. **The ticker must not report writes as changes.** A status re-set to the
//!    value it already holds is not an event. korg's own post-deploy check does
//!    exactly that on every deploy, so this is a live path, not a hypothetical.
//! 2. **Deconfliction must not report satisfied dependencies as blockers.** The
//!    trap here is a real one with two names: `WI_TERMINAL_STATUSES` is a
//!    *list-visibility* split holding `closed` alone, and reading it as the
//!    completion split turns every `done` dependency into a phantom blocker.

use korg_core::repo::{
    board_rollup, create_program, create_project, create_proposal, create_work_item, get_work_item,
    list_transitions, node_id_for_wi, relate, update_program, update_proposal, update_work_item,
    NewProgram, NewProposal, NewWorkItem, ProgramPatch, ProposalPatch, WorkItemPatch,
    BOARD_EVENT_CAP,
};
use korg_core::vocab::{WI_FINISHED_STATUSES, WI_TERMINAL_STATUSES};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use rust_decimal::Decimal;
use sqlx::PgPool;

// --- scaffolding ------------------------------------------------------------

async fn wi(pool: &PgPool, project: &str, title: &str, status: &str) -> i64 {
    create_work_item(
        pool,
        NewWorkItem {
            project: Some(project.into()),
            wi_status: status.into(),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number
}

async fn node_of(pool: &PgPool, wi_number: i64) -> i64 {
    node_id_for_wi(pool, wi_number).await.unwrap().unwrap()
}

async fn proposal(pool: &PgPool, project: &str, title: &str, covers: Vec<i64>) -> i64 {
    create_proposal(
        pool,
        NewProposal {
            summary: format!("{title} summary"),
            rank: Decimal::ONE,
            covers,
            ..new::proposal_in(project, title)
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

// ===========================================================================
// #977 — the transition log
// ===========================================================================

/// The feed's reason for existing: a real transition, dated by when it
/// *happened*, with both ends of the change named.
#[tokio::test]
async fn a_status_change_lands_in_the_log_with_both_ends_named() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, TEST_PROJECT, "the work", "open").await;
    set_wi_status(&pool, n, "resolved").await;
    set_wi_status(&pool, n, "done").await;

    let events = list_transitions(&pool, BOARD_EVENT_CAP).await.unwrap();
    assert_eq!(events.len(), 2, "two changes, two events");
    // Newest first.
    assert_eq!(
        (events[0].from_status.as_str(), events[0].to_status.as_str()),
        ("resolved", "done")
    );
    assert_eq!(
        (events[1].from_status.as_str(), events[1].to_status.as_str()),
        ("open", "resolved")
    );
    assert_eq!(events[0].kind, "workitem");
    assert_eq!(events[0].wi_number, Some(n));
    assert_eq!(events[0].title, "the work");
    assert_eq!(events[0].project.as_deref(), Some(TEST_PROJECT));
}

/// **The rule the whole feed rests on.** `node.updated` cannot serve a "recently
/// shipped" panel because it advances on any edit; a log that recorded status
/// *writes* rather than status *changes* would rebuild that lie one table over.
///
/// This is not hypothetical: `scripts/post-deploy-check.sh` re-PATCHes a status
/// to the value it already holds on every single deploy, and agents re-set
/// statuses routinely.
#[tokio::test]
async fn re_writing_the_same_status_is_not_an_event() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, TEST_PROJECT, "untouched", "open").await;
    let p = proposal(&pool, TEST_PROJECT, "untouched proposal", vec![n]).await;

    // The idempotent write, on both kinds, twice each.
    set_wi_status(&pool, n, "open").await;
    set_wi_status(&pool, n, "open").await;
    set_proposal_status(&pool, p, "proposed").await;
    set_proposal_status(&pool, p, "proposed").await;

    assert!(
        list_transitions(&pool, BOARD_EVENT_CAP)
            .await
            .unwrap()
            .is_empty(),
        "a write that did not change the status must produce no event — this is \
         exactly what node.updated gets wrong, and the reason the log exists"
    );
}

/// Edits that are not status edits move `node.updated` and must move nothing
/// else. This is the tag-edit case #977 names by hand: a proposal touched for
/// its tags dating as newly shipped is the failure the panel was deferred over.
#[tokio::test]
async fn a_non_status_edit_produces_no_event() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let p = proposal(&pool, TEST_PROJECT, "retagged", Vec::new()).await;
    update_proposal(
        &pool,
        p,
        ProposalPatch {
            tags: Some(vec!["kfdc".into(), "board".into()]),
            title: Some("retagged, and retitled".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(
        list_transitions(&pool, BOARD_EVENT_CAP)
            .await
            .unwrap()
            .is_empty(),
        "a tag or title edit is not a transition"
    );
}

/// All three hooked kinds, and the ordering contract. `at` alone cannot order
/// transitions committed in the same microsecond, so the id breaks the tie —
/// a ticker that reshuffles between refreshes is one nobody reads twice.
#[tokio::test]
async fn the_ticker_covers_work_items_proposals_and_programs_newest_first() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, TEST_PROJECT, "task", "open").await;
    let p = proposal(&pool, TEST_PROJECT, "bundle", vec![n]).await;
    let g = create_program(
        &pool,
        NewProgram {
            slices: vec![p],
            ..new::program("the programme")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    set_wi_status(&pool, n, "done").await;
    set_proposal_status(&pool, p, "active").await;
    update_program(
        &pool,
        g,
        ProgramPatch {
            status: Some("holding".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let events = list_transitions(&pool, BOARD_EVENT_CAP).await.unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    // Two program events, not one, since sprint 069 (#1424): starting the slice
    // promoted the program out of `queued`, and that promotion is a status
    // change like any other — it belongs in the ticker exactly as a hand-made
    // one does. It shares a transaction (and therefore an `at`) with the
    // proposal transition that caused it, so this doubles as the tie-break
    // assertion the doc comment above is about: the id orders them, and the
    // effect sorts above its cause.
    assert_eq!(
        kinds,
        vec!["program", "program", "sprint_proposal", "workitem"],
        "newest first, and all three update paths write the log"
    );
    assert_eq!(events[0].title, "the programme");
    assert_eq!(
        events[0].project, None,
        "a program carries no project by construction (#968 D-6)"
    );
    assert_eq!(
        events[1].title, "the programme",
        "the promotion #1424 added — same program, one event older"
    );
    assert_eq!(events[2].title, "bundle");
    assert!(
        events.windows(2).all(|w| w[0].at >= w[1].at),
        "the feed is monotonically newest-first"
    );
}

/// An archived node is out of every other panel; its history must not walk back
/// in through the ticker.
#[tokio::test]
async fn an_archived_nodes_transitions_leave_the_ticker() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, TEST_PROJECT, "abandoned", "open").await;
    set_wi_status(&pool, n, "resolved").await;
    assert_eq!(
        list_transitions(&pool, BOARD_EVENT_CAP)
            .await
            .unwrap()
            .len(),
        1
    );

    update_work_item(
        &pool,
        n,
        WorkItemPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(
        list_transitions(&pool, BOARD_EVENT_CAP)
            .await
            .unwrap()
            .is_empty(),
        "archiving takes the node out of every board panel, this one included"
    );
}

/// The cap is policy, not a suggestion, and the board applies it.
#[tokio::test]
async fn the_ticker_is_capped_at_the_declared_number() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    // Two transitions per item; comfortably over the cap.
    let last = BOARD_EVENT_CAP + 3;
    for i in 0..=last {
        let n = wi(&pool, TEST_PROJECT, &format!("item {i}"), "open").await;
        set_wi_status(&pool, n, "resolved").await;
        set_wi_status(&pool, n, "done").await;
    }

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.events.len() as i64, BOARD_EVENT_CAP);
    assert_eq!(
        board.events[0].title,
        format!("item {last}"),
        "the cap keeps the NEWEST events, not the first ones filed"
    );
    assert!(
        !board.events.iter().any(|e| e.title == "item 0"),
        "and the oldest ones fall off the end"
    );
}

/// A rolled-back update must not leave an event behind. The write is in the
/// same transaction as the status change for this reason — an invalid patch
/// that fails after the status write would otherwise announce a change that
/// never happened.
#[tokio::test]
async fn a_failed_update_records_no_transition() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, TEST_PROJECT, "target", "open").await;
    // `area` resolves inside the transaction, after the status write — so this
    // patch changes the status and then fails.
    let err = update_work_item(
        &pool,
        n,
        WorkItemPatch {
            wi_status: Some("done".into()),
            area: Some(Some("no such area".into())),
            ..Default::default()
        },
    )
    .await;
    assert!(err.is_err(), "the area must not resolve");

    assert_eq!(
        get_work_item(&pool, n).await.unwrap().unwrap().wi_status,
        "open",
        "the status write rolled back"
    );
    assert!(
        list_transitions(&pool, BOARD_EVENT_CAP)
            .await
            .unwrap()
            .is_empty(),
        "and so did the event — a feed that outlives its own transaction is a \
         feed that reports changes that never happened"
    );
}

/// The board starts empty and fills forward. 0026 deliberately did not backfill
/// — there was no honest history to backfill *from* — so a renderer must be able
/// to tell "nothing has moved since the migration" from "nothing ever moved",
/// and this is the state it has to handle.
#[tokio::test]
async fn a_fresh_corpus_has_an_empty_ticker_rather_than_a_fabricated_one() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let n = wi(&pool, TEST_PROJECT, "created, never moved", "open").await;
    proposal(&pool, TEST_PROJECT, "created, never moved either", vec![n]).await;

    let board = board_rollup(&pool).await.unwrap();
    assert!(
        board.events.is_empty(),
        "creating a node is not a transition: `node.created` already dates it \
         honestly, and the log exists only for the events that had no timestamp"
    );
    assert_eq!(board.queue.len(), 1, "the queue itself is unaffected");
}

// ===========================================================================
// #978 — deterministic deconfliction
// ===========================================================================

/// Granularity, half one: the proposal itself carries the edge.
#[tokio::test]
async fn a_proposal_depending_on_an_unfinished_proposal_is_blocked() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let upstream = proposal(&pool, TEST_PROJECT, "must land first", Vec::new()).await;
    let downstream = proposal(&pool, TEST_PROJECT, "waits", Vec::new()).await;
    relate(&pool, downstream, upstream, "depends_on", None, None)
        .await
        .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.blocked.len(), 1);
    let b = &board.blocked[0];
    assert_eq!(b.proposal, downstream);
    assert_eq!(b.via, "proposal");
    assert_eq!(b.dependent, downstream);
    assert_eq!(b.dependent_wi_number, None);
    assert_eq!(b.blocker, upstream);
    assert_eq!(b.blocker_kind, "sprint_proposal");
    assert_eq!(b.blocker_title, "must land first");
    assert_eq!(b.blocker_status, "proposed");
    assert_eq!(
        b.sequenced_by, None,
        "no program orders these, so nothing is showing it twice"
    );
}

/// Granularity, half two: a covered work item carries the edge. Both count
/// (D-3), and `via` is what keeps them tellable apart — the first is a
/// sequencing decision about the sprint, the second is one task inside it
/// waiting on something outside.
#[tokio::test]
async fn a_covered_items_dependency_blocks_its_proposal_and_says_so() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    create_project(&pool, "elsewhere").await.unwrap();

    let mine = wi(&pool, TEST_PROJECT, "my task", "open").await;
    let theirs = wi(&pool, "elsewhere", "their task", "open").await;
    relate(
        &pool,
        node_of(&pool, mine).await,
        node_of(&pool, theirs).await,
        "depends_on",
        None,
        None,
    )
    .await
    .unwrap();
    let p = proposal(&pool, TEST_PROJECT, "the bundle", vec![mine]).await;

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.blocked.len(), 1);
    let b = &board.blocked[0];
    assert_eq!(b.proposal, p);
    assert_eq!(b.via, "covered");
    assert_eq!(b.dependent_wi_number, Some(mine));
    assert_eq!(b.blocker_wi_number, Some(theirs));
    assert_eq!(b.blocker_kind, "workitem");
    assert_eq!(
        b.blocker_project.as_deref(),
        Some("elsewhere"),
        "depends_on is deliberately cross-project — the homelab-ai plan is built \
         out of dependencies between repos"
    );
}

/// **The trap, made executable.** `done` means the agent is satisfied; a
/// dependency on it is met. `WI_TERMINAL_STATUSES` holds `closed` alone because
/// it answers a different question — which rows a lean list hides — and a reader
/// that used it here would report every `done` dependency as an unmet blocker on
/// a panel whose whole promise is that it is right.
#[tokio::test]
async fn a_done_dependency_is_satisfied_even_though_lists_still_show_it() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    assert!(
        WI_FINISHED_STATUSES.contains(&"done") && !WI_TERMINAL_STATUSES.contains(&"done"),
        "the premise: `done` is finished work that a lean list still shows"
    );

    let upstream = wi(&pool, TEST_PROJECT, "already done", "done").await;
    let downstream = wi(&pool, TEST_PROJECT, "the dependent", "open").await;
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
    proposal(&pool, TEST_PROJECT, "bundle", vec![downstream]).await;

    assert!(
        board_rollup(&pool).await.unwrap().blocked.is_empty(),
        "a `done` dependency does not block — reading WI_TERMINAL_STATUSES here \
         instead of WI_FINISHED_STATUSES is what makes this fail"
    );
}

/// `resolved` is the other end of the same judgement and lands the other way:
/// "implemented; may still need a user test / may not be PR'd" is not landed,
/// and a queue row told it was unblocked by unlanded work is told to build on
/// sand.
#[tokio::test]
async fn a_resolved_dependency_still_blocks() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let upstream = wi(&pool, TEST_PROJECT, "implemented, not landed", "resolved").await;
    let downstream = wi(&pool, TEST_PROJECT, "the dependent", "open").await;
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
    proposal(&pool, TEST_PROJECT, "bundle", vec![downstream]).await;

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.blocked.len(), 1);
    assert_eq!(board.blocked[0].blocker_status, "resolved");
}

/// Finishing the blocker clears the row, with no other write. This is the
/// property that makes the panel worth rendering at all: it tracks the work.
#[tokio::test]
async fn finishing_the_blocker_unblocks_the_row() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let upstream = proposal(&pool, TEST_PROJECT, "must land first", Vec::new()).await;
    let downstream = proposal(&pool, TEST_PROJECT, "waits", Vec::new()).await;
    relate(&pool, downstream, upstream, "depends_on", None, None)
        .await
        .unwrap();
    assert_eq!(board_rollup(&pool).await.unwrap().blocked.len(), 1);

    set_proposal_status(&pool, upstream, "done").await;
    assert!(
        board_rollup(&pool).await.unwrap().blocked.is_empty(),
        "a terminal proposal is not something anyone is still waiting on"
    );
}

/// Two exclusions that are decisions rather than oversights, pinned together
/// because they share a rationale: a row held blocked forever by something
/// nobody will ever do is worse than a row that quietly frees up.
#[tokio::test]
async fn an_archived_blocker_and_a_finished_dependent_both_drop_out() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    // (a) the blocker is archived.
    let withdrawn = wi(&pool, TEST_PROJECT, "withdrawn", "open").await;
    let waiting = wi(&pool, TEST_PROJECT, "waiting on it", "open").await;
    relate(
        &pool,
        node_of(&pool, waiting).await,
        node_of(&pool, withdrawn).await,
        "depends_on",
        None,
        None,
    )
    .await
    .unwrap();
    let p = proposal(&pool, TEST_PROJECT, "bundle a", vec![waiting]).await;
    assert_eq!(board_rollup(&pool).await.unwrap().blocked.len(), 1);
    update_work_item(
        &pool,
        withdrawn,
        WorkItemPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        board_rollup(&pool).await.unwrap().blocked.is_empty(),
        "an archived blocker is out of every other view and blocks nothing"
    );

    // (b) the dependent finished anyway.
    update_work_item(
        &pool,
        withdrawn,
        WorkItemPatch {
            archived: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(board_rollup(&pool).await.unwrap().blocked.len(), 1);
    set_wi_status(&pool, waiting, "done").await;
    assert!(
        board_rollup(&pool).await.unwrap().blocked.is_empty(),
        "the covered item finished without its dependency — the proposal is not \
         waiting on it, whatever the edge still says"
    );
    let _ = p;
}

/// korg cannot say whether a link is "finished", so a `depends_on` pointing at
/// one is not a blocker. Inventing an answer is the failure this panel exists
/// to avoid.
#[tokio::test]
async fn a_blocker_with_no_lifecycle_status_does_not_block() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let link = korg_core::repo::create_link(
        &pool,
        korg_core::repo::NewLink {
            project: Some(TEST_PROJECT.into()),
            ..new::link("https://example.invalid/spec")
        },
    )
    .await
    .unwrap()
    .node_id;
    let p = proposal(&pool, TEST_PROJECT, "reads the spec first", Vec::new()).await;
    relate(&pool, p, link, "depends_on", None, None)
        .await
        .unwrap();

    assert!(
        board_rollup(&pool).await.unwrap().blocked.is_empty(),
        "a reading-list link has no lifecycle korg tracks"
    );
}

/// One hop, not a closure (D-4). A→B→C surfaces as two rows, so the chain is
/// visible without korg computing a transitive closure nothing asked for.
#[tokio::test]
async fn blocking_is_one_hop_and_the_chain_shows_as_its_links() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let a = proposal(&pool, TEST_PROJECT, "a", Vec::new()).await;
    let b = proposal(&pool, TEST_PROJECT, "b", Vec::new()).await;
    let c = proposal(&pool, TEST_PROJECT, "c", Vec::new()).await;
    relate(&pool, a, b, "depends_on", None, None).await.unwrap();
    relate(&pool, b, c, "depends_on", None, None).await.unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.blocked.len(), 2, "two edges, two rows — no closure");
    assert!(
        board
            .blocked
            .iter()
            .any(|x| x.proposal == a && x.blocker == b),
        "a is blocked by b"
    );
    assert!(
        board
            .blocked
            .iter()
            .any(|x| x.proposal == b && x.blocker == c),
        "b is blocked by c"
    );
    assert!(
        !board
            .blocked
            .iter()
            .any(|x| x.proposal == a && x.blocker == c),
        "and a is NOT reported as blocked by c — that is a different query and a \
         different UI"
    );
}

/// kfdc #1070's answer (D-5). Ken's objection was that showing Deconfliction for
/// program-ordered work "is essentially showing the same data twice, in
/// Operations and Deconfliction" — and the fix cannot live in kfdc, because kfdc
/// cannot filter what korg did not label. So korg labels it.
#[tokio::test]
async fn a_dependency_a_program_already_orders_names_that_program() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let first = proposal(&pool, TEST_PROJECT, "slice one", Vec::new()).await;
    let second = proposal(&pool, TEST_PROJECT, "slice two", Vec::new()).await;
    let outside = proposal(&pool, TEST_PROJECT, "unsequenced", Vec::new()).await;
    relate(&pool, second, first, "depends_on", None, None)
        .await
        .unwrap();
    relate(&pool, second, outside, "depends_on", None, None)
        .await
        .unwrap();

    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![first, second],
            ..new::program("the sequence")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    let board = board_rollup(&pool).await.unwrap();
    let sequenced = board
        .blocked
        .iter()
        .find(|b| b.blocker == first)
        .expect("slice two is blocked by slice one");
    assert_eq!(
        sequenced.sequenced_by,
        Some(program),
        "both ends are slices of one live program — Operations already draws \
         this, and korg says so rather than dropping the fact"
    );

    let unsequenced = board
        .blocked
        .iter()
        .find(|b| b.blocker == outside)
        .expect("slice two is also blocked by something outside the program");
    assert_eq!(
        unsequenced.sequenced_by, None,
        "a dependency the program does not order is a real Deconfliction card"
    );
}

/// A blocker reached through a covered work item is sequenced by the program
/// that orders the *proposals*, not the work items — the `covers` edge is what
/// connects the two, and without following it every cross-slice task dependency
/// would read as an unsequenced collision.
#[tokio::test]
async fn sequencing_follows_a_covered_items_owning_proposal() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let early = wi(&pool, TEST_PROJECT, "early task", "open").await;
    let late = wi(&pool, TEST_PROJECT, "late task", "open").await;
    relate(
        &pool,
        node_of(&pool, late).await,
        node_of(&pool, early).await,
        "depends_on",
        None,
        None,
    )
    .await
    .unwrap();

    let first = proposal(&pool, TEST_PROJECT, "slice one", vec![early]).await;
    let second = proposal(&pool, TEST_PROJECT, "slice two", vec![late]).await;
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![first, second],
            ..new::program("the sequence")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.blocked.len(), 1);
    assert_eq!(board.blocked[0].via, "covered");
    assert_eq!(board.blocked[0].proposal, second);
    assert_eq!(
        board.blocked[0].sequenced_by,
        Some(program),
        "the task dependency runs between two slices of one program, so it is \
         sequence rather than collision"
    );
}

/// Only *live* rows get blockers. A proposal fetched because it is a slice of a
/// program is not a queue row (sprint 045's rule), and it must not smuggle its
/// dependencies onto the panel either.
#[tokio::test]
async fn a_slice_only_row_contributes_no_blockers() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let shipped = proposal(&pool, TEST_PROJECT, "already shipped", Vec::new()).await;
    let upstream = proposal(&pool, TEST_PROJECT, "still open", Vec::new()).await;
    relate(&pool, shipped, upstream, "depends_on", None, None)
        .await
        .unwrap();
    create_program(
        &pool,
        NewProgram {
            slices: vec![shipped],
            ..new::program("holds a finished slice")
        },
    )
    .await
    .unwrap();

    assert_eq!(board_rollup(&pool).await.unwrap().blocked.len(), 1);
    set_proposal_status(&pool, shipped, "done").await;

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(
        board.programs[0].slices.len(),
        1,
        "Operations still renders the finished slice"
    );
    assert!(
        board.blocked.is_empty(),
        "but a done proposal is not waiting on anything, and was only fetched \
         for the program"
    );
}

/// `blocked` and `proposal_edges` are different lists answering different
/// questions, and neither is a view of the other. The board carries both.
#[tokio::test]
async fn blocked_is_not_a_view_of_proposal_edges() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let a = proposal(&pool, TEST_PROJECT, "a", Vec::new()).await;
    let b = proposal(&pool, TEST_PROJECT, "b", Vec::new()).await;
    // An edge between two live rows that is NOT a dependency.
    relate(&pool, a, b, "collides-with", Some("kfdc-curator"), None)
        .await
        .unwrap();
    // A dependency on a work item, which is not a proposal edge at all.
    let outside = wi(&pool, TEST_PROJECT, "a task", "open").await;
    relate(
        &pool,
        a,
        node_of(&pool, outside).await,
        "depends_on",
        None,
        None,
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(
        board.proposal_edges.len(),
        1,
        "only the proposal-to-proposal edge is a proposal edge"
    );
    assert_eq!(board.proposal_edges[0].label, "collides-with");
    assert_eq!(board.blocked.len(), 1, "only the depends_on is a blocker");
    assert_eq!(board.blocked[0].blocker_kind, "workitem");
}

/// Ordering is stable, for the reason F-19 gave about equal ranks: a card that
/// moves between refreshes is one nobody trusts.
#[tokio::test]
async fn the_blocked_list_is_ordered_deterministically() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let target = proposal(&pool, TEST_PROJECT, "waits on several", Vec::new()).await;
    for i in 0..4 {
        let up = proposal(&pool, TEST_PROJECT, &format!("upstream {i}"), Vec::new()).await;
        relate(&pool, target, up, "depends_on", None, None)
            .await
            .unwrap();
    }

    let once = board_rollup(&pool).await.unwrap().blocked;
    let twice = board_rollup(&pool).await.unwrap().blocked;
    assert_eq!(once.len(), 4);
    let keys = |v: &[korg_core::repo::BoardBlocker]| -> Vec<(i64, i64)> {
        v.iter().map(|b| (b.proposal, b.blocker)).collect()
    };
    assert_eq!(keys(&once), keys(&twice), "two reads, one order");
    let mut sorted = keys(&once);
    sorted.sort();
    assert_eq!(keys(&once), sorted, "and that order is the declared one");
}
