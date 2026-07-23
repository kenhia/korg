//! Name-or-id selectors on the write path (WI #575).
//!
//! Every write that targets a project used to take a bare `project_id`, so an
//! agent without the id had to guess — and a wrong guess was a *silent wrong
//! write*. The incident that produced this sprint: a work item filed with
//! `project_id: 1` landed in an archived project and reported success.
//!
//! These tests fence the three rules that make names safe: resolve-or-fail,
//! never-both, and never-create. The last one matters most — WI #537 removed
//! project-name acceptance from `update_card` precisely because it *created*
//! the project as a side effect, and nothing about this change may bring that
//! back.

use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::repo::{
    self, create_area, create_card, create_link, create_project, create_proposal, create_work_item,
    list_projects, update_card, update_work_item, CardPatch, NewCard, NewLink, NewProposal,
    NewWorkItem, WorkItemPatch,
};
use korg_core::topics;
use korg_test_support::{fresh_korg, new};
use rust_decimal::Decimal;
use sqlx::PgPool;

fn wi(title: &str) -> NewWorkItem {
    NewWorkItem {
        wi_tshirt: "S".into(),
        content: "c".into(),
        ..new::work_item(title)
    }
}

fn card(title: &str) -> NewCard {
    new::card(title)
}

/// The error an operation failed with, classified.
fn code(e: &anyhow::Error) -> ErrorCode {
    e.code()
}

async fn project_count(pool: &PgPool) -> usize {
    list_projects(pool).await.expect("list projects").len()
}

#[tokio::test]
async fn every_write_accepts_a_project_name() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();

    let item = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            ..wi("by name")
        },
    )
    .await
    .expect("work item by project name");
    assert_eq!(item.project.as_deref(), Some("korg"));

    let c = create_card(
        &pool,
        NewCard {
            project: Some("korg".into()),
            ..card("card by name")
        },
    )
    .await
    .expect("card by project name");
    assert_eq!(c.project.as_deref(), Some("korg"));

    let link = create_link(
        &pool,
        NewLink {
            project_id: None,
            project: Some("korg".into()),
            category: None,
            tags: vec![],
            url: "https://example.invalid".into(),
            title: None,
        },
    )
    .await
    .expect("link by project name");
    // LinkRow doesn't carry the project name, so read it back off the node.
    let pid: Option<i64> = sqlx::query_scalar("SELECT project_id FROM node WHERE id = $1")
        .bind(link.node_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(pid.is_some(), "link should have been assigned a project");

    let topic = topics::create_topic(
        &pool,
        topics::NewTopic {
            project_id: None,
            project: Some("korg".into()),
            category: None,
            tags: vec![],
            name: "topic by name".into(),
            description: None,
        },
    )
    .await
    .expect("topic by project name");
    assert_eq!(topic.project.as_deref(), Some("korg"));

    let proposal = create_proposal(
        &pool,
        NewProposal {
            project_id: None,
            project: Some("korg".into()),
            category: None,
            tags: vec![],
            title: "p".into(),
            summary: "s".into(),
            rank: Decimal::ZERO,
            pinned: false,
            covers: vec![],
        },
    )
    .await
    .expect("proposal by project name");
    assert_eq!(proposal.row.project.as_deref(), Some("korg"));
}

#[tokio::test]
async fn patches_accept_a_project_name_and_null_still_unassigns() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();
    create_project(&pool, "kwi").await.unwrap();

    let item = create_work_item(&pool, NewWorkItem { ..wi("movable") })
        .await
        .unwrap();
    assert_eq!(item.project, None);

    // Move by name.
    let moved = update_work_item(
        &pool,
        item.wi_number,
        WorkItemPatch {
            project: Some(Some("korg".into())),
            ..Default::default()
        },
    )
    .await
    .expect("move by name");
    assert_eq!(moved.project.as_deref(), Some("korg"));

    // `project: null` unassigns, exactly as `project_id: null` does.
    let cleared = update_work_item(
        &pool,
        item.wi_number,
        WorkItemPatch {
            project: Some(None),
            ..Default::default()
        },
    )
    .await
    .expect("unassign by name-null");
    assert_eq!(cleared.project, None);

    // Same for cards.
    let c = create_card(&pool, card("movable card")).await.unwrap();
    let moved = update_card(
        &pool,
        c.node_id,
        CardPatch {
            project: Some(Some("kwi".into())),
            ..Default::default()
        },
    )
    .await
    .expect("move card by name");
    assert_eq!(moved.project.as_deref(), Some("kwi"));

    let cleared = update_card(
        &pool,
        c.node_id,
        CardPatch {
            project: Some(None),
            ..Default::default()
        },
    )
    .await
    .expect("unassign card");
    assert_eq!(cleared.project, None);
}

