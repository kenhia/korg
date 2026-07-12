//! Sprint 012 — move a work item between projects (WI #291) and inline,
//! capped comments + comment_count on work-item reads (WI #392).

use korg_core::repo::{
    add_comment, create_area, create_project, create_work_item, get_work_item,
    get_work_item_detail, list_work_items, update_work_item, NewWorkItem, RepoError, WorkItemPatch,
    WORKITEM_COMMENT_CAP,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ImageExt;

async fn fresh_korg() -> (impl Sized, PgPool) {
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect");
    korg_core::migrator().run(&pool).await.expect("migrate");
    (container, pool)
}

fn new_wi(project_id: i64, area_id: Option<i64>) -> NewWorkItem {
    NewWorkItem {
        project_id: Some(project_id),
        area_id,
        wi_type: "task".into(),
        wi_status: "open".into(),
        wi_tshirt: "S".into(),
        sprint: None,
        title: "t".into(),
        content: "c".into(),
        details: None,
        category: None,
        tags: vec![],
    }
}

#[tokio::test]
async fn move_between_projects_and_area_validation() {
    let (_c, pool) = fresh_korg().await;
    let pa = create_project(&pool, "A").await.unwrap();
    let pb = create_project(&pool, "B").await.unwrap();
    let area_a = create_area(&pool, "A", "ui", None).await.unwrap();
    let area_b = create_area(&pool, "B", "backend", None).await.unwrap();

    let wi = create_work_item(&pool, new_wi(pa, Some(area_a))).await.unwrap();

    // Move to B without naming an area → the now-foreign area is dropped.
    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            project_id: Some(Some(pb)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let got = get_work_item(&pool, wi.wi_number).await.unwrap().unwrap();
    assert_eq!(got.project.as_deref(), Some("B"));
    assert_eq!(got.area, None, "stale area cleared on move");

    // Setting an area from the wrong project is rejected as InvalidInput.
    let bad = update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            area_id: Some(Some(area_a)),
            ..Default::default()
        },
    )
    .await;
    let err = bad.expect_err("area from another project must be rejected");
    assert!(matches!(
        err.downcast_ref::<RepoError>(),
        Some(RepoError::InvalidInput(_))
    ));

    // Correct area for the current project works.
    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            area_id: Some(Some(area_b)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get_work_item(&pool, wi.wi_number).await.unwrap().unwrap().area.as_deref(),
        Some("backend")
    );

    // Move back to A while supplying a valid area in the same call.
    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            project_id: Some(Some(pa)),
            area_id: Some(Some(area_a)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let got = get_work_item(&pool, wi.wi_number).await.unwrap().unwrap();
    assert_eq!(got.project.as_deref(), Some("A"));
    assert_eq!(got.area.as_deref(), Some("ui"));
}

#[tokio::test]
async fn get_work_item_detail_inlines_capped_comments() {
    let (_c, pool) = fresh_korg().await;
    let p = create_project(&pool, "P").await.unwrap();
    let wi = create_work_item(&pool, new_wi(p, None)).await.unwrap();

    // No comments: empty, not truncated, count 0.
    let d = get_work_item_detail(&pool, wi.wi_number).await.unwrap().unwrap();
    assert_eq!(d.item.comment_count, 0);
    assert!(d.comments.is_empty());
    assert!(!d.comments_truncated);

    // More than the cap: capped list, truncated flag, true total.
    let over = WORKITEM_COMMENT_CAP + 2;
    for i in 0..over {
        add_comment(&pool, wi.node_id, &format!("c{i}")).await.unwrap();
    }
    let d = get_work_item_detail(&pool, wi.wi_number).await.unwrap().unwrap();
    assert_eq!(d.item.comment_count, over);
    assert_eq!(d.comments.len() as i64, WORKITEM_COMMENT_CAP);
    assert!(d.comments_truncated);

    // list_work_items carries the count too.
    let items = list_work_items(&pool).await.unwrap();
    let row = items.iter().find(|w| w.wi_number == wi.wi_number).unwrap();
    assert_eq!(row.comment_count, over);

    // Missing work item → None.
    assert!(get_work_item_detail(&pool, 999_999).await.unwrap().is_none());
}
