//! Sprint 025 — handoff node core model (proposal korg:614, WI #607).
//!
//! The atomic create contract, the reject-empty / reject-missing guards, the
//! both-sides retrieval, and the load-bearing reconciliation win: a
//! `has_handoff` edge shows up in `get_work_item`'s LB-3 `related` block with
//! the handoff's title, so no handoff-specific read field is needed.

use korg_core::repo::{
    create_handoff, create_proposal, create_work_item, get_handoff, get_node_preview,
    get_work_item_detail, update_handoff, HandoffPatch, NewHandoff, NewWorkItem, RepoError,
};
use korg_test_support::{fresh_korg, new};

fn wi(title: &str) -> NewWorkItem {
    NewWorkItem {
        content: "c".into(),
        ..new::work_item(title)
    }
}

/// Create a handoff attached to a work item and a proposal in one call; both
/// owners come back as `has_handoff` edges, and the body round-trips.
#[tokio::test]
async fn create_links_every_owner_atomically() {
    let (_c, pool) = fresh_korg().await;
    let item = create_work_item(&pool, wi("owning WI")).await.unwrap();
    let prop = create_proposal(&pool, new::proposal("owning sprint"))
        .await
        .unwrap();

    let created = create_handoff(
        &pool,
        NewHandoff {
            title: "Generator output contract".into(),
            summary: "JSON schema + compatibility expectations".into(),
            body: "# State\nfull markdown body".into(),
            related_node_ids: vec![item.node_id, prop.row.node_id],
            ..new::handoff("Generator output contract")
        },
    )
    .await
    .unwrap();

    assert_eq!(created.related_node_ids.len(), 2);

    let full = get_handoff(&pool, created.handoff.node_id)
        .await
        .unwrap()
        .expect("handoff exists");
    assert_eq!(full.body, "# State\nfull markdown body");
    assert!(!full.related_truncated);
    // The handoff sees both owners on the `in` side (owner -> handoff).
    assert_eq!(full.related.len(), 2);
    assert!(full
        .related
        .iter()
        .all(|r| r.label == "has_handoff" && r.direction == "in"));
}

/// The reconciliation win: reading the owning work item alone reveals the
/// handoff through the generic LB-3 block, titled, no extra field.
#[tokio::test]
async fn owner_read_reveals_the_handoff_titled() {
    let (_c, pool) = fresh_korg().await;
    let item = create_work_item(&pool, wi("has a handoff")).await.unwrap();

    create_handoff(
        &pool,
        NewHandoff {
            related_node_ids: vec![item.node_id],
            ..new::handoff("Edge context handoff")
        },
    )
    .await
    .unwrap();

    let detail = get_work_item_detail(&pool, item.wi_number)
        .await
        .unwrap()
        .expect("work item exists");
    let href = detail
        .related
        .iter()
        .find(|r| r.label == "has_handoff")
        .expect("has_handoff edge is inlined on the work item read");
    assert_eq!(href.kind, "handoff");
    assert_eq!(href.title, "Edge context handoff");
    assert_eq!(href.direction, "out"); // WI is the subject: WI -> handoff
    assert!(!detail.related_truncated);
}

/// A handoff with no owners is rejected unless the caller opts in.
#[tokio::test]
async fn empty_related_is_rejected_without_opt_in() {
    let (_c, pool) = fresh_korg().await;

    let err = create_handoff(&pool, new::handoff("orphan"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<RepoError>(),
            Some(RepoError::InvalidInput(_))
        ),
        "empty related_node_ids is invalid input, got {err:?}"
    );

    // The explicit opt-in creates a standalone handoff.
    let created = create_handoff(
        &pool,
        NewHandoff {
            allow_standalone: true,
            ..new::handoff("intentionally standalone")
        },
    )
    .await
    .unwrap();
    assert!(created.related_node_ids.is_empty());
}

