//! Sprint 039 — disposal semantics, fenced.
//!
//! The doctrine these tests pin (`docs/api.md`, *Disposal semantics*):
//! `archived` means "real, but not a target for new work"; a hard delete means
//! "this was never real" and **refuses when the row is referenced**. The
//! refusals are the load-bearing assertions — without them Postgres answers
//! the question instead, and answers it wrongly in both directions: `node`
//! cascades to `relationship` and `comment`, while `workitem.area_id` is
//! `ON DELETE SET NULL`.

use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::repo::{
    add_comment, archived_default, create_area, create_link, create_work_item, delete_area,
    delete_link, get_link, list_areas, list_links, relate, update_area, update_link, LinkPatch,
    LinkQuery, NewWorkItem, PageQuery,
};
use korg_test_support::{fresh_korg, new};
use sqlx::PgPool;

fn code(e: &anyhow::Error) -> ErrorCode {
    e.code()
}

async fn make_project(pool: &PgPool, name: &str) {
    sqlx::query("INSERT INTO project (name, status) VALUES ($1, 'active')")
        .bind(name)
        .execute(pool)
        .await
        .expect("seed project");
}

fn link_query(archived: korg_core::repo::ArchivedFilter) -> LinkQuery {
    LinkQuery {
        disposition: None,
        read: None,
        archived,
        page: PageQuery::default(),
    }
}

// ---------------------------------------------------------------------------
// #888 — links: timestamps, and a reachable lifecycle end
// ---------------------------------------------------------------------------

