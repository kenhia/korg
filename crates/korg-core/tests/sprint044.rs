//! Sprint 044 (#968 + #969, korg:972) — the program layer and the
//! awaiting-Ken marker.
//!
//! 0022 made a proposal single-project and enforced it, which left work that
//! genuinely spans repos with nowhere legal to live. A **program** is that
//! place: it `includes` proposals, ordered, and carries no project of its own.
//!
//! The rules this suite pins, and why each one is a rule rather than a habit:
//!
//! 1. A program takes **no** project (D-6). Not "usually none" — refused, and
//!    backed by a CHECK. Its `span` is derived from its slices, so the fact has
//!    one home and cannot go stale.
//! 2. `includes` is deliberately **cross-project** — it is the layer where
//!    that became legal again.
//! 3. Slice order lives on the **edge**, and re-relating with a rank reorders
//!    in place rather than churning provenance (D-9).
//! 4. The awaiting marker survives an unrelated tag write (the failure that
//!    disqualified the reserved-tag design, D-3), keeps its original timestamp
//!    across a re-set (D-8), and is cleared by the transitions only Ken makes
//!    — but *not* by `resolved`/`done`, which are its best rows (D-7).

use korg_core::repo::{
    create_program, create_project, create_proposal, create_work_item, get_program_detail,
    list_awaiting, list_programs, node_id_for_wi, relate, set_awaiting, update_program,
    update_proposal, update_work_item, ArchivedFilter, NewProgram, NewWorkItem, ProgramPatch,
    ProposalPatch, WorkItemPatch,
};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use rust_decimal::Decimal;
use sqlx::PgPool;

/// A proposal in `project`, returning its node_id.
async fn proposal_in(pool: &PgPool, project: &str, title: &str) -> i64 {
    create_proposal(pool, new::proposal_in(project, title))
        .await
        .unwrap()
        .row
        .node_id
}

/// A work item in `project` with a given status, returning `(wi_number, node_id)`.
async fn wi_in(pool: &PgPool, project: &str, title: &str, status: &str) -> (i64, i64) {
    let wi = create_work_item(
        pool,
        NewWorkItem {
            project: Some(project.into()),
            wi_status: status.into(),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number;
    (wi, node_id_for_wi(pool, wi).await.unwrap().unwrap())
}

// --- 1. a program carries no project (D-6) ----------------------------------

#[tokio::test]
async fn create_program_refuses_a_project_by_name_or_id() {
    let (_c, pool) = fresh_korg().await;
    let pid = test_project(&pool).await;

    for bad in [
        NewProgram {
            project: Some(TEST_PROJECT.into()),
            ..new::program("filed under one repo")
        },
        NewProgram {
            project_id: Some(pid),
            ..new::program("filed under one repo")
        },
    ] {
        let err = create_program(&pool, bad).await.unwrap_err().to_string();
        assert!(
            err.contains("CROSS-project"),
            "the refusal must teach the rule, not just reject: {err}"
        );
        assert!(
            err.contains("span"),
            "and point at the derived answer: {err}"
        );
    }
}

/// The constraint behind the rule. Core is the gate; this is what makes the
/// invariant true of the *database* rather than of whichever code last wrote to
/// it — the same division of labour 0022 §2 used.
#[tokio::test]
async fn the_database_refuses_a_program_with_a_project() {
    let (_c, pool) = fresh_korg().await;
    let pid = test_project(&pool).await;

    let err = sqlx::query("INSERT INTO node (kind, project_id) VALUES ('program', $1)")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("node_program_has_no_project"),
        "expected the CHECK to fire: {err}"
    );
}

/// The 818 case, which is the whole reason the layer exists: work in three
/// projects, one work item each, that had to be filed under a single one.
#[tokio::test]
async fn span_is_derived_from_the_slices_and_spans_projects() {
    let (_c, pool) = fresh_korg().await;
    for p in ["kagent", "klams-mind", "korg"] {
        create_project(&pool, p).await.unwrap();
    }
    let slices = vec![
        proposal_in(&pool, "korg", "korg slice").await,
        proposal_in(&pool, "kagent", "kagent slice").await,
        proposal_in(&pool, "klams-mind", "klams-mind slice").await,
    ];

    let created = create_program(
        &pool,
        NewProgram {
            slices: slices.clone(),
            ..new::program("eval and drill traffic")
        },
    )
    .await
    .unwrap();

    assert_eq!(
        created.row.span,
        vec!["kagent", "klams-mind", "korg"],
        "span is the distinct projects of the slices, alphabetical"
    );
    assert_eq!(created.row.slice_count, 3);
    assert_eq!(created.slices, slices, "the echo preserves the given order");
}

#[tokio::test]
async fn a_program_with_no_slices_has_an_empty_span() {
    let (_c, pool) = fresh_korg().await;
    let created = create_program(&pool, new::program("not started"))
        .await
        .unwrap();
    assert!(created.row.span.is_empty());
    assert_eq!(created.row.slice_count, 0);
}

// --- 2. slices are proposals, and a bad one is refused ----------------------

#[tokio::test]
async fn a_slice_that_is_not_a_proposal_is_refused_not_dropped() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, wi_node) = wi_in(&pool, TEST_PROJECT, "not a proposal", "open").await;

    let err = create_program(
        &pool,
        NewProgram {
            slices: vec![wi_node],
            ..new::program("bad slice")
        },
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("workitem"), "name what it actually is: {err}");

    // And nothing was written — the check runs before the transaction opens.
    let programs = list_programs(&pool, Some("all"), ArchivedFilter::default())
        .await
        .unwrap();
    assert!(
        programs.items.is_empty(),
        "a refused create must leave nothing behind"
    );
}

