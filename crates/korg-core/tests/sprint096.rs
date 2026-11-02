//! Sprint 096 (WI 2936, korg:4314) — the activity log.
//!
//! What this suite holds, in the order it matters:
//!
//! 1. **A write that changes nothing logs nothing** — 0026's guard, extended to
//!    edits, comment edits and re-related edges. Without it the feed is
//!    `node.updated` again.
//! 2. **A status change is counted once.** It is `transition`'s row, read as
//!    `status_changed`; the edit fingerprint leaves the status out so the same
//!    write is not also an `edited`.
//! 3. **Context is resolved at read time** through `covers` and `includes`, so
//!    a work item covered after the event still shows its proposal.
//! 4. The filters and the envelope.

use korg_core::repo::{
    add_comment, create_handoff, create_program, create_proposal, create_work_item, delete_comment,
    list_activity, node_id_for_wi, relate, unrelate, update_comment, update_proposal,
    update_work_item, ActivityEvent, ActivityFilter, NewHandoff, NewProgram, NewProposal,
    NewWorkItem, ProposalPatch, WorkItemPatch,
};
use korg_core::vocab::ACTIVITY_ACTIONS;
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use sqlx::PgPool;

// --- scaffolding ------------------------------------------------------------

async fn wi(pool: &PgPool, title: &str) -> (i64, i64) {
    let row = create_work_item(
        pool,
        NewWorkItem {
            project: Some(TEST_PROJECT.into()),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap();
    let node = node_id_for_wi(pool, row.wi_number).await.unwrap().unwrap();
    (row.wi_number, node)
}

async fn feed(pool: &PgPool, filter: ActivityFilter) -> Vec<ActivityEvent> {
    list_activity(pool, filter).await.unwrap().items
}

/// The feed for one node, oldest first, as `action` strings.
async fn actions_on(pool: &PgPool, node_id: i64) -> Vec<String> {
    let mut items: Vec<String> = feed(
        pool,
        ActivityFilter {
            limit: Some(500),
            ..Default::default()
        },
    )
    .await
    .into_iter()
    .filter(|e| e.target.node_id == node_id)
    .map(|e| e.action)
    .collect();
    items.reverse();
    items
}

fn patch() -> WorkItemPatch {
    WorkItemPatch::default()
}

// --- 1. the vocabulary is the constraint --------------------------------------

#[tokio::test]
async fn activity_actions_match_the_check_constraint() {
    let (_pg, pool) = fresh_korg().await;
    let def: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'activity_action_known'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let admitted: std::collections::BTreeSet<String> = def
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    let declared: std::collections::BTreeSet<String> = ACTIVITY_ACTIONS
        .iter()
        .filter(|a| **a != "status_changed")
        .map(|a| a.to_string())
        .collect();
    assert_eq!(
        declared, admitted,
        "ACTIVITY_ACTIONS (less `status_changed`, which is transition's) and \
         activity_action_known disagree"
    );
}

// --- 2. created, edited, and the no-op guard ----------------------------------

#[tokio::test]
async fn an_edit_that_changes_nothing_logs_nothing() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (n, node) = wi(&pool, "first title").await;
    assert_eq!(actions_on(&pool, node).await, ["created"]);

    // The same title, the same status: korg's own post-deploy check does this.
    update_work_item(
        &pool,
        n,
        WorkItemPatch {
            title: Some("first title".into()),
            wi_status: Some("open".into()),
            ..patch()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        actions_on(&pool, node).await,
        ["created"],
        "a re-PATCH to stored values must not appear in the feed"
    );

    update_work_item(
        &pool,
        n,
        WorkItemPatch {
            title: Some("second title".into()),
            ..patch()
        },
    )
    .await
    .unwrap();
    assert_eq!(actions_on(&pool, node).await, ["created", "edited"]);
}

#[tokio::test]
async fn a_status_change_is_counted_once() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (n, node) = wi(&pool, "item").await;

    update_work_item(
        &pool,
        n,
        WorkItemPatch {
            wi_status: Some("resolved".into()),
            ..patch()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        actions_on(&pool, node).await,
        ["created", "status_changed"],
        "a status-only write is transition's row and nothing else"
    );

    // Both at once: two events, and the transition sorts after the edit.
    update_work_item(
        &pool,
        n,
        WorkItemPatch {
            wi_status: Some("done".into()),
            content: Some("new body".into()),
            ..patch()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        actions_on(&pool, node).await,
        ["created", "status_changed", "edited", "status_changed"]
    );

    let latest = &feed(&pool, ActivityFilter::default()).await[0];
    assert_eq!(latest.action, "status_changed");
    assert_eq!(latest.from_status.as_deref(), Some("resolved"));
    assert_eq!(latest.to_status.as_deref(), Some("done"));
    assert_eq!(latest.target.wi_number, Some(n));
    assert_eq!(latest.target.title, "item");
    assert_eq!(latest.project.as_deref(), Some(TEST_PROJECT));
}

// --- comments and edges --------------------------------------------------------

#[tokio::test]
async fn comments_are_logged_and_a_deleted_one_keeps_its_event() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, node) = wi(&pool, "item").await;

    let c = add_comment(&pool, node, "hello", Some("test"))
        .await
        .unwrap();
    update_comment(&pool, c.id, "hello", None).await.unwrap();
    assert_eq!(
        actions_on(&pool, node).await,
        ["created", "comment_added"],
        "an edit leaving body and origin as they were is not an event"
    );
    update_comment(&pool, c.id, "hello, edited", None)
        .await
        .unwrap();
    assert!(delete_comment(&pool, c.id).await.unwrap());
    assert_eq!(
        actions_on(&pool, node).await,
        [
            "created",
            "comment_added",
            "comment_edited",
            "comment_deleted"
        ]
    );
    let deleted = &feed(&pool, ActivityFilter::default()).await[0];
    assert_eq!(deleted.comment_id, Some(c.id));
}

#[tokio::test]
async fn an_edge_is_logged_once_and_rerelating_it_is_not_an_event() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, a) = wi(&pool, "a").await;
    let (b_num, b) = wi(&pool, "b").await;

    let edge = relate(&pool, a, b, "depends_on", None, None).await.unwrap();
    relate(&pool, a, b, "depends_on", None, None).await.unwrap();
    assert_eq!(actions_on(&pool, a).await, ["created", "related"]);
    assert_eq!(
        actions_on(&pool, b).await,
        ["created"],
        "logged on the left end only"
    );

    assert!(unrelate(&pool, edge).await.unwrap());
    assert_eq!(
        actions_on(&pool, a).await,
        ["created", "related", "unrelated"]
    );
    let e = &feed(&pool, ActivityFilter::default()).await[0];
    assert_eq!(e.label.as_deref(), Some("depends_on"));
    let other = e.other.as_ref().expect("the other end resolves");
    assert_eq!(other.node_id, b);
    assert_eq!(other.wi_number, Some(b_num));
    assert_eq!(other.title, "b");
}