/// The probe: link rows returned url/title/disposition/read/tags/category and
/// nothing else, so the one kind whose entire point is *when did I capture
/// this* could not say. The columns were never missing — `link` is a node, and
/// `node` has carried `created`/`updated` since 0001. The row just never
/// selected them.
#[tokio::test]
async fn link_rows_carry_capture_timestamps() {
    let (_c, pool) = fresh_korg().await;
    let link = create_link(&pool, new::link("https://example.com/timestamps"))
        .await
        .expect("create link");

    assert_eq!(
        link.created, link.updated,
        "a freshly captured link has not been touched yet"
    );

    let updated = update_link(
        &pool,
        link.node_id,
        LinkPatch {
            read: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("mark read");

    assert_eq!(updated.created, link.created, "created is immutable");
    assert!(
        updated.updated > link.updated,
        "an edit has to advance `updated` — that is the whole recency signal \
         (created {}, updated {})",
        updated.created,
        updated.updated
    );
}

/// `list_links` has documented "archived links are EXCLUDED by default" since
/// #534, while no write on the surface could set the flag — a documented
/// filter with nothing on the other side of it. This is that write, asserted
/// through the filter rather than on the returned row, because the filter is
/// what the claim was about.
#[tokio::test]
async fn archiving_a_link_removes_it_from_the_default_list() {
    let (_c, pool) = fresh_korg().await;
    let link = create_link(&pool, new::link("https://example.com/archive-me"))
        .await
        .expect("create link");

    let live = list_links(&pool, link_query(archived_default()))
        .await
        .expect("list");
    assert_eq!(live.total, 1, "a fresh link is live");

    let archived = update_link(
        &pool,
        link.node_id,
        LinkPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("archive");
    assert!(archived.archived, "the row reports its own state");

    let live = list_links(&pool, link_query(archived_default()))
        .await
        .expect("list");
    assert_eq!(live.total, 0, "the default list now hides it");

    let only_archived = list_links(&pool, link_query(Some(true)))
        .await
        .expect("list archived");
    assert_eq!(only_archived.total, 1, "and the archived view finds it");

    // Reversible: archiving is not a disposal that loses anything.
    update_link(
        &pool,
        link.node_id,
        LinkPatch {
            archived: Some(false),
            ..Default::default()
        },
    )
    .await
    .expect("restore");
    let live = list_links(&pool, link_query(archived_default()))
        .await
        .expect("list");
    assert_eq!(live.total, 1, "and back again");
}

/// The disposal for a capture that was never real — the case probe link 878
/// is, stuck live in production because nothing could remove it.
#[tokio::test]
async fn an_unreferenced_link_can_be_deleted_outright() {
    let (_c, pool) = fresh_korg().await;
    let link = create_link(&pool, new::link("https://example.com/t3-probe"))
        .await
        .expect("create link");

    assert!(
        delete_link(&pool, link.node_id).await.expect("delete"),
        "deleting a live link reports true"
    );
    assert!(
        get_link(&pool, link.node_id).await.expect("get").is_none(),
        "and it is gone"
    );
    assert!(
        !delete_link(&pool, link.node_id).await.expect("re-delete"),
        "deleting it again is false, not an error — same shape as delete_comment"
    );
}

/// The load-bearing half. `relationship.left_id/right_id` and
/// `comment.node_id` both `ON DELETE CASCADE` from `node`, so an
/// unguarded delete would take a link's edges and its whole thread with it —
/// silently, and with no way back.
#[tokio::test]
async fn deleting_a_referenced_link_refuses_instead_of_cascading() {
    let (_c, pool) = fresh_korg().await;

    // Referenced by an edge.
    let edged = create_link(&pool, new::link("https://example.com/edged"))
        .await
        .expect("create link");
    let wi = create_work_item(&pool, new::work_item("something the link informs"))
        .await
        .expect("create wi");
    relate(&pool, edged.node_id, wi.node_id, "related-to", None, None)
        .await
        .expect("relate");

    let err = delete_link(&pool, edged.node_id)
        .await
        .expect_err("a referenced link must refuse");
    assert_eq!(code(&err), ErrorCode::Conflict, "{err}");
    assert!(
        err.to_string().contains("relationship"),
        "the refusal has to name what points at it, or the caller cannot act \
         on it: {err}"
    );
    assert!(
        get_link(&pool, edged.node_id).await.expect("get").is_some(),
        "and a refused delete changes nothing"
    );

    // Referenced by a comment.
    let discussed = create_link(&pool, new::link("https://example.com/discussed"))
        .await
        .expect("create link");
    add_comment(&pool, discussed.node_id, "worth re-reading")
        .await
        .expect("comment");

    let err = delete_link(&pool, discussed.node_id)
        .await
        .expect_err("a commented link must refuse");
    assert_eq!(code(&err), ErrorCode::Conflict, "{err}");
    assert!(err.to_string().contains("comment"), "{err}");
}

/// The type guard: `delete_link` addresses nodes, and a node id is not
/// self-describing. Pointing it at a work item must be `invalid_input`, not a
/// silent cross-kind delete.
#[tokio::test]
async fn delete_link_refuses_a_node_that_is_not_a_link() {
    let (_c, pool) = fresh_korg().await;
    let wi = create_work_item(&pool, new::work_item("not a link"))
        .await
        .expect("create wi");

    let err = delete_link(&pool, wi.node_id)
        .await
        .expect_err("wrong kind must refuse");
    assert_eq!(code(&err), ErrorCode::InvalidInput, "{err}");
    assert!(err.to_string().contains("workitem"), "{err}");
}

// ---------------------------------------------------------------------------
// #889 — areas: a readable description, and a lifecycle at all
// ---------------------------------------------------------------------------

/// `create_area` has accepted a description — and documented idempotent
/// update of it — since 0001, while no read returned it. The contract was
/// unverifiable from the surface that offered it, which is the bug: this test
/// is the verification that was impossible.
#[tokio::test]
async fn area_descriptions_are_readable_and_idempotently_updated() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "korg").await;

    create_area(&pool, "korg", "surface", Some("the MCP tool surface"))
        .await
        .expect("create area");
    let areas = list_areas(&pool, "korg").await.expect("list");
    assert_eq!(
        areas[0].description.as_deref(),
        Some("the MCP tool surface"),
        "the description written is the description read"
    );

    // The documented idempotent re-create, now observable.
    create_area(&pool, "korg", "surface", Some("the agent-facing surface"))
        .await
        .expect("re-create");
    let areas = list_areas(&pool, "korg").await.expect("list");
    assert_eq!(areas.len(), 1, "idempotent — no second row");
    assert_eq!(
        areas[0].description.as_deref(),
        Some("the agent-facing surface"),
        "and the re-create updated it, as create_area has always claimed"
    );
}

/// An area is a label, not a record with a history: `workitem.area_id` points
/// at the row, so a rename moves nothing.
#[tokio::test]
async fn renaming_an_area_keeps_its_work_items() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "korg").await;
    let area_id = create_area(&pool, "korg", "surface", None)
        .await
        .expect("create area");
    create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            area_id: Some(area_id),
            ..new::work_item("filed under surface")
        },
    )
    .await
    .expect("create wi");

    let renamed = update_area(&pool, "korg", "surface", Some("agent-surface"), None)
        .await
        .expect("rename");
    assert_eq!(renamed.id, area_id, "a rename is not a new row");
    assert_eq!(renamed.name, "agent-surface");

    let still_filed: i64 = sqlx::query_scalar("SELECT count(*) FROM workitem WHERE area_id = $1")
        .bind(area_id)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(still_filed, 1, "the work item never moved");

    // And the description is separately clearable.
    let cleared = update_area(&pool, "korg", "agent-surface", None, Some(None))
        .await
        .expect("clear description");
    assert_eq!(cleared.description, None);
}