/// `includes` is the layer where cross-project work is legal — the mirror of
/// sprint 043's `covers_is_the_only_single_project_label`.
#[tokio::test]
async fn includes_may_cross_projects() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "kvllm").await.unwrap();
    test_project(&pool).await;
    let program = create_program(&pool, new::program("weekend sidecar"))
        .await
        .unwrap()
        .row
        .node_id;
    let a = proposal_in(&pool, TEST_PROJECT, "korg half").await;
    let b = proposal_in(&pool, "kvllm", "kvllm half").await;

    relate(&pool, program, a, "includes", None, None)
        .await
        .unwrap();
    relate(&pool, program, b, "includes", None, None)
        .await
        .unwrap();

    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    assert_eq!(detail.slices.len(), 2);
    assert_eq!(detail.program.span, vec![TEST_PROJECT, "kvllm"]);
}

#[tokio::test]
async fn includes_validates_both_endpoints() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let program = create_program(&pool, new::program("p"))
        .await
        .unwrap()
        .row
        .node_id;
    let proposal = proposal_in(&pool, TEST_PROJECT, "slice").await;
    let (_, wi_node) = wi_in(&pool, TEST_PROJECT, "wi", "open").await;

    // Right end must be a proposal.
    let err = relate(&pool, program, wi_node, "includes", None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("sprint_proposal"), "{err}");

    // Left end must be a program.
    let err = relate(&pool, proposal, proposal, "includes", None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(!err.is_empty());
}

// --- 3. slice order lives on the edge (D-2, D-9) ----------------------------

#[tokio::test]
async fn create_program_ranks_slices_in_the_order_given() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let first = proposal_in(&pool, TEST_PROJECT, "aaa last alphabetically? no").await;
    let second = proposal_in(&pool, TEST_PROJECT, "second").await;
    let third = proposal_in(&pool, TEST_PROJECT, "third").await;

    // Deliberately not node_id order: the program's order is the caller's, not
    // the database's.
    let created = create_program(
        &pool,
        NewProgram {
            slices: vec![third, first, second],
            ..new::program("ordered")
        },
    )
    .await
    .unwrap();

    let detail = get_program_detail(&pool, created.row.node_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail.slices.iter().map(|s| s.node_id).collect::<Vec<_>>(),
        vec![third, first, second]
    );
}

/// D-9: the reorder path. Before this, `relate`'s ON CONFLICT was a pure no-op,
/// so moving a slice meant `unrelate` + `relate` — which throws away the edge's
/// `created`/`origin`.
#[tokio::test]
async fn re_relating_with_a_rank_reorders_in_place_and_keeps_provenance() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = proposal_in(&pool, TEST_PROJECT, "a").await;
    let b = proposal_in(&pool, TEST_PROJECT, "b").await;
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![a, b],
            ..new::program("reorder me")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    let rel_id: i64 =
        sqlx::query_scalar("SELECT id FROM relationship WHERE left_id = $1 AND right_id = $2")
            .bind(program)
            .bind(b)
            .fetch_one(&pool)
            .await
            .unwrap();
    let created_before: time::OffsetDateTime =
        sqlx::query_scalar("SELECT created FROM relationship WHERE id = $1")
            .bind(rel_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // Move b to the front.
    let same_edge = relate(
        &pool,
        program,
        b,
        "includes",
        Some("some-other-writer"),
        Some(Decimal::new(-1, 0)),
    )
    .await
    .unwrap();
    assert_eq!(same_edge, rel_id, "reorder must not create a second edge");

    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    assert_eq!(
        detail.slices.iter().map(|s| s.node_id).collect::<Vec<_>>(),
        vec![b, a],
        "b moved to the front"
    );

    let (created_after, origin): (time::OffsetDateTime, Option<String>) =
        sqlx::query_as("SELECT created, origin FROM relationship WHERE id = $1")
            .bind(rel_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        created_before, created_after,
        "provenance survives a reorder"
    );
    assert_eq!(
        origin.as_deref(),
        Some("create_program"),
        "the original writer keeps the credit"
    );
}

