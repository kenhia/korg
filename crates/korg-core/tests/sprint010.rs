//! Sprint 010 — status vocabulary (WI #285), editable comments (WI #232),
//! project metadata (WI #246).

use korg_core::repo::{
    add_comment, create_project, create_work_item, list_comments, list_projects, update_comment,
    update_project_by_name, update_work_item, NewWorkItem, ProjectPatch, WorkItemPatch,
    WI_STATUSES,
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

fn wi(title: &str, project_id: i64, status: &str) -> NewWorkItem {
    NewWorkItem {
        project_id: Some(project_id),
        project: None,
        area_id: None,
        area: None,
        wi_type: "task".into(),
        wi_status: status.into(),
        wi_tshirt: "Unknown".into(),
        sprint: None,
        title: title.into(),
        content: "body".into(),
        details: None,
        category: None,
        tags: vec![],
    }
}

#[tokio::test]
async fn wi_status_vocabulary_is_enforced() {
    let (_c, pool) = fresh_korg().await;
    let pid = create_project(&pool, "p").await.unwrap();

    // Every canonical status is accepted at creation.
    for s in WI_STATUSES {
        create_work_item(&pool, wi(&format!("as {s}"), pid, s))
            .await
            .unwrap_or_else(|e| panic!("status '{s}' should be valid: {e}"));
    }

    // Dead vocabulary ("active"/"draft" once lived in the web constant) and
    // typos are rejected at creation…
    for s in ["active", "draft", "Done", "bogus"] {
        assert!(
            create_work_item(&pool, wi(&format!("as {s}"), pid, s))
                .await
                .is_err(),
            "status '{s}' should be rejected"
        );
    }

    // …and on update.
    let r = create_work_item(&pool, wi("patch me", pid, "open"))
        .await
        .unwrap();
    let ok = WorkItemPatch {
        wi_status: Some("done".into()),
        ..Default::default()
    };
    update_work_item(&pool, r.wi_number, ok).await.unwrap();
    let bad = WorkItemPatch {
        wi_status: Some("finished".into()),
        ..Default::default()
    };
    assert!(update_work_item(&pool, r.wi_number, bad).await.is_err());
}

#[tokio::test]
async fn comments_are_editable() {
    let (_c, pool) = fresh_korg().await;
    let pid = create_project(&pool, "p").await.unwrap();
    let r = create_work_item(&pool, wi("holder", pid, "open"))
        .await
        .unwrap();

    let c = add_comment(&pool, r.node_id, "forgot the WI #")
        .await
        .unwrap();
    let edited = update_comment(&pool, c.id, "refers to WI #42")
        .await
        .unwrap();

    assert_eq!(edited.id, c.id);
    assert_eq!(edited.body, "refers to WI #42");
    assert_eq!(edited.created, c.created, "created must be preserved");

    let listed = list_comments(&pool, r.node_id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].body, "refers to WI #42");

    assert!(update_comment(&pool, 999_999, "nope").await.is_err());
}

#[tokio::test]
async fn project_metadata_roundtrip() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "meta").await.unwrap();

    // Migration defaults.
    let p = list_projects(&pool).await.unwrap().remove(0);
    assert_eq!(p.status, "active");
    assert!(p.machines.is_empty() && p.deploy_to.is_empty());
    assert_eq!(p.category, None);

    update_project_by_name(
        &pool,
        "meta",
        &ProjectPatch {
            status: Some("maintenance".into()),
            machines: Some(vec!["kai".into(), "kubs0".into()]),
            deploy_to: Some(vec!["kubsdb".into()]),
            category: Some(Some("tooling".into())),
            description: Some(Some("desc".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let p = list_projects(&pool).await.unwrap().remove(0);
    assert_eq!(p.status, "maintenance");
    assert_eq!(p.machines, vec!["kai", "kubs0"]);
    assert_eq!(p.deploy_to, vec!["kubsdb"]);
    assert_eq!(p.category.as_deref(), Some("tooling"));

    // Invalid project status rejected; unknown project errors; name immutable
    // by construction (no field for it).
    let bad = ProjectPatch {
        status: Some("paused".into()),
        ..Default::default()
    };
    assert!(update_project_by_name(&pool, "meta", &bad).await.is_err());
    let ok = ProjectPatch {
        status: Some("active".into()),
        ..Default::default()
    };
    assert!(update_project_by_name(&pool, "nope", &ok).await.is_err());
}