/// Two areas cannot collide — `UNIQUE (project_id, name)` would otherwise
/// surface as raw Postgres text instead of a `conflict` an agent can branch on.
#[tokio::test]
async fn renaming_onto_an_existing_area_is_a_conflict() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "korg").await;
    create_area(&pool, "korg", "surface", None)
        .await
        .expect("create a");
    create_area(&pool, "korg", "storage", None)
        .await
        .expect("create b");

    let err = update_area(&pool, "korg", "storage", Some("surface"), None)
        .await
        .expect_err("collision must refuse");
    assert_eq!(code(&err), ErrorCode::Conflict, "{err}");
}

/// Areas have no `archived` column and nothing accumulates on them, so delete
/// is their whole lifecycle end. The refusal is what keeps
/// `ON DELETE SET NULL` from quietly unfiling every item under the area.
#[tokio::test]
async fn deleting_an_area_refuses_while_work_items_are_filed_under_it() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "korg").await;
    let area_id = create_area(&pool, "korg", "surface", None)
        .await
        .expect("create area");
    let wi = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            area_id: Some(area_id),
            ..new::work_item("filed under surface")
        },
    )
    .await
    .expect("create wi");

    let err = delete_area(&pool, "korg", "surface")
        .await
        .expect_err("a non-empty area must refuse");
    assert_eq!(code(&err), ErrorCode::Conflict, "{err}");
    assert!(
        err.to_string().contains('1'),
        "the refusal names how many items block it: {err}"
    );

    let filed: Option<i64> = sqlx::query_scalar("SELECT area_id FROM workitem WHERE node_id = $1")
        .bind(wi.node_id)
        .fetch_one(&pool)
        .await
        .expect("read area_id");
    assert_eq!(filed, Some(area_id), "a refused delete unfiled nothing");

    // Move the item off it, and the delete goes through.
    sqlx::query("UPDATE workitem SET area_id = NULL WHERE node_id = $1")
        .bind(wi.node_id)
        .execute(&pool)
        .await
        .expect("unfile");

    assert!(
        delete_area(&pool, "korg", "surface")
            .await
            .expect("delete now-empty area"),
        "an empty area deletes"
    );
    assert!(
        list_areas(&pool, "korg").await.expect("list").is_empty(),
        "and it is gone"
    );
    assert!(
        !delete_area(&pool, "korg", "surface")
            .await
            .expect("re-delete"),
        "deleting it again is false, not an error"
    );
}