/// The other half of D-9: a re-relate with no rank must not wipe the position.
/// This is what stops an unrelated `relate` call silently shuffling a program.
#[tokio::test]
async fn re_relating_without_a_rank_leaves_the_position_alone() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = proposal_in(&pool, TEST_PROJECT, "a").await;
    let b = proposal_in(&pool, TEST_PROJECT, "b").await;
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![a, b],
            ..new::program("stable")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    relate(&pool, program, b, "includes", None, None)
        .await
        .unwrap();

    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    assert_eq!(
        detail.slices.iter().map(|s| s.node_id).collect::<Vec<_>>(),
        vec![a, b],
        "order unchanged"
    );
}

/// An unranked slice sorts last rather than randomly — `ORDER BY rank NULLS
/// LAST, node_id`.
#[tokio::test]
async fn an_unranked_slice_sorts_last() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let ranked = proposal_in(&pool, TEST_PROJECT, "ranked").await;
    let bare = proposal_in(&pool, TEST_PROJECT, "bare").await;
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![ranked],
            ..new::program("mixed")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;
    relate(&pool, program, bare, "includes", None, None)
        .await
        .unwrap();

    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    assert_eq!(
        detail.slices.iter().map(|s| s.node_id).collect::<Vec<_>>(),
        vec![ranked, bare]
    );
    assert!(detail.slices[1].rank.is_none());
}

// --- 4. the rollup: nobody crawls (D-5) -------------------------------------

#[tokio::test]
async fn get_program_rolls_up_work_item_status_per_slice() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let (open_wi, _) = wi_in(&pool, TEST_PROJECT, "open one", "open").await;
    let (resolved_wi, _) = wi_in(&pool, TEST_PROJECT, "resolved one", "resolved").await;
    let (done_wi, _) = wi_in(&pool, TEST_PROJECT, "done one", "done").await;

    let proposal = create_proposal(
        &pool,
        korg_core::repo::NewProposal {
            covers: vec![open_wi, resolved_wi, done_wi],
            ..new::proposal_in(TEST_PROJECT, "a slice with work")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;
    let empty = proposal_in(&pool, TEST_PROJECT, "a slice with none").await;

    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![proposal, empty],
            ..new::program("rollup")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    let first = &detail.slices[0];
    assert_eq!(first.covered_count, 3);
    assert_eq!(
        (first.open, first.resolved, first.done, first.closed),
        (1, 1, 1, 0)
    );
    assert_eq!(first.status, "proposed");
    assert_eq!(first.project.as_deref(), Some(TEST_PROJECT));

    let second = &detail.slices[1];
    assert_eq!(
        second.covered_count, 0,
        "a slice with no work items counts 0"
    );
    assert_eq!((second.open, second.resolved, second.done), (0, 0, 0));
}

/// LB-3: a `has_handoff` edge must surface where the reader already is. A
/// program is precisely the kind that accrues one.
#[tokio::test]
async fn get_program_carries_the_related_block_but_not_its_own_slices() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let slice = proposal_in(&pool, TEST_PROJECT, "slice").await;
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![slice],
            ..new::program("with a handoff")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;
    let handoff = korg_core::repo::create_handoff(
        &pool,
        korg_core::repo::NewHandoff {
            related_node_ids: vec![program],
            ..new::handoff("picking this program up")
        },
    )
    .await
    .unwrap();

    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    assert_eq!(
        detail.related.len(),
        1,
        "includes is excluded — it is `slices`"
    );
    assert_eq!(detail.related[0].label, "has_handoff");
    assert_eq!(detail.related[0].node_id, handoff.handoff.node_id);
    assert_eq!(
        detail.related[0].title, "picking this program up",
        "the title resolves — related_context learned the program kind too"
    );
}

// --- 5. list_programs narrows by default ------------------------------------