/// The whole point: an unresolvable name must fail loudly, and the message has
/// to say what to do next.
#[tokio::test]
async fn an_unknown_project_name_is_actionable_and_writes_nothing() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();
    let before = project_count(&pool).await;

    let err = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("nope".into()),
            ..wi("should not exist")
        },
    )
    .await
    .expect_err("unknown project name must fail");
    assert_eq!(code(&err), ErrorCode::InvalidInput);
    let msg = err.to_string();
    assert!(msg.contains("nope"), "names the bad value: {msg}");
    assert!(
        msg.contains("list_projects"),
        "names the remedy — the error doubles as the documentation: {msg}"
    );

    // WI #537 REGRESSION FENCE: resolving a name must never create a project.
    assert_eq!(
        project_count(&pool).await,
        before,
        "an unknown name created a project as a side effect — the exact bug WI #537 removed"
    );
    // ...and no work item either.
    let items = repo::list_work_items(&pool, repo::WorkItemQuery::default())
        .await
        .unwrap();
    assert_eq!(items.total, 0, "the failed create left a row behind");
}

/// A near-miss on casing is the realistic agent error, so it gets a pointed
/// suggestion rather than the generic pointer.
#[tokio::test]
async fn a_case_mismatch_suggests_the_real_name() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();

    let err = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("KORG".into()),
            ..wi("x")
        },
    )
    .await
    .expect_err("case mismatch must still fail — suggest, don't silently accept");
    let msg = err.to_string();
    assert!(msg.contains("did you mean 'korg'"), "{msg}");
}

/// Passing both halves is a conflict, not a precedence puzzle. A precedence
/// rule would silently discard one of two things the caller explicitly asked
/// for — the failure this whole change exists to remove.
#[tokio::test]
async fn passing_both_id_and_name_is_rejected() {
    let (_c, pool) = fresh_korg().await;
    let pid = create_project(&pool, "korg").await.unwrap();

    let err = create_work_item(
        &pool,
        NewWorkItem {
            project_id: Some(pid),
            project: Some("korg".into()),
            ..wi("ambiguous")
        },
    )
    .await
    .expect_err("both selectors must be rejected even when they agree");
    assert_eq!(code(&err), ErrorCode::InvalidInput);
    assert!(err.to_string().contains("not both"), "{err}");

    let item = create_work_item(&pool, NewWorkItem { ..wi("fine") })
        .await
        .unwrap();
    let err = update_work_item(
        &pool,
        item.wi_number,
        WorkItemPatch {
            project_id: Some(Some(pid)),
            project: Some(Some("korg".into())),
            ..Default::default()
        },
    )
    .await
    .expect_err("patch rejects both too");
    assert_eq!(code(&err), ErrorCode::InvalidInput);
}

/// A typo'd id used to reach the foreign key and come back as a raw Postgres
/// error in a 500 — the shape WI #524 fixed for `relate`'s endpoints.
#[tokio::test]
async fn an_unknown_project_id_is_invalid_input_not_a_database_error() {
    let (_c, pool) = fresh_korg().await;

    let err = create_work_item(
        &pool,
        NewWorkItem {
            project_id: Some(9999),
            ..wi("bad id")
        },
    )
    .await
    .expect_err("unknown project id must fail");
    assert_eq!(code(&err), ErrorCode::InvalidInput, "{err}");
    assert!(err.to_string().contains("9999"), "{err}");
    assert!(err.to_string().contains("list_projects"), "{err}");
}