#[tokio::test]
async fn a_handoff_lands_in_its_owners_feed() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, owner) = wi(&pool, "owner").await;
    let h = create_handoff(
        &pool,
        NewHandoff {
            related_node_ids: vec![owner],
            ..new::handoff("the handoff")
        },
    )
    .await
    .unwrap();
    assert_eq!(actions_on(&pool, owner).await, ["created", "related"]);
    assert_eq!(actions_on(&pool, h.handoff.node_id).await, ["created"]);
}

// --- 3. context is resolved at read time ------------------------------------

#[tokio::test]
async fn an_event_carries_its_proposal_and_program_resolved_now() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, node) = wi(&pool, "item").await;
    // Commented on BEFORE it was covered: context comes from the edges as they
    // are now, not as they were when the event was written.
    add_comment(&pool, node, "early", None).await.unwrap();

    let proposal = create_proposal(
        &pool,
        NewProposal {
            covers: vec![node],
            ..new::proposal("the proposal")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![proposal],
            ..new::program("the program")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    let events = feed(&pool, ActivityFilter::default()).await;
    let comment = events.iter().find(|e| e.action == "comment_added").unwrap();
    let p = comment.proposal.as_ref().expect("covering proposal");
    assert_eq!((p.node_id, p.title.as_str()), (proposal, "the proposal"));
    assert_eq!(p.url.as_deref(), Some(&*format!("/planning/{proposal}")));
    let g = comment.program.as_ref().expect("including program");
    assert_eq!((g.node_id, g.title.as_str()), (program, "the program"));

    // A proposal's own event names its program and not itself as a proposal.
    update_proposal(
        &pool,
        proposal,
        ProposalPatch {
            title: Some("renamed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let e = &feed(&pool, ActivityFilter::default()).await[0];
    assert_eq!(e.action, "edited");
    assert_eq!(e.target.node_id, proposal);
    assert!(e.proposal.is_none());
    assert_eq!(e.program.as_ref().map(|g| g.node_id), Some(program));
}

// --- 4. filters and the envelope ----------------------------------------------

#[tokio::test]
async fn the_feed_filters_by_action_kind_project_and_window() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let (_, node) = wi(&pool, "item").await;
    add_comment(&pool, node, "c", None).await.unwrap();
    let proposal = create_proposal(&pool, new::proposal("p"))
        .await
        .unwrap()
        .row
        .node_id;

    let created = list_activity(
        &pool,
        ActivityFilter {
            action: Some("created".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(created.total, 2);
    assert!(created.items.iter().all(|e| e.action == "created"));

    let wi_created = feed(
        &pool,
        ActivityFilter {
            action: Some("created".into()),
            kind: Some("workitem".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        wi_created.len(),
        1,
        "Ken's example: only work items created"
    );
    assert_eq!(wi_created[0].target.node_id, node);

    let proposals = feed(
        &pool,
        ActivityFilter {
            kind: Some("sprint_proposal".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].target.node_id, proposal);

    let elsewhere = list_activity(
        &pool,
        ActivityFilter {
            project: Some("no-such-project".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(elsewhere.total, 0);

    let now: time::OffsetDateTime = sqlx::query_scalar("SELECT now()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let future = list_activity(
        &pool,
        ActivityFilter {
            since: Some(now + time::Duration::hours(1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(future.total, 0);
    let past = list_activity(
        &pool,
        ActivityFilter {
            until: Some(now + time::Duration::hours(1)),
            limit: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        past.total, 3,
        "`total` is the filtered corpus, not the page"
    );
    assert_eq!(past.items.len(), 1);
}

#[tokio::test]
async fn a_bad_filter_is_invalid_input() {
    let (_pg, pool) = fresh_korg().await;
    for filter in [
        ActivityFilter {
            action: Some("exploded".into()),
            ..Default::default()
        },
        ActivityFilter {
            kind: Some("project".into()),
            ..Default::default()
        },
        ActivityFilter {
            limit: Some(0),
            ..Default::default()
        },
    ] {
        let err = list_activity(&pool, filter).await.unwrap_err();
        assert!(
            err.downcast_ref::<korg_core::repo::RepoError>().is_some(),
            "{err}"
        );
    }
}

/// The same guarantee for the other two kinds `transition` covers: their
/// update paths touch side columns on a status move (promotion, settling the
/// awaiting marker), and none of that may read as an edit.
#[tokio::test]
async fn proposal_and_program_status_moves_are_not_edits() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let proposal = create_proposal(&pool, new::proposal("p"))
        .await
        .unwrap()
        .row
        .node_id;
    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![proposal],
            ..new::program("g")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;
    for status in ["active", "done"] {
        update_proposal(
            &pool,
            proposal,
            ProposalPatch {
                status: Some(status.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(
        actions_on(&pool, proposal).await,
        ["created", "status_changed", "status_changed"]
    );
    korg_core::repo::update_program(
        &pool,
        program,
        korg_core::repo::ProgramPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let on_program = actions_on(&pool, program).await;
    assert!(
        !on_program.contains(&"edited".to_string()),
        "a program status move is not an edit: {on_program:?}"
    );
}