#[tokio::test]
async fn list_programs_hides_done_and_counts_what_it_hid() {
    let (_c, pool) = fresh_korg().await;
    let live = create_program(&pool, new::program("in flight"))
        .await
        .unwrap()
        .row
        .node_id;
    let holding = create_program(&pool, new::program("paused"))
        .await
        .unwrap()
        .row
        .node_id;
    let finished = create_program(&pool, new::program("shipped"))
        .await
        .unwrap()
        .row
        .node_id;

    update_program(
        &pool,
        holding,
        ProgramPatch {
            status: Some("holding".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    update_program(
        &pool,
        finished,
        ProgramPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = list_programs(&pool, None, ArchivedFilter::default())
        .await
        .unwrap();
    let ids: Vec<i64> = listed.items.iter().map(|p| p.node_id).collect();
    assert_eq!(
        ids,
        vec![live, holding],
        "holding is live — it is still a program in play"
    );
    assert_eq!(listed.omitted.done, 1);

    let all = list_programs(&pool, Some("all"), ArchivedFilter::default())
        .await
        .unwrap();
    assert_eq!(all.items.len(), 3);
    assert_eq!(all.omitted.done, 0, "a status you asked for is not omitted");
}

// --- 6. the awaiting-Ken marker (#969) --------------------------------------

/// The failure that disqualified the reserved-tag design (D-3). If the marker
/// were a tag, this test would fail — and it would fail *silently* in
/// production, dropping the item out of the lane Ken is meant to be watching.
#[tokio::test]
async fn an_unrelated_tag_write_does_not_clear_the_marker() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (wi, node) = wi_in(&pool, TEST_PROJECT, "needs a call", "open").await;

    set_awaiting(&pool, node, true, Some("pick an approach"))
        .await
        .unwrap();
    update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            tags: Some(vec!["something".into(), "unrelated".into()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let lane = list_awaiting(&pool).await.unwrap();
    assert_eq!(lane.len(), 1, "the marker survives a wholesale tag write");
    assert_eq!(lane[0].awaiting_note.as_deref(), Some("pick an approach"));
}

/// D-8: the age of an ask is the point, so re-asserting must not restart it.
#[tokio::test]
async fn re_marking_keeps_the_original_timestamp_and_updates_the_note() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, node) = wi_in(&pool, TEST_PROJECT, "old ask", "open").await;

    let first = set_awaiting(&pool, node, true, Some("original"))
        .await
        .unwrap();
    let again = set_awaiting(&pool, node, true, Some("sharpened"))
        .await
        .unwrap();

    assert_eq!(
        first.awaiting_since, again.awaiting_since,
        "a nine-day-old ask must not look fresh because an agent touched it"
    );
    assert_eq!(again.awaiting_note.as_deref(), Some("sharpened"));
}

/// D-8: an agent that got its answer in-session retracts its own marker.
#[tokio::test]
async fn an_agent_can_clear_its_own_marker() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, node) = wi_in(&pool, TEST_PROJECT, "answered", "open").await;

    set_awaiting(&pool, node, true, Some("which way?"))
        .await
        .unwrap();
    let cleared = set_awaiting(&pool, node, false, None).await.unwrap();

    assert!(cleared.awaiting_since.is_none());
    assert!(cleared.awaiting_note.is_none());
    assert!(list_awaiting(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_note_without_the_marker_is_refused() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, node) = wi_in(&pool, TEST_PROJECT, "x", "open").await;

    let err = set_awaiting(&pool, node, false, Some("nonsense"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("clearing"), "{err}");
}

/// D-7, and the one that is easy to get backwards. `resolved` and `done` are the
/// canonical awaiting-Ken states — "implemented, needs your user test". Clearing
/// on them would empty the lane of exactly the rows it exists to show.
#[tokio::test]
async fn resolved_and_done_keep_the_marker_but_closed_clears_it() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (wi, node) = wi_in(&pool, TEST_PROJECT, "needs a user test", "open").await;
    set_awaiting(&pool, node, true, Some("does this work for you?"))
        .await
        .unwrap();

    for status in ["resolved", "done"] {
        update_work_item(
            &pool,
            wi,
            WorkItemPatch {
                wi_status: Some(status.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            list_awaiting(&pool).await.unwrap().len(),
            1,
            "{status} is the canonical awaiting-Ken state, not a reason to clear"
        );
    }

    // `closed` is Ken-only (vocab::WI_STATUSES) — reaching it means he acted.
    update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            wi_status: Some("closed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        list_awaiting(&pool).await.unwrap().is_empty(),
        "if Ken closed it, the ask is answered by definition"
    );
    let since: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT awaiting_since FROM node WHERE id = $1")
            .bind(node)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        since.is_none(),
        "cleared in the data, not just filtered in the read"
    );
}

#[tokio::test]
async fn a_decided_proposal_and_a_finished_program_clear_their_markers() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let proposal = proposal_in(&pool, TEST_PROJECT, "waiting on a call").await;
    let program = create_program(&pool, new::program("waiting too"))
        .await
        .unwrap()
        .row
        .node_id;
    set_awaiting(&pool, proposal, true, Some("ship it?"))
        .await
        .unwrap();
    set_awaiting(&pool, program, true, Some("which slice first?"))
        .await
        .unwrap();
    assert_eq!(list_awaiting(&pool).await.unwrap().len(), 2);

    update_proposal(
        &pool,
        proposal,
        ProposalPatch {
            status: Some("declined".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    update_program(
        &pool,
        program,
        ProgramPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(list_awaiting(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn archiving_clears_the_marker() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (wi, node) = wi_in(&pool, TEST_PROJECT, "shelved", "open").await;
    set_awaiting(&pool, node, true, Some("still relevant?"))
        .await
        .unwrap();

    update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(list_awaiting(&pool).await.unwrap().is_empty());
}

/// The lane is oldest-first, carries each node's own status, and spans kinds —
/// everything a board needs to render without a follow-up read per row.
#[tokio::test]
async fn the_lane_is_oldest_first_and_carries_each_kind_status() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, wi_node) = wi_in(&pool, TEST_PROJECT, "the work item", "resolved").await;
    let proposal = proposal_in(&pool, TEST_PROJECT, "the proposal").await;
    let program = create_program(&pool, new::program("the program"))
        .await
        .unwrap()
        .row
        .node_id;

    set_awaiting(&pool, wi_node, true, Some("user test"))
        .await
        .unwrap();
    set_awaiting(&pool, proposal, true, None).await.unwrap();
    set_awaiting(&pool, program, true, Some("sequence?"))
        .await
        .unwrap();

    let lane = list_awaiting(&pool).await.unwrap();
    assert_eq!(lane.len(), 3);
    assert!(
        lane.windows(2)
            .all(|w| w[0].awaiting_since <= w[1].awaiting_since),
        "oldest ask first"
    );

    let wi_row = lane.iter().find(|r| r.kind == "workitem").unwrap();
    assert_eq!(wi_row.status.as_deref(), Some("resolved"));
    assert_eq!(wi_row.title, "the work item");
    assert_eq!(wi_row.project.as_deref(), Some(TEST_PROJECT));
    assert!(wi_row.wi_number.is_some());

    let program_row = lane.iter().find(|r| r.kind == "program").unwrap();
    assert_eq!(program_row.status.as_deref(), Some("active"));
    assert_eq!(program_row.title, "the program");
    assert!(
        program_row.project.is_none(),
        "a program has no project of its own — D-6"
    );

    let proposal_row = lane.iter().find(|r| r.kind == "sprint_proposal").unwrap();
    assert_eq!(proposal_row.status.as_deref(), Some("proposed"));
    assert!(proposal_row.awaiting_note.is_none(), "a note is optional");
}

/// The belt to D-7's braces: even a marker that somehow escaped the write rules
/// must not haunt the lane. Written straight to the table to simulate exactly
/// that — a row an older image, or a hand-edit, left behind.
#[tokio::test]
async fn the_lane_filters_ghosts_the_write_rules_missed() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, closed) = wi_in(&pool, TEST_PROJECT, "already closed", "closed").await;
    let (_, archived) = wi_in(&pool, TEST_PROJECT, "already archived", "open").await;
    sqlx::query("UPDATE node SET archived = true WHERE id = $1")
        .bind(archived)
        .execute(&pool)
        .await
        .unwrap();

    for id in [closed, archived] {
        sqlx::query(
            "UPDATE node SET awaiting_since = now(), awaiting_note = 'ghost' WHERE id = $1",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }

    assert!(
        list_awaiting(&pool).await.unwrap().is_empty(),
        "a lane that accumulates answered asks is the failure D-3 exists to avoid"
    );
}

#[tokio::test]
async fn the_marker_needs_a_real_node() {
    let (_c, pool) = fresh_korg().await;
    let err = set_awaiting(&pool, 999_999, true, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("999999") || err.contains("999_999"), "{err}");
}