/// A bad `area_id` used to answer `not_found`, which read as "the work item is
/// missing". It is the *input* that is wrong, so it now answers `invalid_input`
/// like every other unresolvable selector — one rule for all of them. This path
/// had no coverage at all before, which is how the inconsistency survived.
#[tokio::test]
async fn an_unknown_area_id_is_invalid_input() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();

    let err = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            area_id: Some(9999),
            ..wi("bad area id")
        },
    )
    .await
    .expect_err("unknown area id must fail");
    assert_eq!(code(&err), ErrorCode::InvalidInput, "{err}");
    assert!(err.to_string().contains("list_areas"), "{err}");
}

#[tokio::test]
async fn area_names_resolve_within_the_work_items_project() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();
    create_project(&pool, "kwi").await.unwrap();
    create_area(&pool, "korg", "ui", None).await.unwrap();
    create_area(&pool, "kwi", "ui", None).await.unwrap();

    // The same area name exists in both projects; the project decides which.
    let item = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            area: Some("ui".into()),
            ..wi("area by name")
        },
    )
    .await
    .expect("area by name");
    assert_eq!(item.project.as_deref(), Some("korg"));
    assert_eq!(item.area.as_deref(), Some("ui"));

    // An area name with no project cannot mean anything — say so plainly
    // rather than mysteriously finding nothing.
    let err = create_work_item(
        &pool,
        NewWorkItem {
            area: Some("ui".into()),
            ..wi("no project")
        },
    )
    .await
    .expect_err("area name without a project must fail");
    assert_eq!(code(&err), ErrorCode::InvalidInput);
    assert!(err.to_string().contains("without a project"), "{err}");

    // An unknown area name in a real project points at list_areas.
    let err = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            area: Some("nope".into()),
            ..wi("bad area")
        },
    )
    .await
    .expect_err("unknown area name must fail");
    assert!(err.to_string().contains("list_areas"), "{err}");

    let err = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            area_id: Some(1),
            area: Some("ui".into()),
            ..wi("both areas")
        },
    )
    .await
    .expect_err("both area selectors must be rejected");
    assert!(err.to_string().contains("not both"), "{err}");
}

/// Patching an area by name resolves against the project the item will have
/// *after* the update, not the one it had before — otherwise a move-and-retag
/// in one call would resolve against the wrong project.
#[tokio::test]
async fn a_patched_area_name_resolves_in_the_new_project() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();
    create_project(&pool, "kwi").await.unwrap();
    create_area(&pool, "korg", "ui", None).await.unwrap();
    let kwi_ui = create_area(&pool, "kwi", "ui", None).await.unwrap();

    let item = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("korg".into()),
            area: Some("ui".into()),
            ..wi("moving")
        },
    )
    .await
    .unwrap();

    // Move to kwi and set the area by name in the same call.
    let moved = update_work_item(
        &pool,
        item.wi_number,
        WorkItemPatch {
            project: Some(Some("kwi".into())),
            area: Some(Some("ui".into())),
            ..Default::default()
        },
    )
    .await
    .expect("move and retag in one call");
    assert_eq!(moved.project.as_deref(), Some("kwi"));
    assert_eq!(moved.area.as_deref(), Some("ui"));

    // It resolved to *kwi's* ui, not korg's.
    let area_id: Option<i64> =
        sqlx::query_scalar("SELECT area_id FROM workitem WHERE node_id = $1")
            .bind(moved.node_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        area_id,
        Some(kwi_ui),
        "the area name resolved in the old project, not the new one"
    );

    // `area: null` clears, like `area_id: null`.
    let cleared = update_work_item(
        &pool,
        item.wi_number,
        WorkItemPatch {
            area: Some(None),
            ..Default::default()
        },
    )
    .await
    .expect("clear area by name-null");
    assert_eq!(cleared.area, None);
}