/// An owner that doesn't resolve rejects the whole create — no node, no detail
/// row, no edge is left behind.
#[tokio::test]
async fn missing_owner_rejects_and_leaves_no_partial_insert() {
    let (_c, pool) = fresh_korg().await;
    let item = create_work_item(&pool, wi("real owner")).await.unwrap();

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM node WHERE kind = 'handoff'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let err = create_handoff(
        &pool,
        NewHandoff {
            // one good, one that does not exist
            related_node_ids: vec![item.node_id, 999_999],
            ..new::handoff("half-attached")
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<RepoError>(),
            Some(RepoError::NotFound(_))
        ),
        "missing owner is not_found, got {err:?}"
    );

    let after_nodes: i64 = sqlx::query_scalar("SELECT count(*) FROM node WHERE kind = 'handoff'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let edges: i64 =
        sqlx::query_scalar("SELECT count(*) FROM relationship WHERE relationship = 'has_handoff'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(after_nodes, before, "no handoff node was left behind");
    assert_eq!(edges, 0, "no has_handoff edge was left behind");
}

/// Update rewrites the body and archive hides the handoff from default reads.
#[tokio::test]
async fn update_and_archive() {
    let (_c, pool) = fresh_korg().await;
    let item = create_work_item(&pool, wi("owner")).await.unwrap();
    let created = create_handoff(
        &pool,
        NewHandoff {
            body: "v1".into(),
            related_node_ids: vec![item.node_id],
            ..new::handoff("mutable")
        },
    )
    .await
    .unwrap();
    let id = created.handoff.node_id;

    update_handoff(
        &pool,
        id,
        HandoffPatch {
            body: Some("v2".into()),
            title: Some("mutable (revised)".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let full = get_handoff(&pool, id).await.unwrap().unwrap();
    assert_eq!(full.body, "v2");
    assert_eq!(full.row.title, "mutable (revised)");

    update_handoff(
        &pool,
        id,
        HandoffPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let full = get_handoff(&pool, id).await.unwrap().unwrap();
    assert!(full.row.archived);
}

/// Sprint 026 (#610): the generic node preview renders a handoff — title,
/// summary field, Markdown body — so the slide-over IS the viewer.
#[tokio::test]
async fn node_preview_renders_the_handoff() {
    let (_c, pool) = fresh_korg().await;
    let item = create_work_item(&pool, wi("owner")).await.unwrap();
    let created = create_handoff(
        &pool,
        NewHandoff {
            body: "# State\nmarkdown body".into(),
            related_node_ids: vec![item.node_id],
            ..new::handoff("Generator contract")
        },
    )
    .await
    .unwrap();

    let preview = get_node_preview(&pool, created.handoff.node_id)
        .await
        .unwrap()
        .expect("preview exists");
    assert_eq!(preview.kind, "handoff");
    assert_eq!(preview.title, "Generator contract");
    assert_eq!(preview.body.as_deref(), Some("# State\nmarkdown body"));
    assert_eq!(preview.body_label.as_deref(), Some("Handoff"));
    assert!(preview
        .fields
        .iter()
        .any(|f| f.label == "Summary" && f.value == "Generator contract summary"));
}

/// A handoff node whose detail row is somehow absent still previews legibly
/// (plan migration step 5) — the default `handoff #<id>` title, not a blank.
#[tokio::test]
async fn node_preview_missing_detail_is_legible() {
    let (_c, pool) = fresh_korg().await;
    let id: i64 = sqlx::query_scalar("INSERT INTO node (kind) VALUES ('handoff') RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();

    let preview = get_node_preview(&pool, id).await.unwrap().expect("preview");
    assert_eq!(preview.kind, "handoff");
    assert_eq!(preview.title, format!("handoff #{id}"));
    assert!(preview.body.is_none());
}

/// Updating a node that isn't a handoff is a clean not_found, not a silent
/// no-op on the wrong table.
#[tokio::test]
async fn update_rejects_non_handoff_node() {
    let (_c, pool) = fresh_korg().await;
    let item = create_work_item(&pool, wi("not a handoff")).await.unwrap();
    let err = update_handoff(
        &pool,
        item.node_id,
        HandoffPatch {
            body: Some("nope".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<RepoError>(),
            Some(RepoError::NotFound(_))
        ),
        "updating a non-handoff node is not_found, got {err:?}"
    );
}
